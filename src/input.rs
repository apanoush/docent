//! Terminal input adapters: files, stdin, line ranges, and prompts.

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::DocentError;
use crate::pdf;
use crate::profile::Requirement;
use crate::request::Request;

const STDIN_NAME: &str = "(stdin)";

/// A 1-based, inclusive line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    /// First line.
    pub start: usize,
    /// Last line.
    pub end: usize,
}

impl LineRange {
    /// Builds a single-line range.
    pub fn single(line: usize) -> Self {
        Self {
            start: line,
            end: line,
        }
    }
}

impl FromStr for LineRange {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (start, end) = parse_number_range(value, "line")?;
        Ok(Self { start, end })
    }
}

impl std::fmt::Display for LineRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}:{}", self.start, self.end)
        }
    }
}

/// A 1-based, inclusive page range for PDF documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    /// First page.
    pub start: usize,
    /// Last page.
    pub end: usize,
}

impl PageRange {
    /// Builds a single-page range.
    pub fn single(page: usize) -> Self {
        Self {
            start: page,
            end: page,
        }
    }
}

impl FromStr for PageRange {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (start, end) = parse_number_range(value, "page")?;
        Ok(Self { start, end })
    }
}

impl std::fmt::Display for PageRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}:{}", self.start, self.end)
        }
    }
}

fn parse_number_range(value: &str, kind: &str) -> Result<(usize, usize), String> {
    let value = value.trim();
    let (start_text, end_text) = match value.split_once(':') {
        Some((start, end)) => (start, Some(end)),
        None => (value, None),
    };

    let start = parse_number(start_text, kind)?;
    let end = match end_text {
        Some(end) => parse_number(end, kind)?,
        None => start,
    };

    if start > end {
        return Err(format!("{kind} range start must not exceed end"));
    }

    Ok((start, end))
}

fn parse_number(value: &str, kind: &str) -> Result<usize, String> {
    let number = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("'{value}' is not a {kind} number"))?;
    if number == 0 {
        return Err(format!("{kind} numbers start at 1"));
    }
    Ok(number)
}

/// How the terminal selected a passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionArg {
    /// A line range.
    Lines(LineRange),
    /// Pasted selection text.
    Text(String),
    /// A phrase to locate in the document.
    Phrase(String),
}

/// A document read from a file or stdin, with a line index for selection.
#[derive(Debug, Clone)]
pub struct Document {
    /// Display name used in prompts and error messages.
    pub name: String,
    /// Normalized document text.
    pub text: String,
    lines: Vec<(usize, usize)>,
}

impl Document {
    /// Builds a document and normalizes CRLF line endings to LF.
    pub fn from_text(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into().replace("\r\n", "\n");
        let mut lines = Vec::new();
        let mut start = 0;

        for part in text.split_inclusive('\n') {
            let content = part.strip_suffix('\n').unwrap_or(part);
            lines.push((start, start + content.len()));
            start += part.len();
        }

        Self {
            name: name.into(),
            text,
            lines,
        }
    }

    /// Number of lines in the document.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Finds the first occurrence of `phrase`.
    ///
    /// An exact match wins. Otherwise whitespace runs are collapsed and
    /// hyphenated line breaks are joined, so a passage copied out of a PDF can
    /// still be located in the extracted text.
    pub fn find(&self, phrase: &str) -> Option<(usize, usize)> {
        find_text(&self.text, phrase)
    }

    /// Counts occurrences of `phrase`, ignoring exact whitespace and line-break
    /// hyphenation.
    pub fn occurrence_count(&self, phrase: &str) -> usize {
        count_text(&self.text, phrase)
    }

