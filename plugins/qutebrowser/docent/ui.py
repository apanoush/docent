"""Answer-tab shell and streaming DOM updates for the qutebrowser adapter."""

from __future__ import annotations

import html
import json
import re
import time
from typing import Any

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

    def append_text(self, text: str) -> None:
        """Append a streamed delta to the answer."""
        if not text or self.finished or self.failed or self.closed:
            return
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
        """Set the final title after a successful stream."""
        if self.closed or self.failed:
            return
        self.finished = True
        label = self._label(model)
        self._run(
            """
(() => {
  const answer = document.getElementById('answer');
  if (answer && answer.dataset.placeholder === '1') {
    answer.textContent = '(no output)';
    answer.dataset.placeholder = '0';
  }
  const status = document.getElementById('status');
  if (status) status.textContent = LABEL;
  document.title = LABEL;
})()
""".replace("LABEL", _js_string(label))
        )

    def finish(self) -> None:
        """Finalize a stream that ended without a done event."""
        if self.closed or self.finished or self.failed:
            return
        self.finished = True
        label = self._label(None)
        self._run(
            """
(() => {
  const answer = document.getElementById('answer');
  if (answer && (answer.dataset.placeholder === '1' || answer.textContent === '')) {
    answer.textContent = '(no output)';
    answer.dataset.placeholder = '0';
  }
  const status = document.getElementById('status');
  if (status) status.textContent = LABEL;
  document.title = LABEL;
})()
""".replace("LABEL", _js_string(label))
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
