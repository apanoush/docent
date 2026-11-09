//! JSON request received on stdin from the Neovim plugin.

use serde::{Deserialize, Serialize};

/// A request to explain, ask about, or translate a selected passage.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Selected passage. May be empty for whole-document profiles.
    #[serde(default)]
    pub selection: String,
    /// Full document used as context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// Byte offset where the selection starts in `document`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_start: Option<usize>,
    /// Byte offset where the selection ends in `document`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_end: Option<usize>,
    /// File name shown to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Follow-up question, when the profile needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

impl Request {
    /// Returns the byte range of the selection inside `document`.
    ///
    /// Valid offsets are trusted only when they match the selection exactly.
    /// Otherwise the first matching occurrence is used, tolerating whitespace
    /// and line-break hyphenation. An empty selection has no range.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        if self.selection.is_empty() {
            return None;
        }
        let document = self.document.as_deref()?;

        if let (Some(start), Some(end)) = (self.byte_start, self.byte_end) {
            if start < end
                && end <= document.len()
                && document.is_char_boundary(start)
                && document.is_char_boundary(end)
                && document.get(start..end) == Some(self.selection.as_str())
            {
                return Some((start, end));
            }
        }

        crate::input::find_text(document, &self.selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_offsets_when_they_match() {
        let request = Request {
            selection: "beta".to_string(),
            document: Some("alpha beta gamma".to_string()),
            byte_start: Some(6),
            byte_end: Some(10),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), Some((6, 10)));
    }

    #[test]
    fn falls_back_to_search_when_offsets_mismatch() {
        let request = Request {
            selection: "beta".to_string(),
            document: Some("alpha beta gamma".to_string()),
            byte_start: Some(0),
            byte_end: Some(4),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), Some((6, 10)));
    }

    #[test]
    fn falls_back_to_search_when_offsets_are_out_of_bounds() {
        let request = Request {
            selection: "beta".to_string(),
            document: Some("alpha beta gamma".to_string()),
            byte_start: Some(100),
            byte_end: Some(200),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), Some((6, 10)));
    }

    #[test]
    fn falls_back_to_tolerant_search_for_wrapped_text() {
        let request = Request {
            selection: "beta gamma".to_string(),
            document: Some("alpha beta\ngamma".to_string()),
            byte_start: Some(0),
            byte_end: Some(4),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), Some((6, 16)));
    }

    #[test]
    fn empty_selection_has_no_range() {
        let request = Request {
            selection: String::new(),
            document: Some("alpha".to_string()),
            byte_start: Some(0),
            byte_end: Some(5),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), None);
    }

    #[test]
    fn missing_document_has_no_range() {
        let request = Request {
            selection: "alpha".to_string(),
            document: None,
            ..Request::default()
        };
        assert_eq!(request.selection_range(), None);
    }

    #[test]
    fn unicode_offsets_are_checked_at_char_boundaries() {
        let document = "aé b".to_string();
        let request = Request {
            selection: "é".to_string(),
            document: Some(document),
            byte_start: Some(1),
            byte_end: Some(3),
            ..Request::default()
        };
        assert_eq!(request.selection_range(), Some((1, 3)));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = serde_json::from_str::<Request>(r#"{"selection":"x","extra":1}"#).unwrap_err();
        assert!(error.to_string().contains("extra"));
    }
}
