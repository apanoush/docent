"""Extract text from a PDF page window with the docent CLI."""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

__all__ = ["PDFError", "extract_text", "page_window"]


class PDFError(Exception):
    """PDF text extraction failed."""


def page_window(page: int, radius: int = 1) -> tuple[int, int]:
    """Return the 1-based page range around ``page``, clamped at page 1."""
    if page < 1:
        raise ValueError("page numbers start at 1")
    if radius < 0:
        raise ValueError("radius must not be negative")
    return max(1, page - radius), page + radius


def extract_text(
    path: Path,
    start: int,
    end: int,
    docent_cmd: str = "docent",
    runner: Any = subprocess.run,
) -> str:
    """Return the text of pages ``start`` through ``end`` as UTF-8.

    ``docent extract`` clamps ``end`` to the last page, so a window that runs
    past the final page still returns the pages that exist.
    """
    try:
        result = runner(
            [
                docent_cmd,
                "extract",
                str(path),
                "--pages",
                f"{start}:{end}",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except FileNotFoundError as exc:
        raise PDFError(f"docent not found: {docent_cmd}") from exc
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PDFError(f"docent extract failed: {exc}") from exc
    if result.returncode != 0:
        detail = (result.stderr or "").strip() or "docent extract failed"
        raise PDFError(detail)

    text = (result.stdout or "").replace("\r\n", "\n").strip()
    if not text:
        raise PDFError("no text layer on this page; the PDF may be scanned")
    return text
