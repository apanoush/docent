"""Minimal Markdown rendering for the qutebrowser answer tab.

Only the constructs that show up in model answers are supported. The renderer
emits a small, fixed set of HTML tags and escapes all text, so model output
cannot inject markup, scripts, or remote resources.
"""

from __future__ import annotations

import html
import re
from urllib.parse import urlsplit

__all__ = ["render"]

_HEADING = re.compile(r"^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
_RULE = re.compile(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$")
_FENCE = re.compile(r"^ {0,3}```\s*([A-Za-z0-9_+-]*)\s*$")
_UNORDERED = re.compile(r"^ {0,3}[-*+]\s+(.*)$")
_ORDERED = re.compile(r"^ {0,3}\d{1,9}[.)]\s+(.*)$")
_QUOTE = re.compile(r"^ {0,3}>\s?(.*)$")
_SEPARATOR_CELL = re.compile(r"^:?-{3,}:?$")

_INLINE = re.compile(
    r"`(?P<code>[^`\n]+)`"
    r"|!\[(?P<image>[^\]\n]*)\]\((?P<image_url>[^)\s]*)\)"
    r"|\*\*(?P<strong>.+?)\*\*"
    r"|~~(?P<strike>.+?)~~"
    r"|(?<!\*)\*(?P<em>[^*\n]+)\*(?!\*)"
    r"|\[(?P<label>[^\]\n]+)\]\((?P<url>[^)\s]+)\)"
)

_SAFE_SCHEMES = frozenset({"http", "https", "mailto"})


def render(text: str) -> str:
    """Convert a Markdown subset to a safe HTML fragment."""
    if not text.strip():
        return ""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    return "\n".join(_blocks(lines))


def _blocks(lines: list[str]) -> list[str]:
    out: list[str] = []
    paragraph: list[str] = []

    def flush() -> None:
        if paragraph:
            text = " ".join(paragraph)
            out.append(f"<p>{_inline(text)}</p>")
            paragraph.clear()

    index = 0
    while index < len(lines):
        line = lines[index]

        if not line.strip():
            flush()
            index += 1
            continue

        fence = _FENCE.match(line)
        if fence:
            flush()
            language = fence.group(1)
            index += 1
            code: list[str] = []
            while index < len(lines) and not _FENCE.match(lines[index]):
                code.append(lines[index])
                index += 1
            index += 1
            body = html.escape("\n".join(code), quote=False)
            attribute = f' class="language-{language}"' if language else ""
            out.append(f"<pre><code{attribute}>{body}</code></pre>")
            continue

        heading = _HEADING.match(line)
        if heading:
            flush()
            level = len(heading.group(1))
            out.append(f"<h{level}>{_inline(heading.group(2))}</h{level}>")
            index += 1
            continue

        if _RULE.match(line):
            flush()
            out.append("<hr>")
            index += 1
            continue

        if _QUOTE.match(line):
            flush()
            quoted: list[str] = []
            while index < len(lines):
                nested = _QUOTE.match(lines[index])
                if not nested:
                    break
                quoted.append(nested.group(1))
                index += 1
            inner = render("\n".join(quoted))
            out.append(f"<blockquote>{inner}</blockquote>")
            continue

        unordered = _UNORDERED.match(line)
        ordered = _ORDERED.match(line)
        if unordered or ordered:
            flush()
            pattern = _UNORDERED if unordered else _ORDERED
            tag = "ul" if unordered else "ol"
            items: list[str] = []
            while index < len(lines):
                item = pattern.match(lines[index])
                if not item:
                    break
                items.append(f"<li>{_inline(item.group(1))}</li>")
                index += 1
            out.append(f"<{tag}>{''.join(items)}</{tag}>")
            continue

        table = _table(lines, index)
        if table is not None:
            flush()
            rendered, index = table
            out.append(rendered)
            continue

        paragraph.append(line.strip())
        index += 1

    flush()
    return out


def _table(lines: list[str], start: int) -> tuple[str, int] | None:
    """Render a GFM pipe table starting at ``start``, if one is present."""
    if "|" not in lines[start] or start + 1 >= len(lines):
        return None
    alignments = _separator_alignments(lines[start + 1])
    if alignments is None:
        return None
    header = _split_row(lines[start])
    if len(header) < 2 or len(header) != len(alignments):
        return None

    rows: list[str] = []
    index = start + 2
    while index < len(lines) and lines[index].strip() and "|" in lines[index]:
        cells = _split_row(lines[index])
        cells = (cells + [""] * len(header))[: len(header)]
        rendered = "".join(
            _cell("td", cell, align) for cell, align in zip(cells, alignments)
        )
        rows.append(f"<tr>{rendered}</tr>")
        index += 1

    head = "".join(_cell("th", cell, align) for cell, align in zip(header, alignments))
    return (
        (
            '<div class="table-wrap"><table>'
            f"<thead><tr>{head}</tr></thead>"
            f"<tbody>{''.join(rows)}</tbody>"
            "</table></div>"
        ),
        index,
    )


def _separator_alignments(line: str) -> list[str] | None:
    """Return per-column alignments when ``line`` is a table separator."""
    if "|" not in line:
        return None
    cells = _split_row(line)
    if len(cells) < 2:
        return None
    alignments: list[str] = []
    for cell in cells:
        if not _SEPARATOR_CELL.fullmatch(cell):
            return None
        if cell.startswith(":") and cell.endswith(":"):
            alignments.append("center")
        elif cell.endswith(":"):
            alignments.append("right")
        elif cell.startswith(":"):
            alignments.append("left")
        else:
            alignments.append("")
    return alignments


def _split_row(line: str) -> list[str]:
    """Split one table row, dropping optional outer pipes."""
    text = line.strip()
    text = text.removeprefix("|")
    if text.endswith("|") and not text.endswith("\\|"):
        text = text[:-1]
    return [cell.strip().replace("\\|", "|") for cell in re.split(r"(?<!\\)\|", text)]


def _cell(tag: str, text: str, align: str) -> str:
    style = f' style="text-align: {align}"' if align else ""
    return f"<{tag}{style}>{_inline(text)}</{tag}>"


def _inline(text: str) -> str:
    """Escape text and render inline Markdown constructs."""
    out: list[str] = []
    position = 0
    for match in _INLINE.finditer(text):
        out.append(html.escape(text[position : match.start()], quote=False))
        if match.group("code") is not None:
            code = html.escape(match.group("code"), quote=False)
            out.append(f"<code>{code}</code>")
        elif match.group("image") is not None:
            out.append(html.escape(match.group("image"), quote=False))
        elif match.group("strong") is not None:
            out.append(f"<strong>{_inline(match.group('strong'))}</strong>")
        elif match.group("strike") is not None:
            out.append(f"<del>{_inline(match.group('strike'))}</del>")
        elif match.group("em") is not None:
            out.append(f"<em>{_inline(match.group('em'))}</em>")
        else:
            label = _inline(match.group("label"))
            out.append(_link(label, match.group("url")))
        position = match.end()
    out.append(html.escape(text[position:], quote=False))
    return "".join(out)


def _link(label: str, url: str) -> str:
    """Render a link, falling back to text for unsupported schemes."""
    try:
        parts = urlsplit(url)
    except ValueError:
        return _link_text(label, url)
    scheme = parts.scheme.lower()
    if scheme not in _SAFE_SCHEMES or (scheme != "mailto" and not parts.netloc):
        return _link_text(label, url)
    href = html.escape(url, quote=True)
    return f'<a href="{href}" target="_blank" rel="noopener noreferrer">{label}</a>'


def _link_text(label: str, url: str) -> str:
    return f"{label} ({html.escape(url, quote=False)})"
