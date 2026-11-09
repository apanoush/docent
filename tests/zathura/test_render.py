"""Tests for NDJSON rendering and request-file cleanup."""

from __future__ import annotations

import io
import json
from pathlib import Path

import pytest
from docent_zathura.render import (
    RenderError,
    docent_arguments,
    parse_event,
    render_request,
    run_render,
    wait_for_close,
)

FAKE_DOCENT = """#!/usr/bin/env python3
import json
import sys

request = json.loads(sys.stdin.read())
selection = request.get("selection", "")
if selection == "fail":
    print(json.dumps({"type": "error", "message": "boom"}), flush=True)
    raise SystemExit(1)
if selection == "invalid":
    print("not json", flush=True)
    raise SystemExit(0)
if selection == "exit":
    raise SystemExit(2)
if selection == "silent":
    raise SystemExit(0)
if selection == "warn":
    print(
        "[WARN  docent::cli] document context truncated", file=sys.stderr, flush=True
    )
    print(json.dumps({"type": "delta", "text": "Hello"}), flush=True)
    print(json.dumps({"type": "done", "model": "fake"}), flush=True)
    raise SystemExit(0)

print(json.dumps({"type": "delta", "text": "Hello "}), flush=True)
print(json.dumps({"type": "delta", "text": selection}), flush=True)
question = request.get("question")
if question:
    print(json.dumps({"type": "delta", "text": f" Q={question}"}), flush=True)
print(json.dumps({"type": "done", "model": "fake"}), flush=True)
"""


@pytest.fixture
def fake_docent(tmp_path: Path) -> Path:
    path = tmp_path / "docent"
    path.write_text(FAKE_DOCENT, encoding="utf-8")
    path.chmod(0o755)
    return path


def _payload(selection: str, **extra: object) -> dict[str, object]:
    payload: dict[str, object] = {
        "profile": "explain",
        "request": {
            "selection": selection,
            "document": "alpha beta",
            "file": "paper.pdf (page 1)",
        },
    }
    payload.update(extra)
    return payload


def test_parse_event_accepts_delta() -> None:
    assert parse_event('{"type": "delta", "text": "hi"}') == {
        "type": "delta",
        "text": "hi",
    }


def test_parse_event_rejects_invalid_json() -> None:
    with pytest.raises(RenderError, match="invalid stream event"):
        parse_event("not json")


def test_parse_event_rejects_missing_type() -> None:
    with pytest.raises(RenderError, match="invalid stream event"):
        parse_event('{"text": "hi"}')


def test_render_request_streams_deltas(fake_docent: Path) -> None:
    out = io.StringIO()
    err = io.StringIO()

    code = render_request(
        _payload("beta", header="paper.pdf (page 1)"),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO(""),
        stdout=out,
        stderr=err,
    )

    assert code == 0
    assert out.getvalue() == "[explain] paper.pdf (page 1)\n\nHello beta\n"
    assert err.getvalue() == ""


def test_render_request_prints_cli_warnings(fake_docent: Path) -> None:
    out = io.StringIO()
    err = io.StringIO()

    code = render_request(
        _payload("warn", header="paper.pdf (page 1)"),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO(""),
        stdout=out,
        stderr=err,
    )

    assert code == 0
    assert out.getvalue() == "[explain] paper.pdf (page 1)\n\nHello\n"
    assert err.getvalue() == "warning: document context truncated\n"


def test_render_request_prompts_for_question(fake_docent: Path) -> None:
    out = io.StringIO()
    err = io.StringIO()

    code = render_request(
        _payload("beta", profile="ask", prompt_for_question=True),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO("Why?\n"),
        stdout=out,
        stderr=err,
    )

    assert code == 0
    assert "Q=Why?" in out.getvalue()
    assert err.getvalue() == ""


def test_render_request_rejects_blank_question(fake_docent: Path) -> None:
    out = io.StringIO()
    err = io.StringIO()

    code = render_request(
        _payload("beta", profile="ask", prompt_for_question=True),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO("\n"),
        stdout=out,
        stderr=err,
    )

    assert code == 1
    assert "No question provided" in err.getvalue()


def test_render_request_reports_error_event(fake_docent: Path) -> None:
    out = io.StringIO()
    err = io.StringIO()

    code = render_request(
        _payload("fail"),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO(""),
        stdout=out,
        stderr=err,
    )

    assert code == 1
    assert "boom" in err.getvalue()


def test_render_request_rejects_invalid_stream(fake_docent: Path) -> None:
    with pytest.raises(RenderError, match="invalid stream event"):
        render_request(
            _payload("invalid"),
            docent_cmd=str(fake_docent),
            stdin=io.StringIO(""),
            stdout=io.StringIO(),
            stderr=io.StringIO(),
        )


def test_render_request_reports_missing_binary(tmp_path: Path) -> None:
    err = io.StringIO()

    code = render_request(
        _payload("beta"),
        docent_cmd=str(tmp_path / "missing"),
        stdin=io.StringIO(""),
        stdout=io.StringIO(),
        stderr=err,
    )

    assert code == 1
    assert "could not start" in err.getvalue()


def test_render_request_reports_process_status(fake_docent: Path) -> None:
    err = io.StringIO()

    code = render_request(
        _payload("exit"),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO(""),
        stdout=io.StringIO(),
        stderr=err,
    )

    assert code == 1
    assert "status 2" in err.getvalue()


def test_render_request_requires_done_event(fake_docent: Path) -> None:
    err = io.StringIO()

    code = render_request(
        _payload("silent"),
        docent_cmd=str(fake_docent),
        stdin=io.StringIO(""),
        stdout=io.StringIO(),
        stderr=err,
    )

    assert code == 1
    assert "done event" in err.getvalue()


def test_run_render_removes_request_file(fake_docent: Path, tmp_path: Path) -> None:
    temp_dir = tmp_path / "docent-zathura-test"
    temp_dir.mkdir()
    request_file = temp_dir / "request.json"
    request_file.write_text(json.dumps(_payload("beta")), encoding="utf-8")

    out = io.StringIO()
    code = run_render(
        request_file,
        stdin=io.StringIO(""),
        stdout=out,
        stderr=io.StringIO(),
        docent_cmd=str(fake_docent),
    )

    assert code == 0
    assert "Hello beta" in out.getvalue()
    assert not request_file.exists()
    assert not temp_dir.exists()


def test_run_render_rejects_invalid_json(tmp_path: Path) -> None:
    temp_dir = tmp_path / "docent-zathura-bad"
    temp_dir.mkdir()
    request_file = temp_dir / "request.json"
    request_file.write_text("{", encoding="utf-8")
    err = io.StringIO()

    code = run_render(
        request_file,
        stdin=io.StringIO(""),
        stdout=io.StringIO(),
        stderr=err,
    )

    assert code == 1
    assert "invalid render request" in err.getvalue()


class _Tty(io.StringIO):
    def isatty(self) -> bool:
        return True


def test_wait_for_close_waits_on_a_tty() -> None:
    out = io.StringIO()

    wait_for_close(_Tty("\n"), out)

    assert "Press Enter" in out.getvalue()


def test_wait_for_close_does_nothing_without_a_tty() -> None:
    out = io.StringIO()

    wait_for_close(io.StringIO(""), out)

    assert out.getvalue() == ""


def test_docent_arguments_tag_the_source() -> None:
    args = docent_arguments("docent", "explain", "French", "full")

    assert args == [
        "docent",
        "request",
        "--profile",
        "explain",
        "--stream",
        "--target-language",
        "French",
        "--context-mode",
        "full",
        "--source",
        "zathura",
    ]
