"""Answer-tab shell and streaming DOM updates for the qutebrowser adapter."""

from __future__ import annotations

import html
import json
import re
import time
from typing import Any

from . import markdown

__all__ = ["AnswerPage", "build_shell"]

_SAFE_COLOR = re.compile(r"^[#a-zA-Z0-9(),.% -]+$")

_DEFAULT_COLORS = {
    "bg": "#1e1e1e",
    "fg": "#e6e6e6",
    "error": "#ff6b6b",
}


def build_shell(profile: str, colors: dict[str, str] | None = None) -> str:
    """Return the small HTML page used to receive streamed text."""
    palette = dict(_DEFAULT_COLORS)
    if colors:
        for name, fallback in _DEFAULT_COLORS.items():
            value = colors.get(name)
            if isinstance(value, str) and _SAFE_COLOR.fullmatch(value.strip()):
                palette[name] = value.strip()
            else:
                palette[name] = fallback

    escaped_profile = html.escape(profile, quote=True)
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>docent: {escaped_profile}</title>
<style>
  :root {{
    color-scheme: light dark;
    --docent-bg: {palette["bg"]};
    --docent-fg: {palette["fg"]};
    --docent-error: {palette["error"]};
  }}
  html, body {{
    margin: 0;
    padding: 0;
    background: var(--docent-bg);
    color: var(--docent-fg);
  }}
  body {{
    font-family: ui-sans-serif, system-ui, sans-serif;
    line-height: 1.5;
  }}
  header {{
    position: sticky;
    top: 0;
    display: flex;
    gap: 1rem;
    align-items: baseline;
    padding: 0.6rem 1.5rem;
    background: var(--docent-bg);
    border-bottom: 1px solid color-mix(in srgb, var(--docent-fg) 20%, transparent);
  }}
  #status {{ font-weight: 600; }}
  #hint {{ margin-left: auto; opacity: 0.55; font-size: 0.9em; }}
  main {{ max-width: 82ch; margin: 0 auto; padding: 1.5rem; }}
  pre#answer {{
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.95rem;
  }}
  .markdown > :first-child {{ margin-top: 0; }}
  .markdown h1, .markdown h2, .markdown h3,
  .markdown h4, .markdown h5, .markdown h6 {{
    margin: 1.4em 0 0.5em;
    line-height: 1.25;
  }}
  .markdown h1 {{ font-size: 1.7rem; }}
  .markdown h2 {{ font-size: 1.4rem; }}
  .markdown h3 {{ font-size: 1.2rem; }}
  .markdown h4, .markdown h5, .markdown h6 {{ font-size: 1.05rem; }}
  .markdown p {{ margin: 0 0 1em; }}
  .markdown ul, .markdown ol {{ margin: 0 0 1em; padding-left: 1.6rem; }}
  .markdown li {{ margin: 0.25em 0; }}
  .markdown blockquote {{
    margin: 0 0 1em;
    padding: 0.1em 0 0.1em 1rem;
    border-left: 3px solid color-mix(in srgb, var(--docent-fg) 30%, transparent);
    opacity: 0.85;
  }}
  .markdown hr {{
    margin: 1.5em 0;
    border: 0;
    border-top: 1px solid color-mix(in srgb, var(--docent-fg) 20%, transparent);
  }}
  .markdown code {{
    padding: 0.1em 0.3em;
    border-radius: 3px;
    background: color-mix(in srgb, var(--docent-fg) 12%, transparent);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9em;
  }}
  .markdown pre {{
    margin: 0 0 1em;
    padding: 0.8rem 1rem;
    overflow-x: auto;
    border-radius: 4px;
    background: color-mix(in srgb, var(--docent-fg) 8%, transparent);
  }}
  .markdown pre code {{ padding: 0; background: none; font-size: 0.9rem; }}
  .markdown a {{
    color: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--docent-fg) 45%, transparent);
    text-underline-offset: 2px;
  }}
  .table-wrap {{ margin: 0 0 1em; overflow-x: auto; }}
  .markdown table {{ border-collapse: collapse; }}
  .markdown th, .markdown td {{
    padding: 0.3rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--docent-fg) 20%, transparent);
    vertical-align: top;
  }}
  .markdown th {{
    background: color-mix(in srgb, var(--docent-fg) 8%, transparent);
    font-weight: 600;
  }}
  .error {{ color: var(--docent-error); }}
