"""Tests for the adapter command line and request assembly."""

from __future__ import annotations

import io
import json
import shutil
import subprocess
from pathlib import Path
from typing import Any

import pytest
from docent_zathura import cli
from docent_zathura.clipboard import ClipboardError
from docent_zathura.render import RenderError


@pytest.fixture
def pdf(tmp_path: Path) -> Path:
    path = tmp_path / "paper.pdf"
    path.write_bytes(b"%PDF-1.4\n")
    return path


@pytest.fixture
def capture(monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    captured: dict[str, Any] = {}

    def fake_launch(
        terminal: str,
        command: list[str],
        env: dict[str, str] | None = None,
    ) -> None:
        captured["terminal"] = terminal
        captured["command"] = command
        captured["env"] = env

    monkeypatch.setattr(cli, "launch_terminal", fake_launch)
    monkeypatch.setattr(cli, "read_best_selection", lambda document, target: "alpha")
    monkeypatch.setattr(cli, "extract_text", lambda path, start, end: "alpha beta")
    monkeypatch.setattr(cli, "detect_terminal", lambda: "foot")
    return captured


def _payload(captured: dict[str, Any]) -> dict[str, Any]:
    request_file = Path(captured["command"][-1])
    try:
        return json.loads(request_file.read_text(encoding="utf-8"))
    finally:
        shutil.rmtree(request_file.parent, ignore_errors=True)


def test_parser_accepts_explain_options() -> None:
    args = cli.build_parser().parse_args(
        ["explain", "paper.pdf", "3", "--clipboard", "--context-pages", "2"]
    )

    assert args.command == "explain"
    assert args.page == 3
    assert args.clipboard is True
    assert args.context_pages == 2


def test_parser_accepts_primary_override() -> None:
    args = cli.build_parser().parse_args(["explain", "paper.pdf", "1", "--primary"])

    assert args.primary is True
    assert args.clipboard is False


def test_parser_accepts_nvim_viewer() -> None:
    args = cli.build_parser().parse_args(
        ["explain", "paper.pdf", "1", "--viewer", "nvim", "--nvim", "/usr/bin/nvim"]
    )

    assert args.viewer == "nvim"
    assert args.nvim == "/usr/bin/nvim"


def test_parser_rejects_two_clipboard_targets() -> None:
    with pytest.raises(SystemExit):
        cli.build_parser().parse_args(
            ["explain", "paper.pdf", "1", "--primary", "--clipboard"]
        )


def test_capture_builds_explain_request(pdf: Path, capture: dict[str, Any]) -> None:
    code = cli.main(
        ["explain", str(pdf), "2", "--terminal", "foot", "--docent", "/bin/docent"]
    )

    assert code == 0
    assert capture["terminal"] == "foot"
    payload = _payload(capture)
    assert payload["profile"] == "explain"
    assert payload["request"] == {
        "selection": "alpha",
        "document": "alpha beta",
        "file": "paper.pdf (page 2)",
        "byte_start": 0,
        "byte_end": 5,
    }
    assert payload["prompt_for_question"] is False
    assert payload["context_mode"] is None
    assert payload["target_language"] is None


def test_capture_prompts_for_ask_question(pdf: Path, capture: dict[str, Any]) -> None:
    code = cli.main(["ask", str(pdf), "1", "--terminal", "foot"])

    assert code == 0
    payload = _payload(capture)
    assert payload["profile"] == "ask"
    assert payload["prompt_for_question"] is True


def test_capture_passes_translate_language(pdf: Path, capture: dict[str, Any]) -> None:
    code = cli.main(
        ["translate", str(pdf), "1", "--terminal", "foot", "--to", "French"]
    )

    assert code == 0
    payload = _payload(capture)
    assert payload["profile"] == "translate"
    assert payload["target_language"] == "French"


def test_capture_runs_a_custom_profile(pdf: Path, capture: dict[str, Any]) -> None:
    code = cli.main(["do", "custom", str(pdf), "1", "--terminal", "foot"])

    assert code == 0
    payload = _payload(capture)
    assert payload["profile"] == "custom"


def test_capture_launches_nvim_viewer(
    pdf: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    captured: dict[str, Any] = {}

    def fake_launch(
        terminal: str,
        command: list[str],
        env: dict[str, str] | None = None,
    ) -> None:
        captured["terminal"] = terminal
        captured["command"] = command
        captured["env"] = env

    monkeypatch.setattr(cli, "launch_terminal", fake_launch)
    monkeypatch.setattr(cli, "read_best_selection", lambda document, target: "alpha")
    monkeypatch.setattr(cli, "extract_text", lambda path, start, end: "alpha beta")
    monkeypatch.setattr(cli, "detect_terminal", lambda: "foot")
    monkeypatch.setattr(cli, "find_docent", lambda value: "/home/me/.local/bin/docent")
    monkeypatch.setattr(cli, "_resolve_executable", lambda value: "/usr/bin/nvim")

    code = cli.main(
        ["explain", str(pdf), "1", "--terminal", "foot", "--viewer", "nvim"]
    )

    assert code == 0
    assert captured["terminal"] == "foot"
    assert captured["command"][0] == "/usr/bin/nvim"
    assert "--cmd" in captured["command"]
    assert captured["env"]["DOCENT_ZATHURA_NVIM"].endswith("plugins/nvim")
    assert captured["env"]["DOCENT_ZATHURA_VIEWER"].endswith("viewer.lua")
    request_file = Path(captured["env"]["DOCENT_ZATHURA_REQUEST"])
    try:
        payload = json.loads(request_file.read_text(encoding="utf-8"))
        assert payload["docent"] == "/home/me/.local/bin/docent"
        assert payload["request"]["selection"] == "alpha"
    finally:
        shutil.rmtree(request_file.parent, ignore_errors=True)


def test_capture_requires_nvim(pdf: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    messages: list[str] = []

    monkeypatch.setattr(cli, "read_best_selection", lambda document, target: "alpha")
    monkeypatch.setattr(cli, "extract_text", lambda path, start, end: "alpha beta")
    monkeypatch.setattr(cli, "detect_terminal", lambda: "foot")
    monkeypatch.setattr(cli, "_resolve_executable", lambda value: None)
    monkeypatch.setattr(cli, "_show_error", messages.append)

    code = cli.main(["explain", str(pdf), "1", "--viewer", "nvim"])

    assert code == 1
    assert "nvim not found" in messages[0]


def test_capture_reports_clipboard_errors(
    pdf: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    messages: list[str] = []

    def fail(document: str, target: str | None) -> str:
        raise ClipboardError("nothing selected")

    monkeypatch.setattr(cli, "read_best_selection", fail)
    monkeypatch.setattr(cli, "extract_text", lambda path, start, end: "alpha beta")
    monkeypatch.setattr(cli, "_show_error", messages.append)

    code = cli.main(["explain", str(pdf), "1", "--terminal", "foot"])

    assert code == 1
    assert messages == ["nothing selected"]


def test_capture_requires_a_terminal(
    pdf: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    messages: list[str] = []

    monkeypatch.setattr(cli, "read_best_selection", lambda document, target: "alpha")
    monkeypatch.setattr(cli, "extract_text", lambda path, start, end: "alpha")
    monkeypatch.setattr(cli, "detect_terminal", lambda: None)
    monkeypatch.setattr(cli, "_show_error", messages.append)

    code = cli.main(["explain", str(pdf), "1"])

    assert code == 1
    assert "no terminal emulator" in messages[0]


def test_capture_rejects_a_missing_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    messages: list[str] = []
    monkeypatch.setattr(cli, "_show_error", messages.append)

    code = cli.main(["explain", str(tmp_path / "missing.pdf"), "1"])

    assert code == 1
    assert "not a file" in messages[0]


def test_render_command_dispatches(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(cli, "run_render", lambda *args, **kwargs: 7)

    assert cli.main(["render", "request.json"]) == 7


def test_render_command_reports_errors(monkeypatch: pytest.MonkeyPatch) -> None:
    def fail(*args: object, **kwargs: object) -> int:
        raise RenderError("bad stream")

    monkeypatch.setattr(cli, "run_render", fail)

    assert cli.main(["render", "request.json"]) == 1


def test_find_docent_uses_path(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(cli.shutil, "which", lambda name: "/opt/bin/docent")

    assert cli.find_docent("docent") == "/opt/bin/docent"


def test_find_docent_searches_home_install_locations(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(cli.shutil, "which", lambda name: None)
    candidate = tmp_path / ".local" / "bin" / "docent"
    candidate.parent.mkdir(parents=True)
    candidate.write_text("#!/bin/sh\n", encoding="utf-8")
    candidate.chmod(0o755)

    assert cli.find_docent("docent") == str(candidate)


def test_find_docent_keeps_an_absolute_path(tmp_path: Path) -> None:
    candidate = tmp_path / "custom-docent"

    assert cli.find_docent(str(candidate)) == str(candidate)


class _NotTty(io.StringIO):
    def isatty(self) -> bool:
        return False


def test_show_error_prefers_zenity(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[list[str]] = []
    monkeypatch.setattr(cli.sys, "stderr", _NotTty())
    monkeypatch.setenv("WAYLAND_DISPLAY", "wayland-1")
    monkeypatch.setattr(
        cli.shutil,
        "which",
        lambda name: f"/usr/bin/{name}" if name == "zenity" else None,
    )

    def runner(argv: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0)

    monkeypatch.setattr(cli.subprocess, "run", runner)

    cli._show_error("boom")

    assert calls[0][0] == "zenity"


def test_show_error_falls_back_to_notify_send(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[list[str]] = []
    monkeypatch.setattr(cli.sys, "stderr", _NotTty())
    monkeypatch.setenv("WAYLAND_DISPLAY", "wayland-1")
    monkeypatch.setattr(
        cli.shutil,
        "which",
        lambda name: "/usr/bin/notify-send" if name == "notify-send" else None,
    )

    def runner(argv: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0)

    monkeypatch.setattr(cli.subprocess, "run", runner)

    cli._show_error("boom")

    assert calls[0][0] == "notify-send"


def test_show_error_skips_dialogs_on_a_terminal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[object] = []

    class Tty(io.StringIO):
        def isatty(self) -> bool:
            return True

    monkeypatch.setattr(cli.sys, "stderr", Tty())
    monkeypatch.setattr(
        cli.subprocess, "run", lambda *args, **kwargs: calls.append(args)
    )

    cli._show_error("boom")

    assert calls == []
