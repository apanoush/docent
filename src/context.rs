//! Builds the document context sent alongside the selection.

use crate::config::ContextMode;
use crate::request::Request;

/// Marker inserted before the selection inside the context.
pub const SELECTION_START: &str = "[[[ SELECTION START ]]]";
/// Marker inserted after the selection inside the context.
pub const SELECTION_END: &str = "[[[ SELECTION END ]]]";
/// Marker used when earlier document content is omitted.
pub const TRUNCATED_START: &str = "[[[ TRUNCATED: earlier part of the document omitted ]]]";
/// Marker used when later document content is omitted.
pub const TRUNCATED_END: &str = "[[[ TRUNCATED: later part of the document omitted ]]]";

/// Context text plus metadata for `--print-prompt` and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextResult {
    /// Document context, with selection and truncation markers.
    pub text: String,
    /// Whether any document content was omitted.
    pub truncated: bool,
    /// Character count of `text`.
    pub chars: usize,
}

impl ContextResult {
    fn empty() -> Self {
        Self {
            text: String::new(),
            truncated: false,
            chars: 0,
        }
    }
}

/// Context window settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextOptions {
    /// How much of the document to include.
    pub mode: ContextMode,
    /// Soft character budget for `auto` mode.
    pub max_chars: usize,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            mode: ContextMode::Auto,
            max_chars: 32_000,
        }
    }
}

/// Builds context according to the selected mode.
pub fn build(request: &Request, options: ContextOptions) -> ContextResult {
    if options.mode == ContextMode::None {
        return ContextResult::empty();
    }

    let Some(document) = request.document.as_deref().filter(|text| !text.is_empty()) else {
        return ContextResult::empty();
    };
    let range = request.selection_range();
    let document_chars = document.chars().count();

    let use_full_document = match options.mode {
        ContextMode::Full => true,
        ContextMode::Auto => document_chars <= options.max_chars,
        ContextMode::None => false,
    };
    if use_full_document {
        return assemble(document, range, 0, document.len(), false, false);
    }

    match range {
        Some((selection_start, selection_end)) => {
            let selection_chars = document[selection_start..selection_end].chars().count();
            let marker_overhead =
                SELECTION_START.chars().count() + SELECTION_END.chars().count() + 2;
            let budget = options
                .max_chars
                .saturating_sub(selection_chars + marker_overhead);
            let before_budget = budget / 2;
            let after_budget = budget - before_budget;

            let prefix_chars = document[..selection_start].chars().count();
            let before_chars = before_budget.min(prefix_chars);
            let before_start_chars = prefix_chars - before_chars;
            let window_start =
                align_line_start(document, byte_index_at_char(document, before_start_chars));

            let suffix = &document[selection_end..];
            let suffix_chars = suffix.chars().count();
            let after_chars = after_budget.min(suffix_chars);
            let window_end = align_line_end(
                document,
                selection_end + byte_index_at_char(suffix, after_chars),
            );

            assemble(
                document,
                Some((selection_start, selection_end)),
                window_start,
                window_end,
                window_start > 0,
                window_end < document.len(),
            )
        }
        None => {
            let window_end =
                align_line_end(document, byte_index_at_char(document, options.max_chars));
            assemble(
                document,
                None,
                0,
                window_end,
                false,
                window_end < document.len(),
            )
        }
    }
}

fn assemble(
    document: &str,
    range: Option<(usize, usize)>,
    start: usize,
    end: usize,
    truncate_before: bool,
    truncate_after: bool,
) -> ContextResult {
    let mut text = String::new();

    if truncate_before {
        text.push_str(TRUNCATED_START);
        text.push('\n');
    }

    if let Some((selection_start, selection_end)) = range {
        push_section(&mut text, &document[start..selection_start]);
        text.push_str(SELECTION_START);
        text.push('\n');
        text.push_str(&document[selection_start..selection_end]);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(SELECTION_END);
        text.push('\n');
        push_section(&mut text, &document[selection_end..end]);
    } else {
        push_section(&mut text, &document[start..end]);
    }

    if truncate_after {
        text.push_str(TRUNCATED_END);
        text.push('\n');
    }

    while text.ends_with('\n') {
        text.pop();
    }

    ContextResult {
        chars: text.chars().count(),
        truncated: truncate_before || truncate_after,
        text,
    }
}

fn push_section(text: &mut String, section: &str) {
    if section.is_empty() {
        return;
    }
    text.push_str(section);
    if !section.ends_with('\n') {
        text.push('\n');
    }
}

fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(index, _)| index)
}

fn align_line_start(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |index| index + 1)
}

