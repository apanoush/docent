"""Tests for the qutebrowser answer viewer."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from docent_zathura.qutebrowser_view import (
    QutebrowserViewError,
    build_shell,
    render_markdown,
    serve,
)

FAKE_DOCENT = """#!/usr/bin/env python3
import json
import sys

request = json.loads(sys.stdin.read())
if request.get("selection") == "fail":
    print(json.dumps({"type": "error", "message": "boom"}), flush=True)
    raise SystemExit(1)

print(json.dumps({"type": "delta", "text": "**bold** and `code`"}), flush=True)
print(json.dumps({"type": "done", "model": "fake"}), flush=True)
"""

FAKE_QUTEBROWSER = """#!/usr/bin/env python3
import json
import os
import sys
import urllib.request

url = sys.argv[-1]
urllib.request.urlopen(url).read()

with urllib.request.urlopen(url.rstrip("/") + "/events") as response:
    for raw in response:
        line = raw.decode("utf-8").strip()
        if not line.startswith("data: "):
            continue
        event = json.loads(line[6:])
        if event["type"] in {"html", "error"}:
            with open(os.environ["FAKE_QUTEBROWSER_OUTPUT"], "w", encoding="utf-8") as handle:
                json.dump(event, handle)
        if event["type"] in {"done", "error"}:
            break
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


def test_build_shell_escapes_header_and_starts_event_source() -> None:
    shell = build_shell("<profile>", "<header>")

    assert "Docent: &lt;profile&gt;" in shell
    assert "&lt;profile&gt; - &lt;header&gt;" in shell
    assert "EventSource('events')" in shell


def test_render_markdown_uses_the_shared_renderer() -> None:
    assert "<strong>bold</strong>" in render_markdown("**bold**")
    assert "&lt;script&gt;" in render_markdown("<script>alert(1)</script>")


def test_serve_streams_rendered_markdown(
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
    event = json.loads(output.read_text(encoding="utf-8"))
    assert event["type"] == "html"
    assert "<strong>bold</strong>" in event["html"]
    assert "<code>code</code>" in event["html"]


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
    event = json.loads(output.read_text(encoding="utf-8"))
    assert event == {"type": "error", "message": "boom"}


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