    /// Extracts a line range and its byte offsets.
    pub fn select_lines(&self, range: LineRange) -> Result<(String, usize, usize), DocentError> {
        if self.lines.is_empty() {
            return Err(DocentError::Input(format!("{} is empty", self.name)));
        }
        if range.start == 0 || range.end == 0 {
            return Err(DocentError::Input("line numbers start at 1".to_string()));
        }
        if range.start > range.end {
            return Err(DocentError::Input(format!("invalid line range {range}")));
        }
        if range.end > self.lines.len() {
            return Err(DocentError::Input(format!(
                "line range {range} is out of bounds; {} has {} lines",
                self.name,
                self.lines.len()
            )));
        }

        let start = self.lines[range.start - 1].0;
        let end = self.lines[range.end - 1].1;
        Ok((self.text[start..end].to_string(), start, end))
    }
}

/// Finds the first occurrence of `needle` in `haystack`.
///
/// An exact match wins. Otherwise whitespace runs are collapsed and hyphenated
/// line breaks are joined before searching. Offsets in the result always index
/// `haystack`.
pub fn find_text(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    if let Some(start) = haystack.find(needle) {
        return Some((start, start + needle.len()));
    }

    let (normalized, offsets) = normalize_for_search(haystack);
    let needle = normalize_for_search(needle).0;
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let start = normalized.find(needle)?;
    let end = start + needle.len();
    map_match(&normalized, &offsets, start, end)
}

/// Counts occurrences of `needle`, ignoring exact whitespace and line-break
/// hyphenation.
pub fn count_text(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let (normalized, _) = normalize_for_search(haystack);
    let needle = normalize_for_search(needle).0;
    let needle = needle.trim();
    if needle.is_empty() {
        return 0;
    }
    normalized.match_indices(needle).count()
}

/// Collapses whitespace runs to one space, drops soft hyphens, and joins
/// hyphenated line breaks. Returns the normalized text and the byte offset in
/// the original text for every normalized character.
fn normalize_for_search(text: &str) -> (String, Vec<usize>) {
    let mut normalized = String::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len());
    let mut characters = text.char_indices().peekable();

    while let Some((offset, character)) = characters.next() {
        if character == '\u{00ad}' {
            continue;
        }
        if is_hyphen(character) && characters.peek().is_some_and(|(_, next)| *next == '\n') {
            characters.next();
            continue;
        }
        if character.is_whitespace() {
            if !normalized.ends_with(' ') {
                normalized.push(' ');
                offsets.push(offset);
            }
            continue;
        }
        normalized.push(character);
        offsets.push(offset);
    }

    while normalized.ends_with(' ') {
        normalized.pop();
        offsets.pop();
    }
    (normalized, offsets)
}

fn is_hyphen(character: char) -> bool {
    matches!(character, '-' | '\u{2010}')
}

/// Maps a byte range in the normalized text back to the original text.
fn map_match(
    normalized: &str,
    offsets: &[usize],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let start_char = normalized[..start].chars().count();
    let end_char = normalized[..end].chars().count();
    let start_offset = *offsets.get(start_char)?;
    let last = end_char.checked_sub(1)?;
    let last_offset = *offsets.get(last)?;
    let last_byte = normalized.char_indices().nth(last)?.0;
    let last_character = normalized[last_byte..].chars().next()?;
    Some((start_offset, last_offset + last_character.len_utf8()))
}

/// Reads a line of input for missing terminal arguments.
pub trait Prompter {
    /// Whether prompts are available.
    fn is_interactive(&self) -> bool;

    /// Prints a label to stderr and reads one trimmed response.
    fn prompt(&mut self, label: &str) -> Result<Option<String>, DocentError>;
}

/// Prompts on the process terminal.
pub struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn is_interactive(&self) -> bool {
        true
    }

    fn prompt(&mut self, label: &str) -> Result<Option<String>, DocentError> {
        eprint!("{label}");
        std::io::stderr()
            .flush()
            .map_err(DocentError::WriteStderr)?;

        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        let mut line = String::new();
        let read = lock.read_line(&mut line).map_err(DocentError::ReadStdin)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }
}

/// Placeholder used when input is not interactive.
pub struct NoPrompter;

impl Prompter for NoPrompter {
    fn is_interactive(&self) -> bool {
        false
    }

