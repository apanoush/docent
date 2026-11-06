"""Tests for clipboard backend selection and reading."""

from __future__ import annotations

import subprocess
from collections.abc import Sequence
from typing import Any

import pytest
from docent_zathura.clipboard import (
    ClipboardError,
    clipboard_commands,
    read_selection,
)


def _completed(
    argv: Sequence[str], code: int, stdout: str = "", stderr: str = ""
) -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(argv, code, stdout, stderr)


def test_wayland_primary_prefers_wl_paste() -> None:
    commands = clipboard_commands("primary", {"WAYLAND_DISPLAY": "wayland-1"})
    assert commands[0].name == "wl-paste"
    assert commands[0].argv == ("wl-paste", "--no-newline", "--primary")


def test_wayland_clipboard_omits_primary() -> None:
    commands = clipboard_commands("clipboard", {"WAYLAND_DISPLAY": "wayland-1"})
    assert commands[0].argv == ("wl-paste", "--no-newline")


def test_x11_uses_xclip_then_xsel(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        "docent_zathura.clipboard.shutil.which",
        lambda name: f"/usr/bin/{name}",
    )

    commands = clipboard_commands("primary", {"DISPLAY": ":0"})

    assert [command.name for command in commands] == ["xclip", "xsel"]
    assert commands[0].argv == ("xclip", "-selection", "primary", "-o")
    assert commands[1].argv == ("xsel", "--primary", "--output")


def test_rejects_unknown_target() -> None:
    with pytest.raises(ValueError, match="target"):
        clipboard_commands("selection", {})


def test_no_tool_available_raises() -> None:
    with pytest.raises(ClipboardError, match="no clipboard tool"):
        read_selection("primary", {})


def test_reads_text_and_strips_trailing_newline() -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, "hello\n")

    assert (
        read_selection("primary", {"WAYLAND_DISPLAY": "wayland-1"}, runner) == "hello"
    )


def test_falls_back_when_first_tool_is_empty(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "docent_zathura.clipboard.shutil.which",
        lambda name: f"/usr/bin/{name}",
    )
    calls: list[Sequence[str]] = []

    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        if argv[0] == "wl-paste":
            return _completed(argv, 0, "")
        return _completed(argv, 0, "fallback")

    text = read_selection(
        "primary",
        {"WAYLAND_DISPLAY": "wayland-1", "DISPLAY": ":0"},
        runner,
    )

    assert text == "fallback"
    assert [call[0] for call in calls] == ["wl-paste", "xclip"]


def test_empty_selection_is_reported() -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 0, "\n")

    with pytest.raises(ClipboardError, match="no text selected"):
        read_selection("primary", {"WAYLAND_DISPLAY": "wayland-1"}, runner)


def test_tool_failure_is_reported() -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return _completed(argv, 1, "", "clipboard unavailable")

    with pytest.raises(ClipboardError, match="clipboard unavailable"):
        read_selection("primary", {"WAYLAND_DISPLAY": "wayland-1"}, runner)


def test_missing_tool_is_reported() -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        raise FileNotFoundError("wl-paste")

    with pytest.raises(ClipboardError, match="wl-paste"):
        read_selection("primary", {"WAYLAND_DISPLAY": "wayland-1"}, runner)
