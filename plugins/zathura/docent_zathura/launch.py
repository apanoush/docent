"""Open a terminal window for the streamed answer."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import Any

__all__ = [
    "TerminalError",
    "build_terminal_argv",
    "detect_terminal",
    "launch_terminal",
]


class TerminalError(Exception):
    """No terminal emulator could be started."""


@dataclass(frozen=True, slots=True)
class TerminalSpec:
    """Arguments a terminal needs before the command it should run."""

    name: str
    prefix: tuple[str, ...]


_TERMINALS = (
    TerminalSpec("foot", ("-a", "docent", "-T", "Docent", "-e")),
    TerminalSpec("kitty", ("--class", "docent", "--title", "Docent", "-e")),
    TerminalSpec("alacritty", ("--class", "docent", "-T", "Docent", "-e")),
    TerminalSpec("xterm", ("-class", "Docent", "-T", "Docent", "-e")),
)


def detect_terminal(environ: Mapping[str, str] | None = None) -> str | None:
    """Return a terminal command from the environment or PATH."""
    environment = os.environ if environ is None else environ

    for variable in ("DOCENT_TERMINAL", "TERMINAL"):
        value = environment.get(variable)
        if value:
            return value

    for spec in _TERMINALS:
        if shutil.which(spec.name):
            return spec.name
    return None


def build_terminal_argv(terminal: str, command: Sequence[str]) -> list[str]:
    """Return the argv that runs ``command`` inside ``terminal``."""
    name = os.path.basename(terminal)
    for spec in _TERMINALS:
        if name == spec.name:
            return [terminal, *spec.prefix, *command]
    return [terminal, "-e", *command]


def launch_terminal(
    terminal: str,
    command: Sequence[str],
    runner: Any = subprocess.Popen,
) -> Any:
    """Start the terminal without waiting for it."""
    argv = build_terminal_argv(terminal, command)
    try:
        return runner(
            argv,
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError as exc:
        raise TerminalError(f"could not start {terminal}: {exc}") from exc
