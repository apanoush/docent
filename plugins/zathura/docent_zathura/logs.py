"""Helpers for the docent CLI's stderr output."""

from __future__ import annotations

__all__ = ["warning_lines"]


def warning_lines(text: str) -> list[str]:
    """Return the warnings the CLI logged, without env_logger prefixes.

    Only lines that start with ``[WARN`` are returned, so debug or info output
    from a custom ``RUST_LOG`` setting is not shown as a warning.
    """
    warnings = []
    for line in text.splitlines():
        if not line.startswith("[WARN"):
            continue
        _, separator, message = line.partition("] ")
        if separator and message:
            warnings.append(message)
    return warnings
