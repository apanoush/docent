//! Markdown rendering for answer pages and the NDJSON `html` event.
//!
//! Only the constructs that show up in model answers are enabled. The
//! renderer emits a small, fixed set of HTML tags. A filtering pass keeps the
//! same policy as the browser adapters used before the renderer moved into the
//! CLI: raw HTML is shown as text, images stay alt text, and only `http`,
//! `https`, and `mailto` URLs become links.
//!
//! TeX math is extracted before parsing and rendered as MathML after the
//! Markdown pass. Code spans, code blocks, and link destinations keep the
//! original TeX.

mod math;

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// Convert a Markdown subset to a safe HTML fragment.
pub fn render(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }

    let (prepared, math_spans) = math::extract(text);
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(&prepared, options);
    let mut events: Vec<Event<'_>> = Vec::with_capacity(prepared.len() / 8);
    let mut unsafe_link: Option<String> = None;

    for event in parser {
        match event {
            Event::Html(value) | Event::InlineHtml(value) => {
                // Show raw HTML literally instead of passing it through.
                events.push(Event::Text(value));
            }
            Event::Start(Tag::Image { .. }) => {
                // Drop the image tag and keep its alt text.
            }
            Event::End(TagEnd::Image) => {}
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                if is_safe_url(&dest_url) {
                    events.push(Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }));
                } else {
                    unsafe_link = Some(dest_url.to_string());
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = unsafe_link.take() {
                    events.push(Event::Text(CowStr::Boxed(
                        format!(" ({url})").into_boxed_str(),
                    )));
                } else {
                    events.push(Event::End(TagEnd::Link));
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                events.push(Event::Html(CowStr::Borrowed("<div class=\"table-wrap\">")));
                events.push(Event::Start(Tag::Table(alignments)));
            }
            Event::End(TagEnd::Table) => {
                events.push(Event::End(TagEnd::Table));
                events.push(Event::Html(CowStr::Borrowed("</div>")));
            }
            other => events.push(other),
        }
    }

    let mut output = String::with_capacity(prepared.len());
    html::push_html(&mut output, events.into_iter());
    // The only anchor tags left are the ones this renderer emitted, because
    // raw HTML was converted to text above.
    let output = output.replace(
        "<a href=\"",
        "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"",
    );
    math::substitute(&output, &math_spans)
}

/// True for the link schemes the answer page is allowed to activate.
fn is_safe_url(url: &str) -> bool {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("mailto") {
        return !rest.trim().is_empty();
    }
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let Some(authority) = rest.strip_prefix("//") else {
        return false;
    };
    let host = authority.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
}

/// Render an answer, falling back to a placeholder when the model was silent.
pub fn render_answer(text: &str) -> String {
    let body = render(text);
    if body.trim().is_empty() {
        "<p>(no output)</p>".to_string()
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_paragraphs_and_emphasis() {
        let output = render("# Title\n\nHello **world** and *you*.");
        assert!(output.contains("<h1>Title</h1>"));
        assert!(output.contains("<strong>world</strong>"));
        assert!(output.contains("<em>you</em>"));
    }

    #[test]
    fn renders_lists_code_and_strikethrough() {
        let output = render("- one\n- two\n\n```rust\nlet x = 1;\n```\n\n~~gone~~");
        assert!(output.contains("<ul>"));
        assert!(output.contains("<li>one</li>"));
        assert!(output.contains("<code class=\"language-rust\">let x = 1;"));
        assert!(output.contains("<del>gone</del>"));
    }

    #[test]
    fn renders_tables_with_the_shared_wrapper() {
        let output = render("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(output.contains("<div class=\"table-wrap\"><table>"));
        assert!(output.contains("</table>"));
        assert!(output.contains("</div>"));
        assert!(output.contains("<th>a</th>"));
        assert!(output.contains("<td>2</td>"));
    }

    #[test]
    fn keeps_raw_html_literal() {
        let output = render("<script>alert('x')</script>\n\n<img src=x onerror=y>");
        assert!(!output.contains("<script>"));
        assert!(!output.contains("<img"));
        assert!(output.contains("&lt;script&gt;"));
    }

    #[test]
    fn drops_images_to_alt_text() {
        let output = render("![a diagram](https://example.com/diagram.png)");
        assert!(!output.contains("<img"));
        assert!(!output.contains("diagram.png"));
        assert!(output.contains("a diagram"));
    }

    #[test]
    fn links_safe_schemes_and_drops_others() {
        let safe = render("[docs](https://example.com/a)");
        assert!(safe.contains(
            "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"https://example.com/a\">docs</a>"
        ));

        let mail = render("[mail](mailto:me@example.com)");
        assert!(mail.contains("href=\"mailto:me@example.com\""));

        let unsafe_link = render("[click](javascript:alert(1))");
        assert!(!unsafe_link.contains("<a "));
        assert!(unsafe_link.contains("click (javascript:alert(1))"));

        let relative = render("[local](/notes)");
        assert!(!relative.contains("<a "));
        assert!(relative.contains("local (/notes)"));
    }

    #[test]
    fn returns_empty_for_blank_input() {
        assert_eq!(render(""), "");
        assert_eq!(render("  \n\t\n"), "");
    }

    #[test]
    fn handles_crlf_and_nested_lists() {
        let output = render("- one\r\n  - nested\r\n- two\r\n");
        assert!(output.contains("<ul>"));
        assert!(output.contains("<li>nested</li>"));
        assert!(output.contains("<li>two</li>"));
    }
}
