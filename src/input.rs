//! Terminal input adapters: files, stdin, line ranges, and prompts.

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::DocentError;
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
        let value = value.trim();
        let (start_text, end_text) = match value.split_once(':') {
            Some((start, end)) => (start, Some(end)),
            None => (value, None),
        };

        let start = parse_line_number(start_text)?;
        let end = match end_text {
            Some(end) => parse_line_number(end)?,
            None => start,
        };

        if start > end {
            return Err("line range start must not exceed end".to_string());
        }

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

fn parse_line_number(value: &str) -> Result<usize, String> {
    let line = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("'{value}' is not a line number"))?;
    if line == 0 {
        return Err("line numbers start at 1".to_string());
    }
    Ok(line)
}

/// How the terminal selected a passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionArg {
    /// A line range.
    Lines(LineRange),
    /// Pasted selection text.
    Text(String),
    /// An exact phrase to locate in the document.
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

    /// Finds the first exact occurrence of `phrase`.
    pub fn find(&self, phrase: &str) -> Option<(usize, usize)> {
        if phrase.is_empty() {
            return None;
        }
        let start = self.text.find(phrase)?;
        Some((start, start + phrase.len()))
    }

    /// Counts exact occurrences of `phrase`.
    pub fn occurrence_count(&self, phrase: &str) -> usize {
        if phrase.is_empty() {
            return 0;
        }
        self.text.match_indices(phrase).count()
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
        Some(path) => Some(read_document(path, stdin_is_terminal, stdin)?),
        None if !stdin_is_terminal => Some(read_stdin(stdin)?),
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
        Some(SelectionArg::Text(text)) => {
            let offsets = document.as_ref().and_then(|document| document.find(&text));
            (
                text,
                offsets.map(|(start, _)| start),
                offsets.map(|(_, end)| end),
            )
        }
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
            (phrase, Some(start), Some(end))
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
    stdin_is_terminal: bool,
    stdin: &mut dyn Read,
) -> Result<Document, DocentError> {
    if path == Path::new("-") {
        if stdin_is_terminal {
            return Err(DocentError::StdinIsTerminal);
        }
        return read_stdin(stdin);
    }

    let text = fs::read_to_string(path).map_err(|source| DocentError::ReadDocument {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Document::from_text(path.display().to_string(), text))
}

fn read_stdin(stdin: &mut dyn Read) -> Result<Document, DocentError> {
    let mut text = String::new();
    stdin
        .read_to_string(&mut text)
        .map_err(DocentError::ReadStdin)?;
    Ok(Document::from_text(STDIN_NAME, text))
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

    fn options<'a>(
        file: Option<&'a Path>,
        selection: Option<SelectionArg>,
        question: Option<&str>,
        requires: &'a [Requirement],
    ) -> RequestOptions<'a> {
        RequestOptions {
            file,
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
