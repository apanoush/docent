"""Stream a docent answer into a qutebrowser window or tab."""

from __future__ import annotations

import json
import queue
import secrets
import subprocess
import sys
import threading
from collections.abc import Mapping
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

__all__ = [
    "QutebrowserViewError",
    "build_page",
    "serve",
]


class QutebrowserViewError(Exception):
    """The qutebrowser viewer could not be started."""


def _qutebrowser_dir() -> Path:
    return Path(__file__).resolve().parents[2] / "qutebrowser"


def _ensure_qutebrowser_path() -> None:
    path = _qutebrowser_dir()
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))


_SSE_SCRIPT = """
let answerElement = document.getElementById('answer');
const status = document.getElementById('status');
const source = new EventSource('events');

function atBottom() {
  return (window.innerHeight + window.scrollY) >= (document.documentElement.scrollHeight - 8);
}

source.onmessage = (message) => {
  const event = JSON.parse(message.data);
  if (event.type === 'delta') {
    if (answerElement.dataset.placeholder === '1') {
      answerElement.textContent = '';
      answerElement.dataset.placeholder = '0';
    }
    const stick = atBottom();
    answerElement.appendChild(document.createTextNode(event.text));
    if (stick) window.scrollTo(0, document.documentElement.scrollHeight);
  } else if (event.type === 'html') {
    const stick = atBottom();
    const block = document.createElement('div');
    block.id = 'answer';
    block.className = 'markdown';
    block.innerHTML = event.html;
    answerElement.replaceWith(block);
    answerElement = block;
    if (stick) window.scrollTo(0, document.documentElement.scrollHeight);
  } else if (event.type === 'done') {
    status.textContent = event.label || 'done';
    source.close();
  } else if (event.type === 'error') {
    status.textContent = 'docent: error';
    const error = document.createElement('span');
    error.className = 'error';
    error.textContent = '\\n\\n' + event.message;
    answerElement.appendChild(error);
    source.close();
  }
};
source.onerror = () => { status.textContent = 'connection lost'; };
"""


def build_page(profile: str, header: str, custom_css: str | None = None) -> str:
    """Return the standalone answer page."""
    _ensure_qutebrowser_path()
    try:
        from docent.ui import build_shell
    except ImportError as exc:
        raise QutebrowserViewError("qutebrowser answer shell not found") from exc
    label = f"docent: {profile}"
    if header:
        label = f"{label} - {header}"
    return build_shell(
        profile,
        custom_css=custom_css,
        label=label,
        hint=None,
        script=_SSE_SCRIPT,
    )


