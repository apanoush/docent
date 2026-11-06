"""Read the current zathura text selection from the system clipboard."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

__all__ = [
    "ClipboardCommand",
    "ClipboardError",
    "clipboard_commands",
    "matches_document",
    "read_best_selection",
    "read_selection",
]


class ClipboardError(Exception):
    """The selection could not be read from the clipboard."""


@dataclass(frozen=True, slots=True)
class ClipboardCommand:
    """A command that prints one clipboard's contents to stdout."""

    name: str
    argv: tuple[str, ...]


def _is_wayland(environ: Mapping[str, str]) -> bool:
    return (
        bool(environ.get("WAYLAND_DISPLAY"))
        or environ.get("XDG_SESSION_TYPE") == "wayland"
    )


def _is_x11(environ: Mapping[str, str]) -> bool:
    return bool(environ.get("DISPLAY"))


def clipboard_commands(
    target: str, environ: Mapping[str, str] | None = None
) -> list[ClipboardCommand]:
    """Return clipboard commands for ``target`` in preference order.

    ``target`` is ``primary`` for zathura's default selection clipboard or
    ``clipboard`` for the regular clipboard.
    """
    if target not in {"primary", "clipboard"}:
        raise ValueError("target must be 'primary' or 'clipboard'")

    environment = os.environ if environ is None else environ
    commands: list[ClipboardCommand] = []

    if _is_wayland(environment):
        argv = ["wl-paste", "--no-newline"]
        if target == "primary":
            argv.append("--primary")
        commands.append(ClipboardCommand("wl-paste", tuple(argv)))

    if _is_x11(environment):
        if shutil.which("xclip"):
            commands.append(
                ClipboardCommand(
                    "xclip",
                    ("xclip", "-selection", target, "-o"),
                )
            )
        if shutil.which("xsel"):
            flag = "--primary" if target == "primary" else "--clipboard"
            commands.append(ClipboardCommand("xsel", ("xsel", flag, "--output")))

    return commands


def read_selection(
    target: str = "primary",
    environ: Mapping[str, str] | None = None,
    runner: Any = subprocess.run,
) -> str:
    """Return the selected text, trying the available clipboard tools."""
    commands = clipboard_commands(target, environ)
    if not commands:
        raise ClipboardError(
            "no clipboard tool found; install wl-clipboard, xclip, or xsel"
        )

    errors: list[str] = []
    saw_empty = False
    for command in commands:
        try:
            result = runner(
                command.argv,
                capture_output=True,
                text=True,
                timeout=5,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            errors.append(f"{command.name}: {exc}")
            continue

        if result.returncode != 0:
            detail = (result.stderr or "").strip() or "failed"
            errors.append(f"{command.name}: {detail}")
            continue

        text = (result.stdout or "").rstrip("\n")
        if text.strip():
            return text
        saw_empty = True

    if saw_empty:
        raise ClipboardError("no text selected; select text with the mouse first")
    raise ClipboardError("; ".join(errors))


def matches_document(selection: str, document: str) -> bool:
    """Return whether ``selection`` occurs in ``document``.

    The exact check is tried first. A whitespace-normalized check then covers
    line breaks and spacing differences between the viewer and pdftotext.
    """
    if not selection.strip() or not document.strip():
        return False
    if selection in document:
        return True
    normalized_selection = " ".join(selection.split())
    normalized_document = " ".join(document.split())
    return bool(normalized_selection) and normalized_selection in normalized_document


def _empty_selection(errors: list[str]) -> bool:
    return all(
        "Nothing is copied" in error or "no text selected" in error for error in errors
    )


def read_best_selection(
    document: str,
    target: str | None = None,
    environ: Mapping[str, str] | None = None,
    runner: Any = subprocess.run,
) -> str:
    """Return the selection that best matches the extracted page.

    With no explicit target, PRIMARY is tried first because it is zathura's
    default. If it is empty or does not occur in the page, CLIPBOARD is tried.
    The fallback keeps the default target working when the selection cannot be
    matched exactly, while avoiding unrelated clipboard text.
    """
    if target is not None:
        return read_selection(target, environ, runner)

    errors: list[str] = []
    primary: str | None = None
    try:
        primary = read_selection("primary", environ, runner)
    except ClipboardError as exc:
        errors.append(str(exc))

    if primary is not None and matches_document(primary, document):
        return primary

    clipboard: str | None = None
    try:
        clipboard = read_selection("clipboard", environ, runner)
    except ClipboardError as exc:
        errors.append(str(exc))

    if clipboard is not None and matches_document(clipboard, document):
        return clipboard
    if primary is not None:
        return primary
    if clipboard is not None:
        raise ClipboardError(
            "the CLIPBOARD selection does not appear on this page; "
            "select text first or pass --clipboard to force it"
        )
    if _empty_selection(errors):
        raise ClipboardError("no text selected; select text with the mouse first")
    raise ClipboardError("; ".join(errors))
