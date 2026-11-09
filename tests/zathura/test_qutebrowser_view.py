"""Tests for the qutebrowser answer viewer."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from docent_zathura.qutebrowser_view import (
    QutebrowserViewError,
    build_page,
    serve,
)

FAKE_DOCENT = """#!/usr/bin/env python3
import json
import sys

request = json.loads(sys.stdin.read())
if "--html" not in sys.argv:
    print(json.dumps({"type": "error", "message": "missing --html"}), flush=True)
    raise SystemExit(2)
if request.get("selection") == "question" and request.get("question") != "Why?":
    print(json.dumps({"type": "error", "message": "missing question"}), flush=True)
    raise SystemExit(2)
if request.get("selection") == "fail":
    print(json.dumps({"type": "error", "message": "boom"}), flush=True)
    raise SystemExit(1)
if request.get("selection") == "warn":
    print(
        "[WARN  docent::cli] document context truncated", file=sys.stderr, flush=True
    )

print(json.dumps({"type": "delta", "text": "**bold** and `code`"}), flush=True)
print(json.dumps({"type": "html", "html": "<p><strong>bold</strong> and <code>code</code></p>"}), flush=True)
print(json.dumps({"type": "done", "model": "fake"}), flush=True)
"""

FAKE_QUTEBROWSER = """#!/usr/bin/env python3
import json
import os
import sys
import urllib.request

url = sys.argv[-1]
urllib.request.urlopen(url).read()

events = []
with urllib.request.urlopen(url.rstrip("/") + "/events") as response:
    for raw in response:
        line = raw.decode("utf-8").strip()
        if not line.startswith("data: "):
            continue
        event = json.loads(line[6:])
        if event["type"] == "question":
            request = urllib.request.Request(
                url.rstrip("/") + "/question",
                data=json.dumps({"question": "Why?"}).encode("utf-8"),
                headers={"content-type": "application/json"},
                method="POST",
            )
            urllib.request.urlopen(request).read()
        events.append(event)
        if event["type"] in {"done", "error"}:
            break

with open(os.environ["FAKE_QUTEBROWSER_OUTPUT"], "w", encoding="utf-8") as handle:
    json.dump(events, handle)
"""


@pytest.fixture
def fake_docent(tmp_path: Path) -> Path:
    path = tmp_path / "docent"
    path.write_text(FAKE_DOCENT, encoding="utf-8")
    path.chmod(0o755)
    return path


@pytest.fixture
def fake_qutebrowser(tmp_path: Path) -> Path:
    path = tmp_path / "qutebrowser"
    path.write_text(FAKE_QUTEBROWSER, encoding="utf-8")
    path.chmod(0o755)
    return path


def _payload(selection: str = "alpha") -> dict[str, object]:
    return {
        "profile": "explain",
        "request": {
            "selection": selection,
            "document": "alpha beta",
            "file": "paper.pdf (page 1)",
        },
        "header": "paper.pdf (page 1)",
    }


def test_build_page_uses_the_shared_shell_and_starts_event_source() -> None:
    page = build_page("<profile>", "<header>")

    assert "docent: &lt;profile&gt; - &lt;header&gt;" in page
    assert "--docent-bg" in page
    assert "EventSource('events')" in page
    assert "event.type === 'warning'" in page
    assert "event.type === 'question'" in page
    assert "fetch('question'" in page
    assert "d close" not in page


def test_build_page_appends_custom_css() -> None:
    page = build_page("explain", "paper.pdf", ":root { --docent-bg: #123456; }")

    assert "--docent-bg: #123456;" in page


def test_serve_streams_the_rendered_html(
    fake_docent: Path,
    fake_qutebrowser: Path,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "answer.json"
    monkeypatch.setenv("FAKE_QUTEBROWSER_OUTPUT", str(output))

    code = serve(
        _payload(),
        docent_cmd=str(fake_docent),
        target="window",
        qutebrowser_cmd=str(fake_qutebrowser),
        timeout=5,
    )

    assert code == 0
    events = json.loads(output.read_text(encoding="utf-8"))
    html = [event for event in events if event["type"] == "html"]
    assert html == [
        {
            "type": "html",
            "html": "<p><strong>bold</strong> and <code>code</code></p>",
        }
    ]
    assert events[-1]["type"] == "done"


def test_serve_prompts_for_a_question(
    fake_docent: Path,
    fake_qutebrowser: Path,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "answer.json"
    monkeypatch.setenv("FAKE_QUTEBROWSER_OUTPUT", str(output))
    payload = _payload("question")
    payload["prompt_for_question"] = True

    code = serve(
        payload,
        docent_cmd=str(fake_docent),
        target="window",
        qutebrowser_cmd=str(fake_qutebrowser),
        timeout=5,
    )

    assert code == 0
    events = json.loads(output.read_text(encoding="utf-8"))
    kinds = [event["type"] for event in events]
    assert kinds[0] == "question"
    assert "html" in kinds
    assert kinds[-1] == "done"


def test_serve_does_not_prompt_when_a_question_is_already_present(
    fake_docent: Path,
    fake_qutebrowser: Path,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "answer.json"
    monkeypatch.setenv("FAKE_QUTEBROWSER_OUTPUT", str(output))
    payload = _payload("question")
    payload["prompt_for_question"] = True
    payload["request"]["question"] = "Why?"

    code = serve(
        payload,
        docent_cmd=str(fake_docent),
        target="window",
        qutebrowser_cmd=str(fake_qutebrowser),
        timeout=5,
    )

    assert code == 0
    events = json.loads(output.read_text(encoding="utf-8"))
    assert all(event["type"] != "question" for event in events)
    assert events[-1]["type"] == "done"


def test_serve_reports_cli_warnings(
    fake_docent: Path,
    fake_qutebrowser: Path,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "answer.json"
    monkeypatch.setenv("FAKE_QUTEBROWSER_OUTPUT", str(output))

    code = serve(
        _payload("warn"),
        docent_cmd=str(fake_docent),
        target="window",
        qutebrowser_cmd=str(fake_qutebrowser),
        timeout=5,
    )

    assert code == 0
    events = json.loads(output.read_text(encoding="utf-8"))
    kinds = [event["type"] for event in events]
    assert "warning" in kinds
    warning = events[kinds.index("warning")]
    assert warning == {
        "type": "warning",
        "message": "document context truncated",
    }
    assert kinds.index("html") < kinds.index("warning") < kinds.index("done")


def test_serve_reports_stream_errors(
    fake_docent: Path,
    fake_qutebrowser: Path,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    output = tmp_path / "answer.json"
    monkeypatch.setenv("FAKE_QUTEBROWSER_OUTPUT", str(output))

    code = serve(
        _payload("fail"),
        docent_cmd=str(fake_docent),
        target="window",
        qutebrowser_cmd=str(fake_qutebrowser),
        timeout=5,
    )

    assert code == 1
    events = json.loads(output.read_text(encoding="utf-8"))
    assert events[-1] == {"type": "error", "message": "boom"}


def test_serve_reports_a_missing_qutebrowser(tmp_path: Path) -> None:
    with pytest.raises(QutebrowserViewError, match="could not start"):
        serve(
            _payload(),
            docent_cmd="docent",
            qutebrowser_cmd=str(tmp_path / "missing"),
            timeout=0.1,
        )


def test_serve_reports_a_page_that_never_connects() -> None:
    with pytest.raises(QutebrowserViewError, match="did not open"):
        serve(
            _payload(),
            docent_cmd="docent",
            qutebrowser_cmd="/bin/true",
            timeout=0.1,
        )
