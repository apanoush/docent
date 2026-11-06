"""Extract text from a PDF page window with pdftotext."""

from __future__ import annotations

import shutil
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
    runner: Any = subprocess.run,
) -> str:
    """Return the text of pages ``start`` through ``end`` as UTF-8."""
    if shutil.which("pdftotext") is None:
        raise PDFError("pdftotext not found; install poppler-utils")

    try:
        result = runner(
            ["pdftotext", "-f", str(start), "-l", str(end), str(path), "-"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PDFError(f"pdftotext failed: {exc}") from exc
    if result.returncode != 0:
        detail = (result.stderr or "").strip() or "pdftotext failed"
        raise PDFError(detail)

    text = (result.stdout or "").replace("\r\n", "\n").strip()
    if not text:
        raise PDFError("no text layer on this page; the PDF may be scanned")
    return text
