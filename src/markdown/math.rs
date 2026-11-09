//! TeX math extraction and MathML rendering.
//!
//! Model answers use four common delimiter styles. Markdown treats the
//! LaTeX forms as backslash escapes, so the renderer extracts formulas before
//! parsing and inserts ASCII placeholders. After the Markdown pass the
//! placeholders become MathML. Code spans, code blocks, and HTML attributes
//! keep the original TeX.

use pulldown_latex::config::{DisplayMode, RenderConfig};
use pulldown_latex::{Parser, Storage, push_mathml};

/// Prefix of a placeholder. ASCII survives URL percent-encoding.
const PREFIX: &str = "DOCENTMATHSLOT";
/// Suffix of a placeholder.
const SUFFIX: &str = "END";

/// One formula found in the answer text.
pub(super) struct Span {
    /// The original source, including its delimiters.
    original: String,
    /// Rendered MathML, or escaped TeX when rendering failed.
    rendered: String,
}

/// Extract formulas and replace them with placeholders.
pub(super) fn extract(text: &str) -> (String, Vec<Span>) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut spans: Vec<Span> = Vec::new();
    let mut index = 0;
    let mut fence: Option<(char, usize)> = None;

    while index < chars.len() {
        if let Some((open, open_len)) = fence {
            if line_start(&chars, index) {
                if let Some((close, close_len, marker)) = fence_marker(&chars, index) {
                    let end = line_end(&chars, index);
                    if close == open
                        && close_len >= open_len
                        && blank(&chars, marker + close_len, end)
                    {
                        fence = None;
                    }
                }
            }
            copy_line(&mut out, &chars, &mut index);
            continue;
        }

        if line_start(&chars, index) {
            if let Some((marker, len, _)) = fence_marker(&chars, index) {
                fence = Some((marker, len));
                copy_line(&mut out, &chars, &mut index);
                continue;
            }
        }

        match chars[index] {
            '`' => {
                let len = run_len(&chars, index, '`');
                if let Some(close) = find_run(&chars, index + len, '`', len) {
                    let end = close + len;
                    copy_range(&mut out, &chars, index, end);
                    index = end;
                } else {
                    copy_range(&mut out, &chars, index, index + len);
                    index += len;
                }
            }
            '\\' => index = backslash(&chars, index, &mut out, &mut spans),
            '$' => index = dollar(&chars, index, &mut out, &mut spans),
            _ => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }

    (out, spans)
}

/// Replace placeholders with rendered MathML, or restore TeX inside code and
/// HTML tags.
pub(super) fn substitute(html: &str, spans: &[Span]) -> String {
    let extra: usize = spans.iter().map(|span| span.rendered.len()).sum();
    let mut out = String::with_capacity(html.len() + extra);
    let mut code_depth = 0usize;
    let mut index = 0;

    while index < html.len() {
        if html[index..].starts_with(PREFIX) {
            if let Some((number, end)) = placeholder(html, index) {
                if let Some(span) = spans.get(number) {
                    if code_depth > 0 {
                        out.push_str(&escape_text(&span.original));
                    } else {
                        out.push_str(&span.rendered);
                    }
                    index = end;
                    continue;
                }
            }
        }
        let ch = html[index..].chars().next().expect("index is char aligned");
        if ch == '<' {
            let end = html[index..]
                .find('>')
                .map_or(html.len(), |offset| index + offset + 1);
            let tag = &html[index..end];
            if is_code_open(tag) {
                code_depth += 1;
            } else if is_code_close(tag) {
                code_depth = code_depth.saturating_sub(1);
            }
            out.push_str(&restore_in_tag(tag, spans));
            index = end;
            continue;
        }
        out.push(ch);
        index += ch.len_utf8();
    }

    out
}

/// Handle a backslash escape, delimiter, or literal character.
fn backslash(chars: &[char], index: usize, out: &mut String, spans: &mut Vec<Span>) -> usize {
    match chars.get(index + 1) {
        Some('(') => {
            if let Some(close) = find_escape_close(chars, index + 2, ')') {
                if has_blank_line(chars, index + 2, close) {
                    out.push('\\');
                    index + 1
                } else {
                    add_span(chars, index, close + 2, false, false, out, spans);
                    close + 2
                }
            } else {
                out.push('\\');
                index + 1
            }
        }
        Some('[') => {
            if let Some(close) = find_escape_close(chars, index + 2, ']') {
                if has_blank_line(chars, index + 2, close) {
                    out.push('\\');
                    index + 1
                } else {
                    add_span(chars, index, close + 2, true, false, out, spans);
                    close + 2
                }
            } else {
                out.push('\\');
                index + 1
            }
        }
        Some('$') => {
            out.push('\\');
            out.push('$');
            index + 2
        }
        Some('\\') => {
            out.push('\\');
            out.push('\\');
            index + 2
        }
        _ => {
            out.push('\\');
            index + 1
        }
    }
}

