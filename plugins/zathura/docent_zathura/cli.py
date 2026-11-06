"""Command line entry point for the zathura adapter."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from .clipboard import ClipboardError, read_selection
from .launch import TerminalError, detect_terminal, launch_terminal
from .pdf import PDFError, extract_text, page_window
from .render import RenderError, run_render
from .request import RequestError, build_request

__all__ = ["build_parser", "main"]

ENTRY = Path(__file__).resolve().parent.parent / "docent-zathura"


def build_parser() -> argparse.ArgumentParser:
    """Return the argument parser for the adapter."""
    parser = argparse.ArgumentParser(
        prog="docent-zathura",
        description=(
            "Send the selected text from zathura to docent and stream the "
            "answer in a terminal window."
        ),
        epilog=(
            "Bind it in zathurarc, for example: "
            "map <C-e> exec /path/to/docent-zathura explain $FILE $PAGE"
        ),
    )
    subcommands = parser.add_subparsers(dest="command", required=True)

    explain = subcommands.add_parser("explain", help="explain the selection")
    _add_capture_args(explain)

    ask = subcommands.add_parser("ask", help="ask a question about the selection")
    _add_capture_args(ask)
    ask.add_argument(
        "--question", help="question text; prompt in the terminal if omitted"
    )

    translate = subcommands.add_parser("translate", help="translate the selection")
    _add_capture_args(translate)
    translate.add_argument(
        "--target-language",
        "--to",
        dest="target_language",
        help="target language; defaults to the profile setting",
    )

    custom = subcommands.add_parser("do", help="run a custom profile")
    custom.add_argument("profile", help="profile name")
    _add_capture_args(custom)
    custom.add_argument(
        "--question", help="question text for profiles that require one"
    )

    render = subcommands.add_parser(
        "render",
        help="internal: render a request file in a terminal",
    )
    render.add_argument("request_file", type=Path)
    render.add_argument("--docent", default="docent")

    return parser


def _add_capture_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("file", type=Path, help="PDF file")
    parser.add_argument("page", type=int, help="current page number")
    parser.add_argument(
        "--clipboard",
        action="store_true",
        help="read the CLIPBOARD selection instead of PRIMARY",
    )
    parser.add_argument("--terminal", help="terminal emulator to use")
    parser.add_argument("--docent", default="docent", help="path to the docent binary")
    parser.add_argument(
        "--context-pages",
        type=int,
        default=1,
        help="pages of context before and after the current page",
    )
    parser.add_argument(
        "--context-mode",
        choices=("auto", "full", "none"),
        help="override the context mode",
    )


def _resolve_docent(value: str) -> str:
    if os.sep in value or value.startswith("."):
        return str(Path(value).expanduser().resolve())
    return value


def _capture(args: argparse.Namespace) -> int:
    path = args.file.expanduser().resolve()
    if not path.is_file():
        raise ValueError(f"not a file: {path}")
    if args.page < 1:
        raise ValueError("page numbers start at 1")
    if args.context_pages < 0:
        raise ValueError("context pages must not be negative")

    target = "clipboard" if args.clipboard else "primary"
    selection = read_selection(target)
    start, end = page_window(args.page, args.context_pages)
    document = extract_text(path, start, end)

    label = f"{path.name} (page {args.page})"
    question = getattr(args, "question", None)
    request = build_request(selection, document, label, question)

    payload = {
        "profile": getattr(args, "profile", args.command),
        "request": request,
        "target_language": getattr(args, "target_language", None),
        "context_mode": args.context_mode,
        "prompt_for_question": args.command == "ask" and not request.get("question"),
        "header": label,
    }

    temp_dir = Path(
        tempfile.mkdtemp(
            prefix="docent-zathura-",
            dir=os.environ.get("XDG_RUNTIME_DIR") or None,
        )
    )
    request_file = temp_dir / "request.json"
    request_file.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    os.chmod(request_file, 0o600)

    terminal = args.terminal or detect_terminal()
    if terminal is None:
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise TerminalError(
            "no terminal emulator found; set DOCENT_TERMINAL or pass --terminal"
        )

    render_command = [
        sys.executable,
        str(ENTRY),
        "render",
        "--docent",
        _resolve_docent(args.docent),
        str(request_file),
    ]
    try:
        launch_terminal(terminal, render_command)
    except TerminalError:
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise
    return 0


def _show_error(message: str) -> None:
    print(f"docent-zathura: {message}", file=sys.stderr)
    if sys.stderr.isatty():
        return
    if shutil.which("notify-send") is None:
        return
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        return
    try:
        subprocess.run(
            ["notify-send", "--app-name=Docent", "Docent", message],
            check=False,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def main(argv: list[str] | None = None) -> int:
    """Run one adapter command and return a process exit code."""
    args = build_parser().parse_args(argv)

    if args.command == "render":
        try:
            return run_render(
                args.request_file,
                stdin=sys.stdin,
                stdout=sys.stdout,
                stderr=sys.stderr,
                docent_cmd=args.docent,
            )
        except RenderError as exc:
            print(f"docent-zathura: {exc}", file=sys.stderr)
            return 1
        except KeyboardInterrupt:
            return 130

    try:
        return _capture(args)
    except KeyboardInterrupt:
        return 130
    except (
        ClipboardError,
        PDFError,
        RequestError,
        TerminalError,
        OSError,
        ValueError,
    ) as exc:
        _show_error(str(exc))
        return 1
