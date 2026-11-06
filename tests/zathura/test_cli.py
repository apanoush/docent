"""Tests for the adapter command line and request assembly."""

from __future__ import annotations

import json
import shutil
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

    def fake_launch(terminal: str, command: list[str]) -> None:
        captured["terminal"] = terminal
        captured["command"] = command

    monkeypatch.setattr(cli, "launch_terminal", fake_launch)
    monkeypatch.setattr(cli, "read_selection", lambda target: "alpha")
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


def test_capture_reports_clipboard_errors(
    pdf: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    messages: list[str] = []

    def fail(target: str) -> str:
        raise ClipboardError("nothing selected")

    monkeypatch.setattr(cli, "read_selection", fail)
    monkeypatch.setattr(cli, "_show_error", messages.append)

    code = cli.main(["explain", str(pdf), "1", "--terminal", "foot"])

    assert code == 1
    assert messages == ["nothing selected"]


def test_capture_requires_a_terminal(
    pdf: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    messages: list[str] = []

    monkeypatch.setattr(cli, "read_selection", lambda target: "alpha")
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
