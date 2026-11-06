"""Tests for terminal detection and argv building."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import pytest
from docent_zathura.launch import (
    TerminalError,
    build_terminal_argv,
    detect_terminal,
    launch_terminal,
)


def test_builds_foot_arguments() -> None:
    assert build_terminal_argv("foot", ["docent", "run"]) == [
        "foot",
        "-a",
        "docent",
        "-T",
        "Docent",
        "-e",
        "docent",
        "run",
    ]


def test_builds_kitty_arguments() -> None:
    assert build_terminal_argv("kitty", ["docent"]) == [
        "kitty",
        "--class",
        "docent",
        "--title",
        "Docent",
        "-e",
        "docent",
    ]


def test_unknown_terminal_uses_e() -> None:
    assert build_terminal_argv("/usr/bin/myterm", ["docent"]) == [
        "/usr/bin/myterm",
        "-e",
        "docent",
    ]


def test_detect_prefers_docent_terminal(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("DOCENT_TERMINAL", "kitty")
    monkeypatch.setenv("TERMINAL", "foot")

    assert detect_terminal() == "kitty"


def test_detect_uses_terminal(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("DOCENT_TERMINAL", raising=False)
    monkeypatch.setenv("TERMINAL", "alacritty")

    assert detect_terminal() == "alacritty"


def test_detect_falls_back_to_path(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("DOCENT_TERMINAL", raising=False)
    monkeypatch.delenv("TERMINAL", raising=False)
    monkeypatch.setattr(
        "docent_zathura.launch.shutil.which",
        lambda name: "/usr/bin/foot" if name == "foot" else None,
    )

    assert detect_terminal() == "foot"


def test_detect_returns_none_when_nothing_is_available(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("DOCENT_TERMINAL", raising=False)
    monkeypatch.delenv("TERMINAL", raising=False)
    monkeypatch.setattr("docent_zathura.launch.shutil.which", lambda name: None)

    assert detect_terminal() is None


def test_launch_terminal_starts_a_new_session() -> None:
    captured: dict[str, Any] = {}

    def runner(argv: Sequence[str], **kwargs: Any) -> object:
        captured["argv"] = argv
        captured["kwargs"] = kwargs
        return object()

    launch_terminal("foot", ["docent"], runner)

    assert captured["argv"][0] == "foot"
    assert captured["kwargs"]["start_new_session"] is True


def test_launch_terminal_reports_start_failure() -> None:
    def runner(argv: Sequence[str], **kwargs: Any) -> object:
        raise FileNotFoundError("missing terminal")

    with pytest.raises(TerminalError, match="missing terminal"):
        launch_terminal("foot", ["docent"], runner)
