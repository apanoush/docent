"""Answer-tab shell and streaming DOM updates for the qutebrowser adapter."""

from __future__ import annotations

import html
import json
import re
import time
from functools import cache
from pathlib import Path
from typing import Any

from . import markdown

__all__ = ["AnswerPage", "build_shell", "stylesheet"]

_SAFE_COLOR = re.compile(r"^[#a-zA-Z0-9(),.% -]+$")
_STYLESHEET = Path(__file__).with_name("answer.css")


@cache
def stylesheet() -> str:
    """Return the default answer-tab stylesheet."""
    return _STYLESHEET.read_text(encoding="utf-8")


def build_shell(
    profile: str,
    colors: dict[str, str] | None = None,
    custom_css: str | None = None,
    *,
    label: str | None = None,
    hint: str | None = "d close",
    script: str | None = None,
) -> str:
    """Return the small HTML page used to receive streamed text."""
    style = stylesheet()
    overrides = _color_overrides(colors)
    if overrides:
        style = f"{style}\n:root {{ {overrides} }}"
    if custom_css:
        style = f"{style}\n{custom_css}"

    escaped_profile = html.escape(profile, quote=True)
    escaped_label = (
        html.escape(label, quote=True) if label else f"docent: {escaped_profile}"
    )
    hint_html = (
        f'\n  <span id="hint">{html.escape(hint, quote=True)}</span>' if hint else ""
    )
    script_html = f"\n<script>\n{script}\n</script>" if script else ""
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{escaped_label}</title>
<style>
{style}
</style>
</head>
<body>
<header>
  <span id="status">{escaped_label}</span>{hint_html}
</header>
<main><pre id="answer" data-placeholder="1">Thinking...</pre></main>{script_html}
</body>
</html>
"""


def _color_overrides(colors: dict[str, str] | None) -> str:
    """Return valid CSS custom-property declarations for the palette."""
    if not colors:
        return ""
    declarations: list[str] = []
    for name in ("bg", "fg", "error"):
        value = colors.get(name)
        if isinstance(value, str) and _SAFE_COLOR.fullmatch(value.strip()):
            declarations.append(f"--docent-{name}: {value.strip()};")
    return " ".join(declarations)


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
