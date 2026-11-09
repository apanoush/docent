"""Tests for PDF page windows and text extraction."""

from __future__ import annotations

import subprocess
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import pytest
from docent_zathura.pdf import PDFError, extract_text, page_window


def _completed(
    argv: Sequence[str], code: int, stdout: str = "", stderr: str = ""
) -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(argv, code, stdout, stderr)


def test_page_window_clamps_at_first_page() -> None:
    assert page_window(1) == (1, 2)


def test_page_window_uses_radius() -> None:
    assert page_window(5) == (4, 6)
    assert page_window(5, 2) == (3, 7)


def test_page_window_rejects_invalid_values() -> None:
    with pytest.raises(ValueError, match="page numbers"):
        page_window(0)
    with pytest.raises(ValueError, match="radius"):
        page_window(1, -1)


def test_extract_text_builds_docent_extract_command(tmp_path: Path) -> None:
    calls: list[Sequence[str]] = []

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        return _completed(argv, 0, "Page text\n")

    path = tmp_path / "paper.pdf"
    text = extract_text(path, 2, 4, "/usr/bin/docent", runner)

    assert text == "Page text"
    assert calls == [
        [
            "/usr/bin/docent",
            "extract",
            str(path),
            "--pages",
            "2:4",
        ],
    ]


def test_extract_text_normalizes_crlf(tmp_path: Path) -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, "a\r\nb\r\n")

    assert extract_text(tmp_path / "paper.pdf", 1, 1, "docent", runner) == "a\nb"


def test_extract_text_reports_missing_docent(tmp_path: Path) -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        raise FileNotFoundError("docent")

    with pytest.raises(PDFError, match="docent not found"):
        extract_text(tmp_path / "paper.pdf", 1, 2, "docent", runner)


def test_extract_text_reports_docent_failure(tmp_path: Path) -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 4, "", "page range 2:4 is out of bounds")

    with pytest.raises(PDFError, match="out of bounds"):
        extract_text(tmp_path / "paper.pdf", 2, 4, "docent", runner)


def test_extract_text_reports_timeout(tmp_path: Path) -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        raise subprocess.TimeoutExpired(argv, 30)

    with pytest.raises(PDFError, match="docent extract failed"):
        extract_text(tmp_path / "paper.pdf", 1, 2, "docent", runner)


def test_extract_text_rejects_empty_text(tmp_path: Path) -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, " \n")

    with pytest.raises(PDFError, match="scanned"):
        extract_text(tmp_path / "paper.pdf", 1, 2, "docent", runner)
