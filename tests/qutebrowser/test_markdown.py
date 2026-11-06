"""Tests for the answer-tab Markdown renderer."""

from __future__ import annotations

import pytest
from docent.markdown import render


def test_blank_input_returns_an_empty_fragment() -> None:
    assert render("") == ""
    assert render(" \n\t\n") == ""


def test_paragraphs_and_soft_wraps() -> None:
    assert render("one\ntwo\n\nthree") == "<p>one two</p>\n<p>three</p>"


def test_headings() -> None:
    assert render("# One") == "<h1>One</h1>"
    assert render("###### Six") == "<h6>Six</h6>"
    assert render("####### seven") == "<p>####### seven</p>"


def test_inline_formatting() -> None:
    result = render("**bold** *ital* ~~gone~~ `code`")

    assert result == (
        "<p><strong>bold</strong> <em>ital</em> <del>gone</del> <code>code</code></p>"
    )


def test_nested_inline_formatting() -> None:
    assert render("**bold with `code`**") == (
        "<p><strong>bold with <code>code</code></strong></p>"
    )


def test_links_open_in_a_new_tab() -> None:
    result = render("[docs](https://example.com/a?b=1&c=2)")

    assert result == (
        '<p><a href="https://example.com/a?b=1&amp;c=2" target="_blank" '
        'rel="noopener noreferrer">docs</a></p>'
    )


def test_link_labels_are_rendered_inline() -> None:
    result = render("[**bold**](https://example.com)")

    assert "<strong>bold</strong>" in result
    assert '<a href="https://example.com"' in result


def test_mailto_links_are_allowed() -> None:
    result = render("[mail](mailto:a@example.com)")

    assert 'href="mailto:a@example.com"' in result


@pytest.mark.parametrize(
    "url",
    [
        "javascript:alert(1)",
        "data:text/html,x",
        "/relative/path",
        "//example.com/x",
    ],
)
def test_unsafe_links_render_as_text(url: str) -> None:
    result = render(f"[x]({url})")

    assert "<a " not in result
    assert "x (" in result


def test_malformed_urls_render_as_text() -> None:
    result = render("[x](http://[::1)")

    assert "<a " not in result
    assert "x (" in result


def test_link_urls_cannot_break_out_of_the_attribute() -> None:
    result = render('[x](https://example.com/"onmouseover="alert)')

    assert 'onmouseover="' not in result
    assert "&quot;onmouseover=&quot;" in result


def test_images_render_as_alt_text() -> None:
    assert render("![diagram](https://example.com/x.png)") == "<p>diagram</p>"


def test_raw_html_is_escaped() -> None:
    result = render('<script>alert("x")</script>')

    assert "<script>" not in result
    assert "&lt;script&gt;" in result


def test_code_is_escaped() -> None:
    assert render("`<b>`") == "<p><code>&lt;b&gt;</code></p>"


def test_fenced_code_block() -> None:
    result = render("```python\nprint('<x>')\n```")

    assert result == (
        "<pre><code class=\"language-python\">print('&lt;x&gt;')</code></pre>"
    )


def test_unclosed_fence_renders_to_the_end() -> None:
    assert render("```\nx") == "<pre><code>x</code></pre>"


def test_unordered_list() -> None:
    assert render("- one\n- two") == "<ul><li>one</li><li>two</li></ul>"


def test_ordered_list() -> None:
    assert render("1. one\n2. two") == "<ol><li>one</li><li>two</li></ol>"


def test_blank_line_ends_a_list() -> None:
    assert render("- one\n\ntext") == "<ul><li>one</li></ul>\n<p>text</p>"


def test_blockquote_renders_inner_blocks() -> None:
    result = render("> ## Title\n> text")

    assert result == "<blockquote><h2>Title</h2>\n<p>text</p></blockquote>"


def test_text_after_a_blockquote_is_a_paragraph() -> None:
    assert (
        render("> quote\nafter")
        == "<blockquote><p>quote</p></blockquote>\n<p>after</p>"
    )


def test_horizontal_rules() -> None:
    assert render("---") == "<hr>"
    assert render("* * *") == "<hr>"


def test_table_with_alignment() -> None:
    text = "| Name | Count |\n|:-----|------:|\n| a | 1 |\n| b | 2 |"

    result = render(text)

    assert '<th style="text-align: left">Name</th>' in result
    assert '<th style="text-align: right">Count</th>' in result
    assert '<td style="text-align: left">a</td>' in result
    assert '<td style="text-align: right">1</td>' in result
    assert "<tbody><tr>" in result


def test_table_without_outer_pipes() -> None:
    result = render("a | b\n--- | ---\n1 | 2")

    assert "<table>" in result
    assert "<th>a</th>" in result
    assert "<td>1</td>" in result


def test_table_cells_render_inline_markdown() -> None:
    result = render("| **a** | `b` |\n|---|---|")

    assert "<strong>a</strong>" in result
    assert "<code>b</code>" in result


def test_table_cell_escaped_pipe() -> None:
    result = render("| a | b |\n|---|---|\n| x \\| y | z |")

    assert "x | y" in result


def test_pipe_text_without_a_separator_stays_a_paragraph() -> None:
    assert render("a | b") == "<p>a | b</p>"
    assert render("a | b\n---") == "<p>a | b</p>\n<hr>"


def test_table_stops_at_a_blank_line() -> None:
    result = render("| a | b |\n|---|---|\n| 1 | 2 |\n\nafter")

    assert "<td>1</td>" in result
    assert "<p>after</p>" in result
    assert "after</td>" not in result


def test_table_column_counts_must_match() -> None:
    result = render("| a | b | c |\n|---|---|")

    assert "<table>" not in result


def test_table_needs_at_least_two_columns() -> None:
    result = render("| a |\n|---|")

    assert "<table>" not in result


def test_invalid_separator_cells_are_not_a_table() -> None:
    result = render("| a | b |\n|---| nope |")

    assert "<table>" not in result


def test_centered_table_column() -> None:
    result = render("| a | b |\n|:---:|:---:|\n| 1 | 2 |")

    assert '<th style="text-align: center">a</th>' in result
    assert '<td style="text-align: center">1</td>' in result
