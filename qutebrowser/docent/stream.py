"""Incremental parser for docent's newline-delimited JSON stream."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, cast

__all__ = ["NdjsonParser", "StreamError"]


class StreamError(Exception):
    """Raised when a stream line is not a JSON object."""


@dataclass(slots=True)
class NdjsonParser:
    """Accumulate bytes and return complete JSON events.

    ``QProcess`` can split stdout at arbitrary byte boundaries, including
    inside a UTF-8 sequence. Only complete lines are decoded, so a partial
    character or a partial JSON object never reaches the caller.
    """

    _buffer: bytearray = field(default_factory=bytearray)

    def feed(self, data: bytes) -> list[dict[str, Any]]:
        """Add bytes and return every complete event now available."""
        if data:
            self._buffer.extend(data)

        events: list[dict[str, Any]] = []
        while True:
            newline = self._buffer.find(b"\n")
            if newline < 0:
                break
            line = bytes(self._buffer[:newline])
            del self._buffer[: newline + 1]
            event = _decode(line)
            if event is not None:
                events.append(event)
        return events

    def flush(self) -> list[dict[str, Any]]:
        """Parse a final line that was not terminated by a newline."""
        if not self._buffer:
            return []
        line = bytes(self._buffer)
        self._buffer.clear()
        event = _decode(line)
        return [] if event is None else [event]


def _decode(line: bytes) -> dict[str, Any] | None:
    """Decode one line, ignoring blank lines."""
    if not line.strip():
        return None

    try:
        value = json.loads(line)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise StreamError(f"invalid stream event: {exc}") from exc

    if not isinstance(value, dict):
        raise StreamError("invalid stream event: expected a JSON object")
    return cast("dict[str, Any]", value)
