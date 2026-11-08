"""Tests for CLI stderr parsing."""

from __future__ import annotations

from docent_zathura.logs import warning_lines


def test_warning_lines_strips_log_prefixes() -> None:
    text = (
        "[WARN  docent::cli] context truncated\n"
        "[INFO  docent::cli] not a warning\n"
        "[WARN  docent::config] another warning\n"
    )

    assert warning_lines(text) == ["context truncated", "another warning"]


def test_warning_lines_ignores_other_output() -> None:
    assert warning_lines("") == []
    assert warning_lines("plain text\n") == []
    assert warning_lines("[WARN]\n") == []