class _Viewer:
    """One qutebrowser page backed by one docent process."""

    def __init__(
        self,
        payload: Mapping[str, Any],
        *,
        docent_cmd: str,
        target: str,
        qutebrowser_cmd: str,
        custom_css: str | None = None,
    ) -> None:
        self.payload = payload
        self.docent_cmd = docent_cmd
        self.target = target
        self.qutebrowser_cmd = qutebrowser_cmd
        self.custom_css = custom_css
        self.profile = str(payload.get("profile") or "explain")
        self.header = str(payload.get("header") or "")
        self.token = secrets.token_urlsafe(16)
        self.queue: queue.Queue[dict[str, Any]] = queue.Queue()
        self.connected = threading.Event()
        self.finished = threading.Event()
        self.cancelled = threading.Event()
        self.started = threading.Event()
        self.start_lock = threading.Lock()
        self.process: subprocess.Popen[str] | None = None
        self.html: str | None = None
        self.error = False

    def handler_class(self) -> type[BaseHTTPRequestHandler]:
        viewer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_GET(self) -> None:
                viewer.handle(self)

            def log_message(self, format: str, *args: object) -> None:
                return

        return Handler

    def handle(self, handler: BaseHTTPRequestHandler) -> None:
        path = urlsplit(handler.path).path
        if path == f"/{self.token}/":
            self.send_page(handler)
        elif path == f"/{self.token}/events":
            self.send_events(handler)
        else:
            handler.send_error(404)

    def send_page(self, handler: BaseHTTPRequestHandler) -> None:
        body = build_page(self.profile, self.header, self.custom_css).encode("utf-8")
        handler.send_response(200)
        handler.send_header("Content-Type", "text/html; charset=utf-8")
        handler.send_header("Content-Length", str(len(body)))
        handler.send_header("Cache-Control", "no-store")
        handler.end_headers()
        handler.wfile.write(body)

    def send_events(self, handler: BaseHTTPRequestHandler) -> None:
        self.connected.set()
        self.start_producer()
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream")
        handler.send_header("Cache-Control", "no-cache")
        handler.send_header("Connection", "keep-alive")
        handler.send_header("Transfer-Encoding", "chunked")
        handler.end_headers()

        def write_chunk(data: bytes) -> None:
            handler.wfile.write(f"{len(data):X}\r\n".encode() + data + b"\r\n")
            handler.wfile.flush()

        try:
            while not self.cancelled.is_set():
                try:
                    event = self.queue.get(timeout=15)
                except queue.Empty:
                    try:
                        write_chunk(b": keepalive\n\n")
                    except (BrokenPipeError, ConnectionResetError):
                        self.cancelled.set()
                        break
                    continue
                data = json.dumps(event, ensure_ascii=False)
                try:
                    write_chunk(f"data: {data}\n\n".encode())
                except (BrokenPipeError, ConnectionResetError):
                    self.cancelled.set()
                    break
                if event.get("type") in {"done", "error"}:
                    break
        finally:
            try:
                handler.wfile.write(b"0\r\n\r\n")
                handler.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
            self.finished.set()

    def start_producer(self) -> None:
        with self.start_lock:
            if self.started.is_set():
                return
            self.started.set()
        threading.Thread(target=self.produce, daemon=True).start()

    def produce(self) -> None:
        args = [
            self.docent_cmd,
            "run",
            "--profile",
            self.profile,
            "--stream",
            "--html",
            "--source",
            "zathura",
        ]
        target_language = self.payload.get("target_language")
        if isinstance(target_language, str) and target_language:
            args.extend(["--target-language", target_language])
        context_mode = self.payload.get("context_mode")
        if isinstance(context_mode, str) and context_mode:
            args.extend(["--context-mode", context_mode])
        request = self.payload.get("request")
        if not isinstance(request, dict):
            self.error = True
            self.queue.put({"type": "error", "message": "missing request data"})
            return

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
            self.error = True
            self.queue.put(
                {
                    "type": "error",
                    "message": f"could not start {self.docent_cmd}: {exc}",
                }
            )
            return
        self.process = process

        stderr_lines: list[str] = []

        def read_stderr() -> None:
            assert process.stderr is not None
            stderr_lines.extend(process.stderr)

        stderr_thread = threading.Thread(target=read_stderr, daemon=True)
        stderr_thread.start()

        error_message: str | None = None
        done = False
        model: str | None = None
        try:
            assert process.stdin is not None
            process.stdin.write(json.dumps(request, ensure_ascii=False))
            process.stdin.close()
            assert process.stdout is not None
            for line in process.stdout:
                if self.cancelled.is_set():
                    break
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    error_message = "invalid stream event"
                    break
                kind = event.get("type")
                if kind == "delta":
                    text = event.get("text")
                    if isinstance(text, str):
                        self.queue.put({"type": "delta", "text": text})
                elif kind == "html":
                    body = event.get("html")
                    if isinstance(body, str):
                        self.html = body
                elif kind == "done":
                    model = (
                        event.get("model")
                        if isinstance(event.get("model"), str)
                        else None
                    )
                    done = True
                    break
                elif kind == "error":
                    message = event.get("message")
                    error_message = (
                        message if isinstance(message, str) else "unknown error"
                    )
                    break
                else:
                    error_message = f"unknown stream event: {kind}"
                    break
            process.wait(timeout=5)
        except (BrokenPipeError, subprocess.TimeoutExpired) as exc:
            error_message = str(exc)
            self.terminate(process)
        finally:
            stderr_thread.join(timeout=1)

        if self.cancelled.is_set():
            self.terminate(process)
            return
        if error_message is not None:
            self.error = True
            self.queue.put({"type": "error", "message": error_message})
            return
        if not done:
            self.error = True
            self.queue.put(
                {"type": "error", "message": "docent stream ended without a done event"}
            )
            return
        if process.returncode != 0:
            detail = "".join(stderr_lines).strip()
            message = f"docent exited with status {process.returncode}"
            if detail:
                message = f"{message}: {detail}"
            self.error = True
            self.queue.put({"type": "error", "message": message})
            return

        if self.html is not None:
            self.queue.put({"type": "html", "html": self.html})
        self.queue.put({"type": "done", "label": self.label(model)})

    def label(self, model: str | None) -> str:
        label = f"docent: {self.profile}"
        if model:
            label = f"{label} - {model}"
        if self.header:
            label = f"{label} - {self.header}"
        return label

    def terminate(self, process: subprocess.Popen[str]) -> None:
        if process.poll() is not None:
            return
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()

    def cancel(self) -> None:
        self.cancelled.set()
        if self.process is not None:
            self.terminate(self.process)


def serve(
    payload: Mapping[str, Any],
    *,
    docent_cmd: str,
    target: str = "window",
    qutebrowser_cmd: str = "qutebrowser",
    custom_css: str | None = None,
    timeout: float = 30.0,
) -> int:
    """Open the answer in qutebrowser and stream until it finishes."""
    viewer = _Viewer(
        payload,
        docent_cmd=docent_cmd,
        target=target,
        qutebrowser_cmd=qutebrowser_cmd,
        custom_css=custom_css,
    )
    server = ThreadingHTTPServer(("127.0.0.1", 0), viewer.handler_class())
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    url = f"http://127.0.0.1:{port}/{viewer.token}/"
    try:
        subprocess.Popen(
            [qutebrowser_cmd, f"--target={target}", url],
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError as exc:
        server.shutdown()
        server.server_close()
        raise QutebrowserViewError(f"could not start {qutebrowser_cmd}: {exc}") from exc

    if not viewer.connected.wait(timeout=timeout):
        server.shutdown()
        server.server_close()
        raise QutebrowserViewError("qutebrowser did not open the answer page")

    viewer.finished.wait()
    server.shutdown()
    server.server_close()
    viewer.cancel()
    return 1 if viewer.error else 0
