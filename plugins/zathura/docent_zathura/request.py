"""Build the JSON request docent reads on stdin."""

from __future__ import annotations

from typing import Any

__all__ = ["RequestError", "build_document_request", "build_request"]


class RequestError(Exception):
    """The captured selection cannot form a request."""


def build_document_request(document: str, file_label: str) -> dict[str, Any]:
    """Return a request with no selection, for whole-document profiles."""
    if not document.strip():
        raise RequestError("the extracted page is empty")

    return {
        "document": document,
        "file": file_label,
    }


def build_request(
    selection: str,
    document: str,
    file_label: str,
    question: str | None = None,
) -> dict[str, Any]:
    """Return a request containing only fields the CLI knows about.

    Offsets are added only when the selection occurs exactly in the extracted
    text. The CLI falls back to its whitespace-tolerant search otherwise.
    """
    selection = selection.rstrip("\r\n")
    if not selection.strip():
        raise RequestError("no text selected; select text with the mouse first")

    request: dict[str, Any] = {
        "selection": selection,
        "document": document,
        "file": file_label,
    }

    encoded_document = document.encode("utf-8")
    encoded_selection = selection.encode("utf-8")
    start = encoded_document.find(encoded_selection)
    if start != -1:
        request["byte_start"] = start
        request["byte_end"] = start + len(encoded_selection)

    if question is not None and question.strip():
        request["question"] = question.strip()

    return request