    fn prompt(&mut self, _label: &str) -> Result<Option<String>, DocentError> {
        Ok(None)
    }
}

/// Inputs needed to build a terminal request.
pub struct RequestOptions<'a> {
    /// Document path, or `-` for stdin.
    pub file: Option<&'a Path>,
    /// Page range for PDF documents.
    pub pages: Option<PageRange>,
    /// Clamp a PDF page range to the last page instead of rejecting it.
    pub clamp_pages: bool,
    /// Selection supplied on the command line.
    pub selection: Option<SelectionArg>,
    /// Question supplied on the command line.
    pub question: Option<String>,
    /// Requirements declared by the profile.
    pub requires: &'a [Requirement],
}

/// Builds a request from files, stdin, command-line selections, and prompts.
pub fn resolve_request(
    options: RequestOptions<'_>,
    prompter: &mut dyn Prompter,
    stdin_is_terminal: bool,
    stdin: &mut dyn Read,
) -> Result<Request, DocentError> {
    let requires_selection = options.requires.contains(&Requirement::Selection);
    let requires_question = options.requires.contains(&Requirement::Question);
    let requires_context = options.requires.contains(&Requirement::Context);

    let mut file = options.file.map(Path::to_path_buf);
    let pages = options.pages;
    let clamp_pages = options.clamp_pages;
    let mut selection = options.selection;
    let mut question = options.question;

    let should_prompt_for_file = file.is_none()
        && stdin_is_terminal
        && (requires_context || !matches!(selection, Some(SelectionArg::Text(_))));
    if should_prompt_for_file {
        if !prompter.is_interactive() {
            return Err(DocentError::Input(
                "no file provided; pass FILE or pipe a document on stdin".to_string(),
            ));
        }
        let value = prompt_required(prompter, "File (or - for stdin): ", "no file provided")?;
        if value == "-" {
            return Err(DocentError::StdinIsTerminal);
        }
        file = Some(PathBuf::from(value));
    }

    let document = match &file {
        Some(path) => Some(read_document(
            path,
            pages,
            clamp_pages,
            stdin_is_terminal,
            stdin,
        )?),
        None if !stdin_is_terminal => Some(read_stdin(pages, clamp_pages, stdin)?),
        None => None,
    };

    if requires_context
        && document
            .as_ref()
            .is_none_or(|document| document.text.trim().is_empty())
    {
        return Err(DocentError::Input(
            "profile requires document context but none was provided".to_string(),
        ));
    }

    if selection.is_none() && requires_selection {
        if !prompter.is_interactive() {
            return Err(DocentError::Input(
                "no selection provided; use --lines, --line, --phrase, or --text".to_string(),
            ));
        }
        let value = prompt_required(
            prompter,
            "Lines (e.g. 12:20) or text: ",
            "no selection provided",
        )?;
        selection = Some(parse_prompt_selection(&value)?);
    }

    let (selection_text, byte_start, byte_end) = match selection {
        None => (String::new(), None, None),
        Some(SelectionArg::Text(text)) => match document.as_ref().and_then(|document| {
            document
                .find(&text)
                .map(|range| (document.text[range.0..range.1].to_string(), range))
        }) {
            Some((selection, (start, end))) => (selection, Some(start), Some(end)),
            None => (text, None, None),
        },
        Some(SelectionArg::Phrase(phrase)) => {
            let Some(document) = document.as_ref() else {
                return Err(DocentError::Input(
                    "--phrase requires a document; pass FILE or pipe one on stdin".to_string(),
                ));
            };
            let (start, end) = document.find(&phrase).ok_or_else(|| {
                DocentError::Input(format!("phrase not found in {}: {phrase}", document.name))
            })?;
            let count = document.occurrence_count(&phrase);
            if count > 1 {
                log::warn!(
                    "phrase occurs {count} times in {}; using the first occurrence",
                    document.name
                );
            }
            (
                document.text[start..end].to_string(),
                Some(start),
                Some(end),
            )
        }
        Some(SelectionArg::Lines(range)) => {
            let Some(document) = document.as_ref() else {
                return Err(DocentError::Input(
                    "line selection requires a document; pass FILE or pipe one on stdin"
                        .to_string(),
                ));
            };
            let (text, start, end) = document.select_lines(range)?;
            (text, Some(start), Some(end))
        }
    };

    if requires_selection && selection_text.trim().is_empty() {
        return Err(DocentError::Input(
            "the selected passage is empty".to_string(),
        ));
    }

    let missing_question = question
        .as_deref()
        .is_none_or(|value| value.trim().is_empty());
    if requires_question && missing_question {
        if !prompter.is_interactive() {
            return Err(DocentError::Input(
                "this profile requires a question; pass --question".to_string(),
            ));
        }
        let value = prompt_required(prompter, "Question: ", "no question provided")?;
        question = Some(value);
    }
    if question
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        question = None;
    }

    let file_name = document.as_ref().map(|document| document.name.clone());

    Ok(Request {
        selection: selection_text,
        document: document.map(|document| document.text),
        byte_start,
        byte_end,
        file: file_name,
        question,
    })
}

