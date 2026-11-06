"""Tests for the docent request builder."""

from __future__ import annotations

from docent.request import Capture, build_request


def test_minimal_request_contains_only_selection_and_document() -> None:
    request = build_request(Capture(selection="alpha", document="alpha beta"))

    assert request == {"selection": "alpha", "document": "alpha beta"}


def test_file_is_included_when_present() -> None:
    request = build_request(
        Capture(selection="alpha", document="alpha beta", file="notes.md")
    )

    assert request["file"] == "notes.md"


def test_empty_file_is_omitted() -> None:
    request = build_request(Capture(selection="alpha", document="alpha beta", file=""))

    assert "file" not in request


def test_question_is_included_when_non_empty() -> None:
    request = build_request(
        Capture(selection="alpha", document="alpha beta"),
        question="Why does this matter?",
    )

    assert request["question"] == "Why does this matter?"


def test_blank_question_is_omitted() -> None:
    request = build_request(
        Capture(selection="alpha", document="alpha beta"),
        question="   \n\t",
    )

    assert "question" not in request


def test_selection_whitespace_is_preserved() -> None:
    request = build_request(Capture(selection="  alpha\n", document="x  alpha\n y"))

    assert request["selection"] == "  alpha\n"


def test_matching_utf8_offsets_are_included() -> None:
    document = "aé beta"
    request = build_request(
        Capture(
            selection="é",
            document=document,
            byte_start=1,
            byte_end=3,
        )
    )

    assert request["byte_start"] == 1
    assert request["byte_end"] == 3


def test_mismatched_offsets_are_omitted() -> None:
    request = build_request(
        Capture(
            selection="beta",
            document="alpha beta gamma",
            byte_start=0,
            byte_end=4,
        )
    )

    assert "byte_start" not in request
    assert "byte_end" not in request


def test_out_of_bounds_offsets_are_omitted() -> None:
    request = build_request(
        Capture(
            selection="beta",
            document="alpha beta",
            byte_start=100,
            byte_end=200,
        )
    )

    assert "byte_start" not in request
    assert "byte_end" not in request


def test_partial_offsets_are_omitted() -> None:
    request = build_request(
        Capture(
            selection="beta",
            document="alpha beta",
            byte_start=6,
        )
    )

    assert "byte_start" not in request
    assert "byte_end" not in request


def test_offsets_inside_a_utf8_sequence_are_omitted() -> None:
    request = build_request(
        Capture(
            selection="é",
            document="aé",
            byte_start=1,
            byte_end=2,
        )
    )

    assert "byte_start" not in request
    assert "byte_end" not in request
