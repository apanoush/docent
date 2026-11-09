"""Read the current zathura text selection from the system clipboard."""

from __future__ import annotations

import os
import re
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


_HYPHEN_BREAK = re.compile(r"-\s+")


def _normalize_for_match(text: str) -> str:
    """Join hyphenated line breaks and collapse whitespace."""
    return " ".join(_HYPHEN_BREAK.sub("", text).split())


def _match_score(selection: str, document: str) -> tuple[int, int] | None:
    """Return ``(length, exact)`` when ``selection`` occurs in ``document``.

    Longer matches win, so a short stale clipboard token cannot shadow the
    passage the user selected. An exact occurrence breaks ties against a
    normalized one.
    """
    if not selection.strip() or not document.strip():
        return None
    if selection in document:
        return (len(selection), 1)
    normalized_selection = _normalize_for_match(selection)
    normalized_document = _normalize_for_match(document)
    if normalized_selection and normalized_selection in normalized_document:
        return (len(normalized_selection), 0)
    return None


def matches_document(selection: str, document: str) -> bool:
    """Return whether ``selection`` occurs in ``document``.

    The exact check is tried first. A normalized check then covers line breaks,
    spacing differences, and hyphenated line breaks between the viewer and the
    extracted page text.
    """
    return _match_score(selection, document) is not None


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

    With no explicit target, both PRIMARY and CLIPBOARD are read. The longest
    text that occurs in the page wins, so a short stale token such as ``LLM``
    cannot shadow the passage the user selected, whichever clipboard zathura
    writes to. An exact occurrence breaks ties against a normalized one, and
    PRIMARY breaks remaining ties because it is zathura's default.

    When neither clipboard text occurs in the page, the longer text is used so
    a page-extraction mismatch does not discard the actual selection.
    """
    if target is not None:
        return read_selection(target, environ, runner)

    errors: list[str] = []
    primary: str | None = None
    clipboard: str | None = None
    try:
        primary = read_selection("primary", environ, runner)
    except ClipboardError as exc:
        errors.append(str(exc))
    try:
        clipboard = read_selection("clipboard", environ, runner)
    except ClipboardError as exc:
        errors.append(str(exc))

    matches: list[tuple[tuple[int, int], str]] = []
    if primary is not None:
        score = _match_score(primary, document)
        if score is not None:
            matches.append((score, primary))
    if clipboard is not None:
        score = _match_score(clipboard, document)
        if score is not None:
            matches.append((score, clipboard))
    if matches:
        return max(matches, key=lambda match: match[0])[1]

    if primary is not None and clipboard is not None:
        return primary if len(primary) >= len(clipboard) else clipboard
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