/// Handle `$...$` and `$$...$$`.
fn dollar(chars: &[char], index: usize, out: &mut String, spans: &mut Vec<Span>) -> usize {
    let run = run_len(chars, index, '$');

    if run >= 2 {
        if let Some(close) = find_dollar_close(chars, index + 2) {
            if !has_blank_line(chars, index + 2, close) {
                let content = chars[index + 2..close]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string();
                if !content.is_empty() {
                    add_span(chars, index, close + 2, true, true, out, spans);
                    return close + 2;
                }
            }
        }
        copy_range(out, chars, index, index + run);
        return index + run;
    }

    if chars.get(index + 1).is_none_or(|next| next.is_whitespace()) {
        out.push('$');
        return index + 1;
    }

    let Some(close) = find_inline_close(chars, index + 1) else {
        out.push('$');
        return index + 1;
    };
    if close == index + 1
        || chars[close - 1].is_whitespace()
        || chars
            .get(close + 1)
            .is_some_and(|next| next.is_ascii_digit())
        || has_blank_line(chars, index + 1, close)
    {
        out.push('$');
        return index + 1;
    }

    let content = chars[index + 1..close]
        .iter()
        .collect::<String>()
        .trim()
        .to_string();
    if content.is_empty() || !looks_like_math(&content) {
        out.push('$');
        return index + 1;
    }

    add_span(chars, index, close + 1, false, true, out, spans);
    close + 1
}

