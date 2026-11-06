"""Read the current zathura text selection from the system clipboard."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

__all__ = ["ClipboardCommand", "ClipboardError", "clipboard_commands", "read_selection"]


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
