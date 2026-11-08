"""Tests for the JSON request builder."""

from __future__ import annotations

import pytest
from docent_zathura.request import RequestError, build_document_request, build_request


def test_build_request_adds_exact_byte_offsets() -> None:
    request = build_request("beta", "alpha beta gamma", "paper.pdf (page 2)")

    assert request == {
        "selection": "beta",
        "document": "alpha beta gamma",
        "file": "paper.pdf (page 2)",
        "byte_start": 6,
        "byte_end": 10,
    }


def test_build_request_uses_utf8_byte_offsets() -> None:
    request = build_request("é", "aé b", "x")

    assert request["byte_start"] == 1
    assert request["byte_end"] == 3


def test_build_request_omits_offsets_when_selection_is_missing() -> None:
    request = build_request("missing", "alpha", "x")

    assert "byte_start" not in request
    assert "byte_end" not in request


def test_build_request_strips_trailing_newline() -> None:
    request = build_request("alpha\n", "alpha", "x")

    assert request["selection"] == "alpha"
    assert request["byte_start"] == 0
    assert request["byte_end"] == 5


def test_build_request_rejects_empty_selection() -> None:
    with pytest.raises(RequestError, match="no text selected"):
        build_request("  \n", "alpha", "x")


def test_build_request_includes_question() -> None:
    request = build_request("alpha", "alpha", "x", question=" Why? ")

    assert request["question"] == "Why?"


def test_build_request_ignores_blank_question() -> None:
    request = build_request("alpha", "alpha", "x", question="  ")

    assert "question" not in request


def test_build_document_request_has_no_selection() -> None:
    request = build_document_request("alpha beta", "paper.pdf (page 2)")

    assert request == {
        "document": "alpha beta",
        "file": "paper.pdf (page 2)",
    }


def test_build_document_request_rejects_an_empty_page() -> None:
    with pytest.raises(RequestError, match="page is empty"):
        build_document_request("  \n", "x")
