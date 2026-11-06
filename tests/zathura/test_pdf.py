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


def test_extract_text_builds_pdftotext_command(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )
    calls: list[Sequence[str]] = []

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        return _completed(argv, 0, "Page text\n")

    path = tmp_path / "paper.pdf"
    text = extract_text(path, 2, 4, runner)

    assert text == "Page text"
    assert calls == [
        ["pdftotext", "-f", "2", "-l", "4", str(path), "-"],
    ]


def test_extract_text_normalizes_crlf(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, "a\r\nb\r\n")

    assert extract_text(tmp_path / "paper.pdf", 1, 1, runner) == "a\nb"


def test_extract_text_requires_pdftotext(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr("docent_zathura.pdf.shutil.which", lambda name: None)

    with pytest.raises(PDFError, match="poppler-utils"):
        extract_text(tmp_path / "paper.pdf", 1, 2)


def test_extract_text_reports_pdftotext_failure(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 1, "", "bad pdf")

    with pytest.raises(PDFError, match="bad pdf"):
        extract_text(tmp_path / "paper.pdf", 1, 2, runner)


def test_extract_text_reports_timeout(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        raise subprocess.TimeoutExpired(argv, 30)

    with pytest.raises(PDFError, match="pdftotext failed"):
        extract_text(tmp_path / "paper.pdf", 1, 2, runner)


def test_extract_text_reports_missing_binary(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        raise FileNotFoundError("pdftotext")

    with pytest.raises(PDFError, match="pdftotext failed"):
        extract_text(tmp_path / "paper.pdf", 1, 2, runner)


def test_extract_text_rejects_empty_text(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        "docent_zathura.pdf.shutil.which",
        lambda name: "/usr/bin/pdftotext",
    )

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, " \n")

    with pytest.raises(PDFError, match="scanned"):
        extract_text(tmp_path / "paper.pdf", 1, 2, runner)
