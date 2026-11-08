//! Standalone answer page served by the CLI browser viewer.

/// Default answer styles shared with the qutebrowser adapter.
const DEFAULT_CSS: &str = include_str!("../../plugins/qutebrowser/docent/answer.css");

const SSE_SCRIPT: &str = r#"
let answerRoot = document.getElementById('answer');
const status = document.getElementById('status');
const source = new EventSource('events');

function atBottom() {
  return (window.innerHeight + window.scrollY) >= (document.documentElement.scrollHeight - 8);
}

source.onmessage = (message) => {
  const event = JSON.parse(message.data);
  if (event.type === 'delta') {
    if (answerRoot.dataset.placeholder === '1') {
      answerRoot.textContent = '';
      answerRoot.dataset.placeholder = '0';
    }
    const stick = atBottom();
    answerRoot.appendChild(document.createTextNode(event.text));
    if (stick) window.scrollTo(0, document.documentElement.scrollHeight);
  } else if (event.type === 'html') {
    const stick = atBottom();
    const block = document.createElement('div');
    block.id = 'answer';
    block.className = 'markdown';
    block.innerHTML = event.html;
    answerRoot.replaceWith(block);
    answerRoot = block;
    if (stick) window.scrollTo(0, document.documentElement.scrollHeight);
  } else if (event.type === 'done') {
    status.textContent = event.label || 'done';
    document.title = event.label || 'docent';
    source.close();
  } else if (event.type === 'error') {
    status.textContent = 'docent: error';
    const error = document.createElement('span');
    error.className = 'error';
    error.textContent = '\n\n' + event.message;
    answerRoot.appendChild(error);
    source.close();
  }
};

source.onerror = () => { status.textContent = 'connection lost'; };
"#;

/// Return the standalone answer page.
pub(super) fn build_page(profile: &str, header: &str, custom_css: Option<&str>) -> String {
    let mut label = format!("docent: {}", escape(profile));
    if !header.is_empty() {
        label = format!("{label} - {}", escape(header));
    }
    let custom = custom_css.unwrap_or("");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="color-scheme" content="light dark">
<title>{label}</title>
<style>
{DEFAULT_CSS}
{custom}
</style>
</head>
<body>
<header>
  <span id="status">{label}</span>
</header>
<main><pre id="answer" data-placeholder="1">Thinking...</pre></main>
<script>
{SSE_SCRIPT}
</script>
</body>
</html>
"#
    )
}

/// Escape text inserted into HTML content or an attribute.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_the_profile_and_header() {
        let page = build_page("ex<script>", "a&b", None);
        assert!(page.contains("docent: ex&lt;script&gt;"));
        assert!(page.contains("a&amp;b"));
        assert!(!page.contains("<script>alert"));
    }

    #[test]
    fn appends_custom_css_after_the_defaults() {
        let page = build_page("explain", "notes.md", Some(".markdown h2 { color: red; }"));
        let default_at = page.find("--docent-font-body").unwrap();
        let custom_at = page.find(".markdown h2 { color: red; }").unwrap();
        assert!(default_at < custom_at);
    }

    #[test]
    fn starts_the_event_source_and_uses_the_answer_shell() {
        let page = build_page("explain", "", None);
        assert!(page.contains("<pre id=\"answer\" data-placeholder=\"1\">"));
        assert!(page.contains("new EventSource('events')"));
        assert!(page.contains("block.id = 'answer'"));
        assert!(page.contains("block.className = 'markdown'"));
    }

    #[test]
    fn label_without_a_header_is_just_the_profile() {
        let page = build_page("explain", "", None);
        assert!(page.contains("<title>docent: explain</title>"));
        assert!(page.contains("<span id=\"status\">docent: explain</span>"));
    }
}
