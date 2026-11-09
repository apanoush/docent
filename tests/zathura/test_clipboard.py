"""Tests for clipboard backend selection and reading."""

from __future__ import annotations

import subprocess
from collections.abc import Sequence
from typing import Any

import pytest
from docent_zathura.clipboard import (
    ClipboardError,
    clipboard_commands,
    matches_document,
    read_best_selection,
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


def test_matches_document_exactly_and_after_normalizing_whitespace() -> None:
    assert matches_document("alpha beta", "Page one alpha beta gamma.")
    assert matches_document("alpha\nbeta", "Page one alpha beta gamma.")
    assert not matches_document("missing", "Page one alpha beta gamma.")
    assert not matches_document("", "Page one alpha beta gamma.")


def test_matches_document_joins_hyphenated_line_breaks() -> None:
    assert matches_document("enhanc-\ning academic", "enhancing academic")
    assert matches_document("state-of-the-art", "state-of-the-art")


def _two_clipboard_runner(
    primary: str | None,
    clipboard: str | None,
    calls: list[Sequence[str]],
) -> Any:
    def runner(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        if "--primary" in argv:
            if primary is None:
                return _completed(argv, 1, "", "Nothing is copied")
            return _completed(argv, 0, primary)
        if clipboard is None:
            return _completed(argv, 1, "", "Nothing is copied")
        return _completed(argv, 0, clipboard)

    return runner


def test_best_selection_uses_matching_primary() -> None:
    calls: list[Sequence[str]] = []

    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner("alpha beta", "other", calls),
    )

    assert selection == "alpha beta"
    assert len(calls) == 2
    assert "--primary" in calls[0]
    assert "--primary" not in calls[1]


def test_best_selection_falls_back_to_clipboard_when_primary_is_empty() -> None:
    calls: list[Sequence[str]] = []

    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner(None, "alpha beta", calls),
    )

    assert selection == "alpha beta"
    assert len(calls) == 2


def test_best_selection_uses_matching_clipboard_when_primary_does_not_match() -> None:
    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner("stale", "alpha beta", []),
    )

    assert selection == "alpha beta"


def test_best_selection_prefers_the_longer_match() -> None:
    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner("alpha", "alpha beta", []),
    )

    assert selection == "alpha beta"


def test_best_selection_ignores_a_short_stale_primary_match() -> None:
    document = "DeepSeek-LLM-Chat and English mathematical datasets with tool-integrated solutions"
    selection = read_best_selection(
        document,
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner(
            "LLM", "English mathematical datasets with tool-integrated solutions", []
        ),
    )

    assert selection == "English mathematical datasets with tool-integrated solutions"


def test_best_selection_falls_back_to_primary_when_nothing_matches() -> None:
    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner("stale", "other", []),
    )

    assert selection == "stale"


def test_best_selection_falls_back_to_the_longer_candidate_when_nothing_matches() -> (
    None
):
    selection = read_best_selection(
        "alpha beta",
        environ={"WAYLAND_DISPLAY": "wayland-1"},
        runner=_two_clipboard_runner("stale", "a longer unrelated text", []),
    )

    assert selection == "a longer unrelated text"


def test_best_selection_rejects_unrelated_clipboard_text() -> None:
    with pytest.raises(ClipboardError, match="does not appear"):
        read_best_selection(
            "alpha beta",
            environ={"WAYLAND_DISPLAY": "wayland-1"},
            runner=_two_clipboard_runner(None, "unrelated", []),
        )


def test_best_selection_reports_when_both_are_empty() -> None:
    with pytest.raises(ClipboardError, match="no text selected"):
        read_best_selection(
            "alpha beta",
            environ={"WAYLAND_DISPLAY": "wayland-1"},
            runner=_two_clipboard_runner(None, None, []),
        )


def test_best_selection_honors_an_explicit_target() -> None:
    calls: list[Sequence[str]] = []

    selection = read_best_selection(
        "alpha beta",
        "clipboard",
        {"WAYLAND_DISPLAY": "wayland-1"},
        _two_clipboard_runner("alpha", "alpha beta", calls),
    )

    assert selection == "alpha beta"
    assert len(calls) == 1
    assert "--primary" not in calls[0]
