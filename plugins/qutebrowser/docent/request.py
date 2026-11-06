"""Build the JSON request docent reads on stdin."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

__all__ = ["Capture", "build_request"]


@dataclass(frozen=True, slots=True)
class Capture:
    """Text captured from the current qutebrowser tab."""

    selection: str
    document: str
    file: str = ""
    byte_start: int | None = None
    byte_end: int | None = None


def build_request(capture: Capture, *, question: str | None = None) -> dict[str, Any]:
    """Return a request object containing only fields the CLI knows about.

    Offsets are included only when they match ``selection`` exactly. This
    mirrors the CLI's own validation in ``src/request.rs`` and keeps a stale
    capture from producing misleading selection markers.
    """
    request: dict[str, Any] = {
        "selection": capture.selection,
        "document": capture.document,
    }
    if capture.file:
        request["file"] = capture.file

    offsets = _valid_offsets(capture)
    if offsets is not None:
        request["byte_start"], request["byte_end"] = offsets

    if question is not None and question.strip():
        request["question"] = question

    return request


def _valid_offsets(capture: Capture) -> tuple[int, int] | None:
    """Return validated UTF-8 byte offsets, or None when they do not match."""
    if capture.byte_start is None or capture.byte_end is None:
        return None

    start = capture.byte_start
    end = capture.byte_end
    if not 0 <= start < end:
        return None

    try:
        data = capture.document.encode("utf-8")
    except UnicodeEncodeError:
        return None
    if end > len(data):
        return None

    try:
        selected = data[start:end].decode("utf-8")
    except UnicodeDecodeError:
        return None

    if selected != capture.selection:
        return None
    return start, end