fn align_line_end(text: &str, byte: usize) -> usize {
    text[byte..]
        .find('\n')
        .map_or(text.len(), |index| byte + index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        document: &str,
        selection: &str,
        byte_start: Option<usize>,
        byte_end: Option<usize>,
    ) -> Request {
        Request {
            selection: selection.to_string(),
            document: Some(document.to_string()),
            byte_start,
            byte_end,
            ..Request::default()
        }
    }

    #[test]
    fn none_mode_returns_empty_context() {
        let request = request("alpha beta", "beta", Some(6), Some(10));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::None,
                max_chars: 100,
            },
        );
        assert_eq!(context, ContextResult::empty());
    }

    #[test]
    fn missing_document_returns_empty_context() {
        let request = Request {
            selection: "beta".to_string(),
            ..Request::default()
        };
        let context = build(&request, ContextOptions::default());
        assert!(context.text.is_empty());
        assert!(!context.truncated);
    }

    #[test]
    fn full_mode_keeps_everything_and_marks_the_selection() {
        let document = "alpha beta gamma";
        let request = request(document, "beta", Some(6), Some(10));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Full,
                max_chars: 5,
            },
        );
        assert!(context.text.contains("alpha"));
        assert!(context.text.contains(SELECTION_START));
        assert!(context.text.contains("beta"));
        assert!(context.text.contains(SELECTION_END));
        assert!(context.text.contains("gamma"));
        assert!(!context.truncated);
        assert_eq!(context.chars, context.text.chars().count());
    }

    #[test]
    fn auto_mode_uses_full_document_when_it_fits() {
        let document = "alpha beta gamma";
        let request = request(document, "beta", Some(6), Some(10));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 100,
            },
        );
        assert!(context.text.contains("alpha"));
        assert!(context.text.contains("beta"));
        assert!(context.text.contains("gamma"));
        assert!(!context.truncated);
    }

    #[test]
    fn auto_mode_windows_around_the_selection() {
        let document = "first line\nsecond line\nthird line\nfourth line\nfifth line";
        let selection = "third";
        let start = document.find(selection).unwrap();
        let request = request(
            document,
            selection,
            Some(start),
            Some(start + selection.len()),
        );
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 30,
            },
        );

        assert!(context.text.contains(selection));
        assert!(context.text.contains(SELECTION_START));
        assert!(context.text.contains(SELECTION_END));
        assert!(context.truncated);
        assert!(context.text.contains(TRUNCATED_START));
        assert!(context.text.contains(TRUNCATED_END));
        assert_eq!(context.chars, context.text.chars().count());
    }

    #[test]
    fn auto_mode_without_selection_keeps_the_beginning() {
        let document = "first line\nsecond line\nthird line\nfourth line";
        let request = request(document, "", None, None);
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 15,
            },
        );

        assert!(context.text.starts_with("first line"));
        assert!(context.text.contains(TRUNCATED_END));
        assert!(!context.text.contains(TRUNCATED_START));
        assert!(context.truncated);
    }

    #[test]
    fn selection_at_the_start_has_no_leading_truncation_marker() {
        let document = "alpha beta gamma delta";
        let request = request(document, "alpha", Some(0), Some(5));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 20,
            },
        );

        assert!(context.text.starts_with(SELECTION_START));
        assert!(!context.text.contains(TRUNCATED_START));
    }

    #[test]
    fn selection_at_the_end_has_no_trailing_truncation_marker() {
        let document = "alpha beta gamma delta";
        let end = document.len();
        let request = request(document, "delta", Some(end - 5), Some(end));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 20,
            },
        );

        assert!(context.text.ends_with(SELECTION_END));
        assert!(!context.text.contains(TRUNCATED_END));
    }

    #[test]
    fn utf8_boundaries_are_safe() {
        let document = "αβγ\nδεζ\nηθι";
        let selection = "δεζ";
        let start = document.find(selection).unwrap();
        let request = request(
            document,
            selection,
            Some(start),
            Some(start + selection.len()),
        );
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 10,
            },
        );

        assert!(context.text.contains(selection));
        assert_eq!(context.chars, context.text.chars().count());
    }

    #[test]
    fn invalid_offsets_fall_back_to_search() {
        let document = "alpha beta gamma";
        let request = request(document, "beta", Some(0), Some(4));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Full,
                max_chars: 1000,
            },
        );

        assert!(context.text.contains(SELECTION_START));
        assert!(context.text.contains("beta"));
        assert!(
            !context
                .text
                .contains("alpha\n[[[ SELECTION START ]]]\nbeta")
        );
    }

    #[test]
    fn long_selection_is_still_included() {
        let document = "abcdefghij";
        let request = request(document, "cdefgh", Some(2), Some(8));
        let context = build(
            &request,
            ContextOptions {
                mode: ContextMode::Auto,
                max_chars: 5,
            },
        );

        assert!(context.text.contains("cdefgh"));
        assert!(context.text.contains(SELECTION_START));
        assert!(context.text.contains(SELECTION_END));
    }
}
