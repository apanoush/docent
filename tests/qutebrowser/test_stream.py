"""Tests for the NDJSON stream parser."""

from __future__ import annotations

import json

import pytest
from docent_qutebrowser.stream import NdjsonParser, StreamError


def _line(event: dict[str, object]) -> bytes:
    return json.dumps(event, ensure_ascii=False).encode("utf-8") + b"\n"


def test_multiple_complete_lines_are_returned() -> None:
    parser = NdjsonParser()
    data = _line({"type": "delta", "text": "Hello"}) + _line(
        {"type": "delta", "text": " world"}
    )

    events = parser.feed(data)

    assert events == [
        {"type": "delta", "text": "Hello"},
        {"type": "delta", "text": " world"},
    ]


def test_partial_line_is_buffered_until_newline() -> None:
    parser = NdjsonParser()
    payload = _line({"type": "delta", "text": "Hello"})

    assert parser.feed(payload[:-1]) == []
    assert parser.feed(b"\n") == [{"type": "delta", "text": "Hello"}]


def test_every_byte_boundary_produces_the_same_events() -> None:
    payload = _line({"type": "delta", "text": "héllo"}) + _line(
        {"type": "done", "model": "test"}
    )
    expected = [
        {"type": "delta", "text": "héllo"},
        {"type": "done", "model": "test"},
    ]

    for split in range(len(payload) + 1):
        parser = NdjsonParser()
        events = parser.feed(payload[:split]) + parser.feed(payload[split:])
        events += parser.flush()
        assert events == expected, f"split at {split}"


def test_utf8_character_split_across_chunks() -> None:
    parser = NdjsonParser()
    encoded = _line({"type": "delta", "text": "é"})
    index = encoded.index(b"\xc3")

    assert parser.feed(encoded[: index + 1]) == []
    assert parser.feed(encoded[index + 1 :]) == [{"type": "delta", "text": "é"}]


def test_flush_parses_a_final_line_without_newline() -> None:
    parser = NdjsonParser()
    payload = _line({"type": "done"}).rstrip(b"\n")

    assert parser.feed(payload) == []
    assert parser.flush() == [{"type": "done"}]
    assert parser.flush() == []


def test_blank_lines_are_ignored() -> None:
    parser = NdjsonParser()

    assert parser.feed(b"\n\n  \n") == []


def test_invalid_json_raises() -> None:
    parser = NdjsonParser()

    with pytest.raises(StreamError, match="invalid stream event"):
        parser.feed(b"not json\n")


def test_non_object_json_raises() -> None:
    parser = NdjsonParser()

    with pytest.raises(StreamError, match="expected a JSON object"):
        parser.feed(b'"just a string"\n')


def test_invalid_utf8_raises() -> None:
    parser = NdjsonParser()

    with pytest.raises(StreamError, match="invalid stream event"):
        parser.feed(b'{"type":"delta","text":"\xff"}\n')
