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

from .clipboard import ClipboardError, read_best_selection
from .launch import TerminalError, detect_terminal, launch_terminal
from .pdf import PDFError, extract_text, page_window
from .qutebrowser_view import QutebrowserViewError
from .qutebrowser_view import serve as serve_qutebrowser
from .render import RenderError, run_render
from .request import RequestError, build_document_request, build_request

__all__ = ["build_parser", "find_docent", "main"]

ENTRY = Path(__file__).resolve().parent.parent / "docent-zathura"
PLUGIN_DIR = Path(__file__).resolve().parents[2] / "nvim"
VIEWER_SCRIPT = Path(__file__).resolve().parent / "viewer.lua"


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
            'map <C-e> exec "/path/to/docent-zathura explain $FILE $PAGE"'
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

    summarize = subcommands.add_parser(
        "summarize",
        help="summarize the page, or the selection when the clipboard matches it",
    )
    _add_capture_args(summarize)

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
    target = parser.add_mutually_exclusive_group()
    target.add_argument(
        "--clipboard",
        action="store_true",
        help="force the CLIPBOARD selection",
    )
    target.add_argument(
        "--primary",
        action="store_true",
        help="force the PRIMARY selection",
    )
    parser.add_argument("--terminal", help="terminal emulator to use")
    parser.add_argument(
        "--viewer",
        choices=("terminal", "nvim", "qutebrowser"),
        default="terminal",
        help="where to show the answer",
    )
    parser.add_argument(
        "--nvim",
        default="nvim",
        help="Neovim executable for --viewer nvim",
    )
    parser.add_argument(
        "--qutebrowser",
        default="qutebrowser",
        help="qutebrowser executable for --viewer qutebrowser",
    )
    parser.add_argument(
        "--qutebrowser-target",
        choices=(
            "auto",
            "tab",
            "tab-bg",
            "tab-silent",
            "tab-bg-silent",
            "window",
            "private-window",
        ),
        default="window",
        help="qutebrowser --target value for --viewer qutebrowser",
    )
    parser.add_argument(
        "--qutebrowser-css",
        type=Path,
        help="CSS file appended after the qutebrowser answer styles",
    )
    parser.add_argument(
        "--docent",
        default="docent",
        help="path or command name; common locations are searched when not on PATH",
    )
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


def _resolve_executable(value: str) -> str | None:
    if os.sep in value or value.startswith("."):
        path = Path(value).expanduser().resolve()
        return str(path) if path.is_file() else None
    return shutil.which(value)


def _docent_candidates() -> tuple[Path, ...]:
    home = Path.home()
    return (
        home / ".local" / "bin" / "docent",
        home / ".cargo" / "bin" / "docent",
        Path("/usr/local/bin/docent"),
        Path("/usr/bin/docent"),
    )


def find_docent(value: str) -> str:
    """Resolve a docent command, searching common install locations."""
    resolved = _resolve_docent(value)
    if os.sep in resolved:
        return resolved

    found = shutil.which(resolved)
    if found:
        return found

    for candidate in _docent_candidates():
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
    return resolved


def _capture(args: argparse.Namespace) -> int:
    path = args.file.expanduser().resolve()
    if not path.is_file():
        raise ValueError(f"not a file: {path}")
    if args.page < 1:
        raise ValueError("page numbers start at 1")
    if args.context_pages < 0:
        raise ValueError("context pages must not be negative")

    start, end = page_window(args.page, args.context_pages)
    document = extract_text(path, start, end)

    target = None
    if args.clipboard:
        target = "clipboard"
    elif args.primary:
        target = "primary"

    label = f"{path.name} (page {args.page})"
    question = getattr(args, "question", None)
    if args.command == "summarize":
        # A selection is optional: use the clipboard when it matches the page,
        # otherwise summarize the extracted page window.
        try:
            request = build_request(
                read_best_selection(document, target), document, label
            )
        except ClipboardError:
            request = build_document_request(document, label)
    else:
        request = build_request(
            read_best_selection(document, target), document, label, question
        )
    docent_cmd = find_docent(args.docent)

    payload = {
        "profile": getattr(args, "profile", args.command),
        "request": request,
        "target_language": getattr(args, "target_language", None),
        "context_mode": args.context_mode,
        "prompt_for_question": args.command == "ask" and not request.get("question"),
        "header": label,
        "docent": docent_cmd,
    }

    if args.viewer == "qutebrowser":
        qutebrowser = _resolve_executable(args.qutebrowser)
        if qutebrowser is None:
            raise ValueError(
                "qutebrowser not found; install qutebrowser or pass --qutebrowser"
            )
        custom_css = None
        if args.qutebrowser_css is not None:
            css_path = args.qutebrowser_css.expanduser().resolve()
            try:
                custom_css = css_path.read_text(encoding="utf-8")
            except OSError as exc:
                raise ValueError(f"could not read qutebrowser CSS: {exc}") from exc
        return serve_qutebrowser(
            payload,
            docent_cmd=docent_cmd,
            target=args.qutebrowser_target,
            qutebrowser_cmd=qutebrowser,
            custom_css=custom_css,
        )

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

    env = None
    if args.viewer == "nvim":
        nvim = _resolve_executable(args.nvim)
        if nvim is None:
            shutil.rmtree(temp_dir, ignore_errors=True)
            raise ValueError("nvim not found; install Neovim or pass --nvim")
        if not PLUGIN_DIR.is_dir():
            shutil.rmtree(temp_dir, ignore_errors=True)
            raise ValueError(f"Neovim plugin not found: {PLUGIN_DIR}")
        if not VIEWER_SCRIPT.is_file():
            shutil.rmtree(temp_dir, ignore_errors=True)
            raise ValueError(f"viewer script not found: {VIEWER_SCRIPT}")

        viewer_command = [
            nvim,
            "--cmd",
            "lua vim.opt.runtimepath:append(vim.env.DOCENT_ZATHURA_NVIM)",
            "-c",
            "lua dofile(vim.env.DOCENT_ZATHURA_VIEWER)",
        ]
        env = {
            **os.environ,
            "DOCENT_ZATHURA_REQUEST": str(request_file),
            "DOCENT_ZATHURA_VIEWER": str(VIEWER_SCRIPT),
            "DOCENT_ZATHURA_NVIM": str(PLUGIN_DIR),
        }
    else:
        viewer_command = [
            sys.executable,
            str(ENTRY),
            "render",
            "--docent",
            docent_cmd,
            str(request_file),
        ]

    try:
        launch_terminal(terminal, viewer_command, env=env)
    except TerminalError:
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise
    return 0


def _show_error(message: str) -> None:
    print(f"docent-zathura: {message}", file=sys.stderr)
    if sys.stderr.isatty():
        return
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        return

    if shutil.which("zenity"):
        try:
            result = subprocess.run(
                ["zenity", "--error", "--title=Docent", "--text", message],
                check=False,
                timeout=30,
            )
            if result.returncode == 0:
                return
        except (OSError, subprocess.TimeoutExpired):
            pass

    if shutil.which("notify-send"):
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
        QutebrowserViewError,
        RequestError,
        TerminalError,
        OSError,
        ValueError,
    ) as exc:
        _show_error(str(exc))
        return 1
