"""Render docent's NDJSON stream into a terminal."""

from __future__ import annotations

import json
import subprocess
import threading
from collections.abc import Mapping
from pathlib import Path
from typing import Any, TextIO

__all__ = [
    "RenderError",
    "docent_arguments",
    "parse_event",
    "render_request",
    "run_render",
    "wait_for_close",
]


class RenderError(Exception):
    """The render request or the stream is invalid."""


def parse_event(line: str) -> dict[str, Any]:
    """Parse one NDJSON line and require a string ``type`` field."""
    try:
        event = json.loads(line)
    except json.JSONDecodeError as exc:
        raise RenderError(f"invalid stream event: {line.rstrip()}") from exc

    if not isinstance(event, dict) or not isinstance(event.get("type"), str):
        raise RenderError("invalid stream event")
    return event


def _terminate(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def docent_arguments(
    docent_cmd: str,
    profile: str,
    target_language: str | None,
    context_mode: str | None,
) -> list[str]:
    """Build the argv passed to ``docent run``."""
    args = [docent_cmd, "run", "--profile", profile, "--stream"]
    if target_language:
        args.extend(["--target-language", target_language])
    if context_mode:
        args.extend(["--context-mode", context_mode])
    args.extend(["--source", "zathura"])
    return args


def render_request(
    payload: Mapping[str, Any],
    *,
    docent_cmd: str = "docent",
    stdin: TextIO,
    stdout: TextIO,
    stderr: TextIO,
) -> int:
    """Run docent for one request and print the streamed answer."""
    profile = payload.get("profile")
    if not isinstance(profile, str) or not profile:
        raise RenderError("missing profile in render request")

    request = payload.get("request")
    if not isinstance(request, dict):
        raise RenderError("missing request in render request")

    if (
        payload.get("prompt_for_question")
        and not str(request.get("question", "")).strip()
    ):
        stdout.write("Question: ")
        stdout.flush()
        answer = stdin.readline().strip()
        if not answer:
            stderr.write("No question provided.\n")
            return 1
        request = {**request, "question": answer}

    target_language = payload.get("target_language")
    context_mode = payload.get("context_mode")
    args = docent_arguments(
        docent_cmd,
        profile,
        target_language if isinstance(target_language, str) else None,
        context_mode if isinstance(context_mode, str) else None,
    )

    header = payload.get("header")
    if isinstance(header, str) and header:
        stdout.write(f"[{profile}] {header}\n\n")
        stdout.flush()

    try:
        process = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
    except OSError as exc:
        stderr.write(f"could not start {docent_cmd}: {exc}\n")
        return 1

    stderr_lines: list[str] = []

    def read_stderr() -> None:
        assert process.stderr is not None
        stderr_lines.extend(process.stderr)

    stderr_thread = threading.Thread(target=read_stderr, daemon=True)
    stderr_thread.start()

    error_message: str | None = None
    done = False
    try:
        assert process.stdin is not None
        process.stdin.write(json.dumps(request, ensure_ascii=False))
        process.stdin.close()

        assert process.stdout is not None
        for line in process.stdout:
            event = parse_event(line)
            kind = event["type"]
            if kind == "delta":
                text = event.get("text")
                if isinstance(text, str):
                    stdout.write(text)
                    stdout.flush()
            elif kind == "done":
                done = True
                break
            elif kind == "error":
                message = event.get("message")
                error_message = message if isinstance(message, str) else "unknown error"
                break
            else:
                raise RenderError(f"unknown stream event: {kind}")

        process.wait(timeout=5)
    except BrokenPipeError:
        _terminate(process)
        try:
            stderr.write("\ndocent closed its input early.\n")
            detail = "".join(stderr_lines).strip()
            if detail:
                stderr.write(detail + "\n")
        except BrokenPipeError:
            pass
        return 1
    except KeyboardInterrupt:
        _terminate(process)
        return 130
    except RenderError:
        _terminate(process)
        raise
    except subprocess.TimeoutExpired:
        _terminate(process)
        stderr.write("\ndocent did not exit after closing its output.\n")
        return 1
    finally:
        stderr_thread.join(timeout=1)

    if error_message is not None:
        stderr.write(f"\nError: {error_message}\n")
        return 1

    if process.returncode != 0:
        detail = "".join(stderr_lines).strip()
        stderr.write(f"\ndocent exited with status {process.returncode}.\n")
        if detail:
            stderr.write(detail + "\n")
        return 1

    if not done:
        stderr.write("\ndocent stream ended without a done event.\n")
        return 1

    stdout.write("\n")
    stdout.flush()
    return 0


def wait_for_close(stdin: TextIO, stdout: TextIO) -> None:
    """Keep the terminal window open until the reader presses Enter."""
    if not stdin.isatty():
        return
    try:
        stdout.write("Press Enter to close.\n")
        stdout.flush()
        stdin.readline()
    except (EOFError, KeyboardInterrupt, BrokenPipeError):
        pass


def run_render(
    request_file: Path,
    *,
    stdin: TextIO,
    stdout: TextIO,
    stderr: TextIO,
    docent_cmd: str = "docent",
) -> int:
    """Read a private request file, render it, and remove the file."""
    try:
        raw = request_file.read_text(encoding="utf-8")
    except OSError as exc:
        stderr.write(f"could not read render request: {exc}\n")
        return 1
    finally:
        parent = request_file.parent
        request_file.unlink(missing_ok=True)
        if parent.name.startswith("docent-zathura-"):
            try:
                parent.rmdir()
            except OSError:
                pass

    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        stderr.write(f"invalid render request: {exc}\n")
        wait_for_close(stdin, stdout)
        return 1

    if not isinstance(payload, dict):
        stderr.write("invalid render request: expected an object\n")
        wait_for_close(stdin, stdout)
        return 1

    try:
        code = render_request(
            payload,
            docent_cmd=docent_cmd,
            stdin=stdin,
            stdout=stdout,
            stderr=stderr,
        )
    except RenderError as exc:
        stderr.write(f"docent-zathura: {exc}\n")
        code = 1

    wait_for_close(stdin, stdout)
    return code