fn prompt_required(
    prompter: &mut dyn Prompter,
    label: &str,
    error: &str,
) -> Result<String, DocentError> {
    let value = prompter
        .prompt(label)?
        .ok_or_else(|| DocentError::Input(error.to_string()))?;
    let value = value.trim();
    if value.is_empty() {
        return Err(DocentError::Input(error.to_string()));
    }
    Ok(value.to_string())
}

fn parse_prompt_selection(value: &str) -> Result<SelectionArg, DocentError> {
    let looks_like_lines = value
        .chars()
        .all(|character| character.is_ascii_digit() || character == ':')
        && value.chars().any(|character| character.is_ascii_digit());

    if looks_like_lines {
        value
            .parse::<LineRange>()
            .map(SelectionArg::Lines)
            .map_err(DocentError::Input)
    } else {
        Ok(SelectionArg::Text(value.to_string()))
    }
}

fn read_document(
    path: &Path,
    pages: Option<PageRange>,
    clamp_pages: bool,
    stdin_is_terminal: bool,
    stdin: &mut dyn Read,
) -> Result<Document, DocentError> {
    if path == Path::new("-") {
        if stdin_is_terminal {
            return Err(DocentError::StdinIsTerminal);
        }
        return read_stdin(pages, clamp_pages, stdin);
    }

    let bytes = fs::read(path).map_err(|source| DocentError::ReadDocument {
        path: path.to_path_buf(),
        source,
    })?;
    document_from_bytes(&path.display().to_string(), bytes, pages, clamp_pages)
}

fn read_stdin(
    pages: Option<PageRange>,
    clamp_pages: bool,
    stdin: &mut dyn Read,
) -> Result<Document, DocentError> {
    let mut bytes = Vec::new();
    stdin
        .read_to_end(&mut bytes)
        .map_err(DocentError::ReadStdin)?;
    document_from_bytes(STDIN_NAME, bytes, pages, clamp_pages)
}