</style>
</head>
<body>
<header>
  <span id="status">docent: {escaped_profile}</span>
  <span id="hint">d close</span>
</header>
<main><pre id="answer" data-placeholder="1">Thinking...</pre></main>
</body>
</html>
"""


class AnswerPage:
    """Stream text into an answer-tab page."""

    def __init__(self, tab: Any, profile: str) -> None:
        self.tab = tab
        self.profile = profile
        self.started = time.monotonic()
        self.closed = False
        self.finished = False
        self.failed = False
        self._chunks: list[str] = []

    def append_text(self, text: str) -> None:
        """Append a streamed delta to the answer."""
        if not text or self.finished or self.failed or self.closed:
            return
        self._chunks.append(text)
        self._run(
            """
(() => {
  const answer = document.getElementById('answer');
  if (!answer) return;
  if (answer.dataset.placeholder === '1') {
    answer.textContent = '';
    answer.dataset.placeholder = '0';
  }
  const root = document.documentElement;
  const atBottom = (window.innerHeight + window.scrollY) >= (root.scrollHeight - 8);
  answer.appendChild(document.createTextNode(TEXT));
  if (atBottom) window.scrollTo(0, root.scrollHeight);
})()
""".replace("TEXT", _js_string(text))
        )

    def set_error(self, message: str) -> None:
        """Append an error line and mark the request finished."""
        if self.closed or self.failed:
            return
        self.failed = True
        self.finished = True
        self._run(
            """
(() => {
  const answer = document.getElementById('answer');
  if (!answer) return;
  if (answer.dataset.placeholder === '1') {
    answer.textContent = '';
    answer.dataset.placeholder = '0';
  }
  const span = document.createElement('span');
  span.className = 'error';
  span.textContent = MESSAGE;
  answer.appendChild(span);
  const status = document.getElementById('status');
  if (status) status.textContent = 'docent: error';
  document.title = 'docent: error';
})()
""".replace("MESSAGE", _js_string("\n\nerror: " + message))
        )

    def done(self, model: str | None = None) -> None:
        """Render the accumulated answer and set the final title."""
        if self.closed or self.failed:
            return
        self.finished = True
        self._show(self._label(model))

    def finish(self) -> None:
        """Render the answer when the stream ends without a done event."""
        if self.closed or self.finished or self.failed:
            return
        self.finished = True
        self._show(self._label(None))

    def _show(self, label: str) -> None:
        """Replace the raw answer with rendered Markdown."""
        body = markdown.render("".join(self._chunks)) or "<p>(no output)</p>"
        block = f'<div id="answer" class="markdown">{body}</div>'
        self._chunks.clear()
        payload = json.dumps({"block": block, "label": label}, ensure_ascii=False)
        payload = payload.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
        self._run(
            """
(() => {
  const payload = PAYLOAD;
  const answer = document.getElementById('answer');
  if (answer) {
    const root = document.documentElement;
    const atBottom = (window.innerHeight + window.scrollY) >= (root.scrollHeight - 8);
    answer.outerHTML = payload.block;
    if (atBottom) window.scrollTo(0, root.scrollHeight);
  }
  const status = document.getElementById('status');
  if (status) status.textContent = payload.label;
  document.title = payload.label;
})()
""".replace("PAYLOAD", payload)
        )

    def close(self) -> None:
        """Stop accepting updates."""
        self.closed = True

    def _label(self, model: str | None) -> str:
        elapsed = time.monotonic() - self.started
        base = f"docent: {self.profile}"
        if model:
            base = f"{base} - {model}"
        return f"{base} ({elapsed:.1f}s)"

    def _run(self, code: str) -> None:
        if self.closed or getattr(self.tab, "pending_removal", False):
            self.closed = True
            return
        try:
            self.tab.run_js_async(code)
        except Exception:  # noqa: BLE001
            self.closed = True


def _js_string(value: str) -> str:
    """Return a JavaScript string literal for arbitrary text."""
    encoded = json.dumps(value, ensure_ascii=False)
    return encoded.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