/// Push a span and append its placeholder.
fn add_span(
    chars: &[char],
    start: usize,
    end: usize,
    display: bool,
    dollars: bool,
    out: &mut String,
    spans: &mut Vec<Span>,
) {
    let original: String = chars[start..end].iter().collect();
    let (content_start, content_end) = if dollars {
        (
            start + if display { 2 } else { 1 },
            end - if display { 2 } else { 1 },
        )
    } else {
        (start + 2, end - 2)
    };
    let tex = chars[content_start..content_end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string();
    if tex.is_empty() {
        out.extend(chars[start..end].iter());
        return;
    }

    let rendered = render_math(&tex, &original, display);
    out.push_str(PREFIX);
    out.push_str(&spans.len().to_string());
    out.push_str(SUFFIX);
    spans.push(Span { original, rendered });
}

/// Render one formula as MathML, or escaped TeX on failure.
fn render_math(tex: &str, original: &str, display: bool) -> String {
    let storage = Storage::new();
    let parser = Parser::new(tex, &storage);
    let config = RenderConfig {
        display_mode: if display {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        annotation: None,
        ..RenderConfig::default()
    };
    let mut mathml = String::new();
    if push_mathml(&mut mathml, parser, config).is_ok() && !mathml.contains("<merror") {
        let mathml = sanitize_mathml(&mathml);
        let class = if display {
            "math math-display"
        } else {
            "math math-inline"
        };
        format!("<span class=\"{class}\">{mathml}</span>")
    } else {
        let class = if display {
            "math math-display math-source"
        } else {
            "math math-inline math-source"
        };
        format!("<span class=\"{class}\">{}</span>", escape_text(original))
    }
}

/// True for inline `$...$` content that looks mathematical rather than a
/// currency amount.
fn looks_like_math(content: &str) -> bool {
    content.chars().any(|ch| {
        ch.is_alphabetic()
            || matches!(
                ch,
                '\\' | '{'
                    | '}'
                    | '_'
                    | '^'
                    | '='
                    | '<'
                    | '>'
                    | '±'
                    | '≤'
                    | '≥'
                    | '≠'
                    | '∞'
                    | '√'
                    | '∑'
                    | '∫'
                    | '→'
                    | '∈'
            )
    })
}

/// Find an unescaped `\)` or `\]` closing a LaTeX form.
fn find_escape_close(chars: &[char], start: usize, close: char) -> Option<usize> {
    let mut index = start;
    while index + 1 < chars.len() {
        if chars[index] == '\\' {
            if chars[index + 1] == close {
                return Some(index);
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    None
}

/// Find the closing `$$` of a display formula.
fn find_dollar_close(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    while index + 1 < chars.len() {
        if chars[index] == '\\' {
            index += 2;
            continue;
        }
        if chars[index] == '$' && chars[index + 1] == '$' {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Find the closing `$` of an inline formula.
fn find_inline_close(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    while index < chars.len() {
        if chars[index] == '\\' {
            index += 2;
            continue;
        }
        if chars[index] == '$' {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// True when the range contains a blank line.
fn has_blank_line(chars: &[char], start: usize, end: usize) -> bool {
    let mut index = start;
    while index + 1 < end {
        if chars[index] == '\n' {
            let mut next = index + 1;
            while next < end && (chars[next] == ' ' || chars[next] == '\t') {
                next += 1;
            }
            if next < end && chars[next] == '\n' {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// Count a run of one character.
fn run_len(chars: &[char], start: usize, needle: char) -> usize {
    chars[start..]
        .iter()
        .take_while(|&&ch| ch == needle)
        .count()
}

/// Find a later run of exactly `len` backticks.
fn find_run(chars: &[char], start: usize, needle: char, len: usize) -> Option<usize> {
    let mut index = start;
    while index < chars.len() {
        if chars[index] == needle {
            let run = run_len(chars, index, needle);
            if run == len {
                return Some(index);
            }
            index += run;
        } else {
            index += 1;
        }
    }
    None
}

/// True at the start of a line.
fn line_start(chars: &[char], index: usize) -> bool {
    index == 0 || chars[index - 1] == '\n'
}

/// End index of the current line, excluding the newline.
fn line_end(chars: &[char], index: usize) -> usize {
    chars[index..]
        .iter()
        .position(|&ch| ch == '\n')
        .map_or(chars.len(), |offset| index + offset)
}

/// Copy the rest of the current line.
fn copy_line(out: &mut String, chars: &[char], index: &mut usize) {
    let end = line_end(chars, *index);
    out.extend(chars[*index..end].iter());
    if end < chars.len() {
        out.push('\n');
        *index = end + 1;
    } else {
        *index = end;
    }
}

/// Copy a range of characters.
fn copy_range(out: &mut String, chars: &[char], start: usize, end: usize) {
    out.extend(chars[start..end].iter());
}

/// A fence marker and its marker position.
fn fence_marker(chars: &[char], index: usize) -> Option<(char, usize, usize)> {
    let mut position = index;
    let mut spaces = 0;
    while position < chars.len() && chars[position] == ' ' && spaces < 4 {
        position += 1;
        spaces += 1;
    }
    if spaces > 3 {
        return None;
    }
    let marker = *chars.get(position)?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = run_len(chars, position, marker);
    if len < 3 {
        return None;
    }
    Some((marker, len, position))
}

/// True when every character in the range is whitespace.
fn blank(chars: &[char], start: usize, end: usize) -> bool {
    chars[start..end].iter().all(|ch| ch.is_whitespace())
}

/// Parse a placeholder at `index`.
fn placeholder(html: &str, index: usize) -> Option<(usize, usize)> {
    let rest = html.get(index..)?.strip_prefix(PREFIX)?;
    let length = rest.find(SUFFIX)?;
    let digits = &rest[..length];
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse().ok()?;
    Some((number, index + PREFIX.len() + length + SUFFIX.len()))
}

/// Restore placeholders inside an HTML tag or attribute.
fn restore_in_tag(tag: &str, spans: &[Span]) -> String {
    if !tag.contains(PREFIX) {
        return tag.to_string();
    }
    let mut out = String::with_capacity(tag.len());
    let mut index = 0;
    while index < tag.len() {
        if tag[index..].starts_with(PREFIX) {
            if let Some((number, end)) = placeholder(tag, index) {
                if let Some(span) = spans.get(number) {
                    out.push_str(&escape_attribute(&span.original));
                    index = end;
                    continue;
                }
            }
        }
        let ch = tag[index..].chars().next().expect("index is char aligned");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// True for opening `<code>` or `<pre>` tags.
fn is_code_open(tag: &str) -> bool {
    starts_tag(tag, "code") || starts_tag(tag, "pre")
}

/// True for closing `</code>` or `</pre>` tags.
fn is_code_close(tag: &str) -> bool {
    tag.starts_with("</code") || tag.starts_with("</pre")
}

/// True when a tag starts with `<name>` or `<name ...>`.
fn starts_tag(tag: &str, name: &str) -> bool {
    let Some(rest) = tag
        .strip_prefix('<')
        .and_then(|rest| rest.strip_prefix(name))
    else {
        return false;
    };
    rest.starts_with('>') || rest.starts_with(char::is_whitespace)
}

/// Escape text inserted as element content.
fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape text inserted inside an attribute.
fn escape_attribute(value: &str) -> String {
    escape_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escape text nodes and drop unsafe tags from generated MathML.
fn sanitize_mathml(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut index = 0;
    while index < html.len() {
        let ch = html[index..].chars().next().expect("index is char aligned");
        if ch == '<' {
            if let Some((tag, end)) = parse_tag(html, index) {
                if safe_tag(tag) {
                    out.push('<');
                    out.push_str(tag);
                    out.push('>');
                    index = end;
                    continue;
                }
            }
            out.push_str("&lt;");
            index += 1;
            continue;
        }
        if ch == '&' {
            if let Some((entity, end)) = parse_entity(html, index) {
                out.push_str(entity);
                index = end;
                continue;
            }
            out.push_str("&amp;");
            index += 1;
            continue;
        }
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// Split a tag from the input, or return None when `<` starts text.
fn parse_tag(input: &str, start: usize) -> Option<(&str, usize)> {
    let rest = input.get(start + 1..)?;
    let first = rest.chars().next()?;
    let name_start = usize::from(first == '/');
    let name = rest.get(name_start..)?.chars().next()?;
    if !name.is_ascii_alphabetic() {
        return None;
    }
    let end = rest.find('>')?;
    Some((&rest[..end], start + end + 2))
}

/// Split an entity from the input when it is one this renderer emits.
fn parse_entity(input: &str, start: usize) -> Option<(&str, usize)> {
    let end = input[start..].find(';')?;
    let entity = &input[start..start + end + 1];
    safe_entity(entity).then_some((entity, start + end + 1))
}

/// Validate one tag and its attributes.
fn safe_tag(tag: &str) -> bool {
    let tag = tag.trim();
    let tag = tag.strip_suffix('/').unwrap_or(tag).trim_end();
    let (closing, rest) = match tag.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, tag),
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    if !ALLOWED_ELEMENTS.contains(&name) {
        return false;
    }
    if closing {
        return parts.next().is_none_or(|rest| rest.trim().is_empty());
    }

    let mut rest = parts.next().unwrap_or("").trim();
    while !rest.is_empty() {
        let Some(equals) = rest.find('=') else {
            return false;
        };
        let name = rest[..equals].trim();
        if !ALLOWED_ATTRIBUTES.contains(&name) {
            return false;
        }
        let Some(value) = rest[equals + 1..].trim_start().strip_prefix('"') else {
            return false;
        };
        let Some(end) = value.find('"') else {
            return false;
        };
        if !safe_attribute(name, &value[..end]) {
            return false;
        }
        rest = value[end + 1..].trim_start();
    }
    true
}

/// Restrict attribute values to the characters this renderer emits.
fn safe_attribute(name: &str, value: &str) -> bool {
    let characters_ok = value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                ' ' | '#' | '%' | '.' | ',' | ':' | ';' | '+' | '-' | '(' | ')' | '/' | '_'
            )
    });
    if !characters_ok {
        return false;
    }
    if name != "style" {
        return true;
    }
    let value = value.to_ascii_lowercase();
    !value.contains("url(")
        && !value.contains("expression(")
        && !value.contains("image-set(")
        && !value.contains("@import")
}

/// Allow only the entities this renderer emits.
fn safe_entity(entity: &str) -> bool {
    matches!(entity, "&amp;" | "&lt;" | "&gt;" | "&nbsp;")
        || entity
            .strip_prefix("&#")
            .and_then(|rest| rest.strip_suffix(';'))
            .is_some_and(|digits| {
                let digits = digits.strip_prefix('x').unwrap_or(digits);
                !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_hexdigit())
            })
}

/// Elements emitted by `pulldown-latex`.
const ALLOWED_ELEMENTS: &[&str] = &[
    "annotation",
    "math",
    "menclose",
    "merror",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "msqrt",
    "mspace",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "none",
    "semantics",
];

/// Attributes emitted by `pulldown-latex`, plus safe MathML layout hints.
const ALLOWED_ATTRIBUTES: &[&str] = &[
    "class",
    "columnalign",
    "columnlines",
    "columnspacing",
    "depth",
    "display",
    "displaystyle",
    "encoding",
    "equalcolumns",
    "equalrows",
    "frame",
    "framespacing",
    "height",
    "largeop",
    "linethickness",
    "mathbackground",
    "mathcolor",
    "mathvariant",
    "maxsize",
    "minsize",
    "movablelimits",
    "rowalign",
    "rowlines",
    "rowspacing",
    "scriptlevel",
    "stretchy",
    "style",
    "symmetric",
    "width",
    "xmlns",
];

#[cfg(test)]
mod tests {
    fn rendered(text: &str) -> String {
        crate::markdown::render(text)
    }

    #[test]
    fn renders_inline_parentheses() {
        let html = rendered("the model is \\(\\pi_{\\text{ref}}\\) here");
        assert!(html.contains("math math-inline"), "{html}");
        assert!(html.contains("<math display=\"inline\""), "{html}");
        assert!(!html.contains("\\("), "{html}");
        assert!(!html.contains("merror"), "{html}");
        assert!(!html.contains("math-source"), "{html}");
    }

    #[test]
    fn renders_display_brackets() {
        let html = rendered(
            "\\[\n r_t = r_\\varphi(q,o_{\\le t}) - \\beta \\log \\frac{\\pi_\\theta(o_t\\mid q,o_{<t})}{\\pi_{\\text{ref}}(o_t\\mid q,o_{<t})}\n \\]",
        );
        assert!(html.contains("math math-display"), "{html}");
        assert!(html.contains("<math display=\"block\""), "{html}");
        assert!(html.contains("&lt;"), "{html}");
        assert!(!html.contains("merror"), "{html}");
        assert!(!html.contains("math-source"), "{html}");
    }

    #[test]
    fn renders_dollar_and_double_dollar() {
        let html = rendered("inline $x^2$ and display $$\\sum_{i=1}^n i$$");
        assert!(html.contains("<math display=\"inline\""), "{html}");
        assert!(html.contains("<math display=\"block\""), "{html}");
    }

    #[test]
    fn keeps_code_literal() {
        let html = rendered("`\\(x\\)` and\n\n```text\n\\(y\\) $z$\n```\n");
        assert!(html.contains("<code>\\(x\\)</code>"), "{html}");
        assert!(html.contains("\\(y\\) $z$"), "{html}");
    }

    #[test]
    fn keeps_currency_literal() {
        let html = rendered("it costs $100-$200 or US$100 today");
        assert!(!html.contains("<math"), "{html}");
        assert!(html.contains("$100-$200"), "{html}");
    }

    #[test]
    fn escapes_the_fallback() {
        let html = rendered("\\(\\unknowncommand{a<b}\\)");
        assert!(html.contains("math-source"), "{html}");
        assert!(html.contains("&lt;"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn escapes_injected_tags() {
        let html = rendered("\\(\\operatorname{<img src=x onerror=y>}\\)");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;img"), "{html}");
    }

    #[test]
    fn escapes_remote_style_urls() {
        let html = rendered(
            "\\(\\operatorname{</mi><mi style=\"background:url(https://example.com/x)\">x</mi><mi>}\\)",
        );
        assert!(!html.contains("<mi "), "{html}");
        assert!(html.contains("&lt;mi style"), "{html}");
    }

    #[test]
    fn keeps_safe_style_attributes() {
        let html = rendered("\\(\\color{red}{x}\\)");
        assert!(html.contains("<math"), "{html}");
        assert!(!html.contains("math-source"), "{html}");
    }

    #[test]
    fn restores_math_inside_a_link_destination() {
        let html = rendered("[x](https://example.com/$y$)");
        assert!(!html.contains("<span"), "{html}");
        assert!(html.contains("$y$"), "{html}");
    }

    #[test]
    fn keeps_escaped_delimiters_literal() {
        let html = rendered("\\\\(x\\\\) and \\$5");
        assert!(!html.contains("<math"), "{html}");
    }

    #[test]
    fn rejects_blank_line_spans() {
        let html = rendered("\\(x\n\ny\\)");
        assert!(!html.contains("<math"), "{html}");
    }

    #[test]
    fn accepts_spaces_inside_latex_delimiters() {
        let html = rendered("\\( \\pi \\)");
        assert!(html.contains("<math"), "{html}");
    }
}