/// Builds a document from raw file or stdin bytes, decoding PDFs when the
/// content starts with the PDF header.
fn document_from_bytes(
    name: &str,
    bytes: Vec<u8>,
    pages: Option<PageRange>,
    clamp_pages: bool,
) -> Result<Document, DocentError> {
    if pdf::is_pdf(&bytes) {
        let extracted = pdf::extract(&bytes, pages).map_err(|message| {
            DocentError::Input(format!("failed to read PDF {name}: {message}"))
        })?;
        if let Some(range) = pages
            && !clamp_pages
            && range.end > extracted.pages
        {
            return Err(DocentError::Input(format!(
                "failed to read PDF {name}: page range {range} is out of bounds; the PDF has {} pages",
                extracted.pages
            )));
        }
        if extracted.text.trim().is_empty() {
            return Err(DocentError::Input(format!(
                "{name} has no extractable text; the PDF may be scanned or image-only"
            )));
        }
        return Ok(Document::from_text(name, extracted.text));
    }

    if pages.is_some() {
        return Err(DocentError::Input(format!(
            "--pages requires a PDF; {name} is not a PDF"
        )));
    }

    let text = String::from_utf8(bytes).map_err(|_| {
        DocentError::Input(format!(
            "{name} is not UTF-8 text; supported inputs are plain text and PDF"
        ))
    })?;
    Ok(Document::from_text(name, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Cursor;

    struct ScriptedPrompter {
        answers: VecDeque<String>,
    }

    impl ScriptedPrompter {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|answer| answer.to_string()).collect(),
            }
        }
    }

    impl Prompter for ScriptedPrompter {
        fn is_interactive(&self) -> bool {
            true
        }

        fn prompt(&mut self, _label: &str) -> Result<Option<String>, DocentError> {
            Ok(self.answers.pop_front())
        }
    }

    fn temp_file(name: &str, contents: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        fs::write(&path, contents).unwrap();
        (directory, path)
    }

    fn temp_byte_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        fs::write(&path, bytes).unwrap();
        (directory, path)
    }

    fn options<'a>(
        file: Option<&'a Path>,
        selection: Option<SelectionArg>,
        question: Option<&str>,
        requires: &'a [Requirement],
    ) -> RequestOptions<'a> {
        RequestOptions {
            file,
            pages: None,
            clamp_pages: false,
            selection,
            question: question.map(str::to_string),
            requires,
        }
    }

    #[test]
    fn parses_line_ranges() {
        assert_eq!(
            "12".parse::<LineRange>().unwrap(),
            LineRange { start: 12, end: 12 }
        );
        assert_eq!(
            "12:20".parse::<LineRange>().unwrap(),
            LineRange { start: 12, end: 20 }
        );
        assert_eq!(
            " 12 : 20 ".parse::<LineRange>().unwrap(),
            LineRange { start: 12, end: 20 }
        );
        assert!("0".parse::<LineRange>().is_err());
        assert!("0:2".parse::<LineRange>().is_err());
        assert!("20:12".parse::<LineRange>().is_err());
        assert!("".parse::<LineRange>().is_err());
        assert!("12:".parse::<LineRange>().is_err());
        assert!("a".parse::<LineRange>().is_err());
    }

    #[test]
    fn displays_line_ranges() {
        assert_eq!(LineRange::single(7).to_string(), "7");
        assert_eq!(LineRange { start: 7, end: 9 }.to_string(), "7:9");
    }

    #[test]
    fn parses_page_ranges() {
        assert_eq!("5".parse::<PageRange>().unwrap(), PageRange::single(5));
        assert_eq!(
            "5:7".parse::<PageRange>().unwrap(),
            PageRange { start: 5, end: 7 }
        );
        assert_eq!(
            "0:2".parse::<PageRange>().unwrap_err(),
            "page numbers start at 1"
        );
        assert_eq!(
            "7:5".parse::<PageRange>().unwrap_err(),
            "page range start must not exceed end"
        );
        assert_eq!(
            "x".parse::<PageRange>().unwrap_err(),
            "'x' is not a page number"
        );
    }

    #[test]
    fn selects_lines_and_offsets() {
        let document = Document::from_text("notes.md", "alpha\nbeta gamma\ndelta");
        assert_eq!(document.line_count(), 3);

        let (text, start, end) = document.select_lines(LineRange::single(2)).unwrap();
        assert_eq!(text, "beta gamma");
        assert_eq!((start, end), (6, 16));

        let (text, start, end) = document
            .select_lines(LineRange { start: 2, end: 3 })
            .unwrap();
        assert_eq!(text, "beta gamma\ndelta");
        assert_eq!((start, end), (6, 22));
    }

    #[test]
    fn handles_utf8_and_crlf() {
        let document = Document::from_text("notes.md", "αβ\r\nγδ\r\n");
        assert_eq!(document.text, "αβ\nγδ\n");
        assert_eq!(document.line_count(), 2);

        let (text, start, end) = document.select_lines(LineRange::single(2)).unwrap();
        assert_eq!(text, "γδ");
        assert_eq!((start, end), (5, 9));
    }

    #[test]
    fn rejects_empty_and_out_of_bounds_documents() {
        let empty = Document::from_text("empty.md", "");
        assert_eq!(empty.line_count(), 0);
        assert!(empty.select_lines(LineRange::single(1)).is_err());

        let document = Document::from_text("notes.md", "one\ntwo");
        let error = document.select_lines(LineRange::single(3)).unwrap_err();
        assert!(error.to_string().contains("out of bounds"));
        assert!(document.select_lines(LineRange::single(0)).is_err());
    }

    #[test]
    fn finds_phrases_and_counts_occurrences() {
        let document = Document::from_text("notes.md", "alpha beta beta");
        assert_eq!(document.find("beta"), Some((6, 10)));
        assert_eq!(document.occurrence_count("beta"), 2);
        assert_eq!(document.find("missing"), None);
        assert_eq!(document.occurrence_count(""), 0);
    }

    #[test]
    fn finds_phrases_across_line_breaks() {
        let document = Document::from_text("paper.txt", "alpha beta\ngamma delta");
        assert_eq!(document.find("beta gamma"), Some((6, 16)));
        assert_eq!(document.find("beta   gamma"), Some((6, 16)));
        assert_eq!(document.find("beta gamma"), document.find("beta\ngamma"));
    }

    #[test]
    fn joins_hyphenated_line_breaks() {
        let document = Document::from_text("paper.txt", "signifi-\ncantly better");
        assert_eq!(document.find("significantly"), Some((0, 15)));
        assert_eq!(document.find("significantly better"), Some((0, 22)));
    }

    #[test]
    fn does_not_join_words_across_missing_whitespace() {
        let document = Document::from_text("notes.md", "ab");
        assert_eq!(document.find("a b"), None);
    }

    #[test]
    fn maps_offsets_through_utf8_whitespace() {
        let document = Document::from_text("notes.md", "αβ\nγδ");
        assert_eq!(document.find("β γ"), Some((2, 7)));
    }

    #[test]
    fn resolves_numbered_lines_non_interactively() {
        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\ngamma\n");
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Lines(LineRange::single(2))),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta");
        assert_eq!(request.document.as_deref(), Some("alpha\nbeta\ngamma\n"));
        assert_eq!(request.byte_start, Some(6));
        assert_eq!(request.byte_end, Some(10));
        assert_eq!(request.file.as_deref(), path.to_str());
    }

    #[test]
    fn reads_a_document_from_stdin() {
        let mut stdin = Cursor::new(b"alpha\nbeta\n".to_vec());
        let request = resolve_request(
            options(
                None,
                Some(SelectionArg::Lines(LineRange::single(2))),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta");
        assert_eq!(request.file.as_deref(), Some("(stdin)"));
    }

    #[test]
    fn accepts_text_without_a_document() {
        let mut stdin = Cursor::new(Vec::new());
        let mut prompter = ScriptedPrompter::new(&[]);
        let request = resolve_request(
            options(
                None,
                Some(SelectionArg::Text("a passage".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut prompter,
            true,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "a passage");
        assert_eq!(request.document, None);
        assert_eq!(request.byte_start, None);
    }

    #[test]
    fn finds_offsets_for_pasted_text() {
        let (_directory, path) = temp_file("notes.md", "alpha beta gamma");
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Text("beta".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta");
        assert_eq!(request.byte_start, Some(6));
        assert_eq!(request.byte_end, Some(10));
    }

    #[test]
    fn resolves_wrapped_text_to_document_offsets() {
        let (_directory, path) = temp_file("notes.md", "alpha beta\ngamma");
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Text("beta gamma".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta\ngamma");
        assert_eq!(request.byte_start, Some(6));
        assert_eq!(request.byte_end, Some(16));
    }

    #[test]
    fn reads_pdf_documents() {
        let pdf = crate::pdf::sample_pdf(&["alpha page", "beta page"]);
        let (_directory, path) = temp_byte_file("paper.pdf", &pdf);
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Text("beta page".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        let document = request.document.unwrap();
        assert!(document.contains("alpha page"), "{document:?}");
        assert!(document.contains("beta page"), "{document:?}");
        assert_eq!(request.file.as_deref(), path.to_str());
        assert!(request.byte_start.is_some());
        assert!(request.byte_end.is_some());
    }

    #[test]
    fn reads_pdf_documents_from_stdin() {
        let pdf = crate::pdf::sample_pdf(&["alpha page"]);
        let mut stdin = Cursor::new(pdf);
        let request = resolve_request(
            options(
                None,
                Some(SelectionArg::Text("alpha page".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.file.as_deref(), Some("(stdin)"));
        assert!(request.document.unwrap().contains("alpha page"));
    }

    #[test]
    fn scopes_pdf_documents_to_pages() {
        let pdf = crate::pdf::sample_pdf(&["alpha page", "beta page", "gamma page"]);
        let (_directory, path) = temp_byte_file("paper.pdf", &pdf);
        let mut stdin = Cursor::new(Vec::new());
        let mut options = options(Some(&path), None, None, &[Requirement::Context]);
        options.pages = Some(PageRange { start: 2, end: 3 });
        let request = resolve_request(options, &mut NoPrompter, false, &mut stdin).unwrap();

        let document = request.document.unwrap();
        assert!(!document.contains("alpha page"), "{document:?}");
        assert!(document.contains("beta page"), "{document:?}");
        assert!(document.contains("gamma page"), "{document:?}");
    }

    #[test]
    fn reports_a_page_range_past_the_end() {
        let pdf = crate::pdf::sample_pdf(&["alpha page"]);
        let (_directory, path) = temp_byte_file("paper.pdf", &pdf);
        let mut stdin = Cursor::new(Vec::new());
        let mut options = options(Some(&path), None, None, &[Requirement::Context]);
        options.pages = Some(PageRange::single(2));
        let error = resolve_request(options, &mut NoPrompter, false, &mut stdin).unwrap_err();

        assert!(error.to_string().contains("out of bounds"), "{error}");
        assert!(error.to_string().contains("1 pages"), "{error}");
    }

    #[test]
    fn rejects_pages_for_non_pdf_documents() {
        let (_directory, path) = temp_file("notes.md", "alpha");
        let mut stdin = Cursor::new(Vec::new());
        let mut options = options(Some(&path), None, None, &[Requirement::Context]);
        options.pages = Some(PageRange::single(1));
        let error = resolve_request(options, &mut NoPrompter, false, &mut stdin).unwrap_err();

        assert!(
            error.to_string().contains("--pages requires a PDF"),
            "{error}"
        );
    }

    #[test]
    fn rejects_pdfs_without_a_text_layer() {
        let pdf = crate::pdf::sample_pdf(&[""]);
        let (_directory, path) = temp_byte_file("scan.pdf", &pdf);
        let mut stdin = Cursor::new(Vec::new());
        let error = resolve_request(
            options(Some(&path), None, None, &[Requirement::Context]),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();

        assert!(error.to_string().contains("no extractable text"), "{error}");
    }

    #[test]
    fn rejects_binary_files_that_are_not_pdfs() {
        let (_directory, path) = temp_byte_file("blob.docx", &[0xff, 0xfe, 0x00]);
        let mut stdin = Cursor::new(Vec::new());
        let error = resolve_request(
            options(Some(&path), None, None, &[Requirement::Context]),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();

        assert!(error.to_string().contains("is not UTF-8 text"), "{error}");
        assert!(error.to_string().contains("plain text and PDF"), "{error}");
    }

    #[test]
    fn selects_a_phrase() {
        let (_directory, path) = temp_file("notes.md", "alpha beta gamma");
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Phrase("beta".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta");
        assert_eq!(request.byte_start, Some(6));
        assert_eq!(request.byte_end, Some(10));
    }

    #[test]
    fn reports_a_missing_phrase() {
        let (_directory, path) = temp_file("notes.md", "alpha beta gamma");
        let mut stdin = Cursor::new(Vec::new());
        let error = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Phrase("missing".to_string())),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();

        assert!(error.to_string().contains("phrase not found"));
    }

    #[test]
    fn prompts_for_a_file_and_selection() {
        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\n");
        let path_text = path.to_str().unwrap().to_string();
        let mut prompter = ScriptedPrompter::new(&[path_text.as_str(), "2"]);
        let mut stdin = Cursor::new(Vec::new());

        let request = resolve_request(
            options(None, None, None, &[Requirement::Selection]),
            &mut prompter,
            true,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.selection, "beta");
        assert_eq!(request.file.as_deref(), path.to_str());
    }

    #[test]
    fn prompts_for_a_question() {
        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\n");
        let mut prompter = ScriptedPrompter::new(&["Why?"]);
        let mut stdin = Cursor::new(Vec::new());

        let request = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Lines(LineRange::single(2))),
                None,
                &[Requirement::Selection, Requirement::Question],
            ),
            &mut prompter,
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(request.question.as_deref(), Some("Why?"));
    }

    #[test]
    fn parses_a_prompted_line_or_text_selection() {
        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\n");
        let mut prompter = ScriptedPrompter::new(&["2"]);
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(Some(&path), None, None, &[Requirement::Selection]),
            &mut prompter,
            true,
            &mut stdin,
        )
        .unwrap();
        assert_eq!(request.selection, "beta");

        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\n");
        let mut prompter = ScriptedPrompter::new(&["pasted text"]);
        let mut stdin = Cursor::new(Vec::new());
        let request = resolve_request(
            options(Some(&path), None, None, &[Requirement::Selection]),
            &mut prompter,
            true,
            &mut stdin,
        )
        .unwrap();
        assert_eq!(request.selection, "pasted text");
    }

    #[test]
    fn rejects_missing_inputs_without_a_prompter() {
        let (_directory, path) = temp_file("notes.md", "alpha\nbeta\n");
        let mut stdin = Cursor::new(Vec::new());

        let error = resolve_request(
            options(Some(&path), None, None, &[Requirement::Selection]),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no selection provided"));

        let error = resolve_request(
            options(
                Some(&path),
                Some(SelectionArg::Lines(LineRange::single(1))),
                None,
                &[Requirement::Selection, Requirement::Question],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires a question"));
    }

    #[test]
    fn rejects_a_terminal_as_the_document_source() {
        let mut stdin = Cursor::new(Vec::new());
        let error = resolve_request(
            options(
                Some(Path::new("-")),
                Some(SelectionArg::Lines(LineRange::single(1))),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            true,
            &mut stdin,
        )
        .unwrap_err();

        assert!(matches!(error, DocentError::StdinIsTerminal));
    }

    #[test]
    fn rejects_empty_context_when_required() {
        let mut stdin = Cursor::new(Vec::new());
        let error = resolve_request(
            options(
                None,
                Some(SelectionArg::Text("passage".to_string())),
                None,
                &[Requirement::Selection, Requirement::Context],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();

        assert!(error.to_string().contains("requires document context"));
    }

    #[test]
    fn reports_unreadable_files() {
        let mut stdin = Cursor::new(Vec::new());
        let path = Path::new("/definitely/not/a/real/docent-file");
        let error = resolve_request(
            options(
                Some(path),
                Some(SelectionArg::Lines(LineRange::single(1))),
                None,
                &[Requirement::Selection],
            ),
            &mut NoPrompter,
            false,
            &mut stdin,
        )
        .unwrap_err();

        assert!(matches!(error, DocentError::ReadDocument { .. }));
    }
}
