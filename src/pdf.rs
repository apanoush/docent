//! PDF text extraction for the terminal input path.
//!
//! PDFs are detected by their magic header, decoded with `pdf-extract`, and
//! joined into plain text. Page ranges are applied before joining, so the
//! document text only contains the pages the user asked for.
//!
//! `pdf-extract` loses text and spacing on a few documents with broken font
//! encodings. When its output contains NUL, control characters, or replacement
//! characters, the same pages are decoded again with `pdf-inspector` and the
//! cleaner result wins. The final text always passes through a small sanitizer.

use std::collections::{BTreeMap, HashSet};

use pdf_extract::{Document, PlainTextOutput, extract_text_from_mem_by_pages, output_doc_page};
use pdf_inspector::extractor::{
    ItemType, extract_text_with_positions_mem_pages, group_into_lines_preserving_all_text,
};

use crate::input::PageRange;

/// Error reported when the extractor panics on a document.
const UNSUPPORTED_PDF: &str =
    "could not decode the PDF: the extractor failed on an unsupported document structure";

/// Page count at which decoding is spread across worker threads.
const PARALLEL_PAGE_THRESHOLD: usize = 4;

/// Whether `bytes` start with the PDF magic header.
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

/// Extracted text and the number of pages in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedText {
    /// Joined page text.
    pub text: String,
    /// Total number of pages in the document.
    pub pages: usize,
}

/// Extracts text from every page, or only from `pages` when given.
///
/// Page numbers are 1-based and inclusive. The end of the range is clamped to
/// the last page, matching `pdftotext -l`, so a caller can request a window
/// around the final page without knowing the page count. A start past the last
/// page is still an error.
pub fn extract(bytes: &[u8], pages: Option<PageRange>) -> Result<ExtractedText, String> {
    let primary = extract_primary(bytes, pages)?;
    if dirty_score(&primary.text) == 0 {
        return Ok(ExtractedText {
            text: sanitize_text(&primary.text),
            pages: primary.pages,
        });
    }

    let primary_score = dirty_score(&primary.text);
    let fallback = extract_fallback(bytes, pages, primary.pages);
    let text = match fallback {
        Ok(text)
            if dirty_score(&text) < primary_score
                && word_count(&text) * 10 >= word_count(&primary.text) * 9 =>
        {
            text
        }
        _ => primary.text,
    };

    Ok(ExtractedText {
        text: sanitize_text(&text),
        pages: primary.pages,
    })
}

/// Extracts text with `pdf-extract`, the default backend.
fn extract_primary(bytes: &[u8], pages: Option<PageRange>) -> Result<ExtractedText, String> {
    let (page_texts, page_count) = match pages {
        None if worker_count() < 2 => {
            // Keep the library's single-pass decoder on single-core machines.
            let page_texts = decode_all(bytes)?;
            let page_count = page_texts.len();
            (page_texts, page_count)
        }
        _ => decode(bytes, pages)?,
    };
    if page_count == 0 {
        return Err("the PDF has no pages".to_string());
    }

    let text = page_texts
        .iter()
        .map(|page| page.trim_matches('\n'))
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(ExtractedText {
        text,
        pages: page_count,
    })
}

/// Decodes the selected pages with `pdf-inspector`.
///
/// This backend handles CID and two-byte fonts more reliably than
/// `pdf-extract`. It is only used when the primary text is visibly broken.
fn extract_fallback(
    bytes: &[u8],
    pages: Option<PageRange>,
    page_count: usize,
) -> Result<String, String> {
    let filter = page_filter(pages, page_count);
    let items = extract_text_with_positions_mem_pages(bytes, filter.as_ref())
        .map_err(|error| format!("could not decode the PDF: {error}"))?;
    let items = items
        .into_iter()
        .filter(|item| matches!(item.item_type, ItemType::Text))
        .collect();
    let lines = group_into_lines_preserving_all_text(items);

    let mut by_page: BTreeMap<u32, String> = BTreeMap::new();
    for line in lines {
        let text = strip_script_tags(&line.text());
        if text.is_empty() {
            continue;
        }
        let page = by_page.entry(line.page).or_default();
        page.push_str(&text);
        page.push('\n');
    }

    let start = pages.map_or(1, |range| range.start);
    let end = pages.map_or(page_count, |range| range.end.min(page_count));
    let mut page_texts = Vec::with_capacity(end.saturating_sub(start).saturating_add(1));
    for page in start..=end {
        let text = by_page.remove(&(page as u32)).unwrap_or_default();
        page_texts.push(text.trim_matches('\n').to_string());
    }
    Ok(page_texts.join("\n\n"))
}

fn page_filter(pages: Option<PageRange>, page_count: usize) -> Option<HashSet<u32>> {
    pages.map(|range| {
        (range.start..=range.end.min(page_count))
            .map(|page| page as u32)
            .collect()
    })
}

/// `TextLine::text` renders super- and subscripts as inline HTML tags.
fn strip_script_tags(text: &str) -> String {
    text.replace("<sup>", "")
        .replace("</sup>", "")
        .replace("<sub>", "")
        .replace("</sub>", "")
}

/// Counts characters that usually signal a failed font decode.
fn dirty_score(text: &str) -> usize {
    text.chars()
        .filter(|&ch| {
            matches!(ch, '\0' | '\u{fffd}')
                || (ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'))
        })
        .count()
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Removes characters that no model or terminal should receive.
fn sanitize_text(text: &str) -> String {
    text.split_inclusive('\n')
        .map(sanitize_line)
        .collect::<Vec<_>>()
        .concat()
}

fn sanitize_line(line: &str) -> String {
    let line = if is_dense_nul_run(line) {
        line.replace('\0', "")
    } else {
        line.replace('\0', " ")
    };

    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                output.push('\n');
            }
            '\n' | '\t' => output.push(ch),
            '\u{2028}' | '\u{2029}' => output.push('\n'),
            '\u{fffd}' | '\u{feff}' | '\u{200b}' => {}
            c if c.is_control() => {}
            c if is_private_use(c) || is_bidi_control(c) || is_noncharacter(c) => {}
            c => output.push(c),
        }
    }
    output
}

/// Whether NULs are interleaved with printable text, as in two-byte font text.
fn is_dense_nul_run(line: &str) -> bool {
    let nul = line.bytes().filter(|&byte| byte == 0).count();
    if nul < 2 {
        return false;
    }
    let other = line.chars().count().saturating_sub(nul);
    nul * 2 >= other
}

fn is_private_use(ch: char) -> bool {
    matches!(
        ch,
        '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}'
    )
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn is_noncharacter(ch: char) -> bool {
    let value = ch as u32;
    (0xfdd0..=0xfdef).contains(&value) || value & 0xfffe == 0xfffe
}

/// Decodes every page of a PDF with the library's single-pass decoder.
fn decode_all(bytes: &[u8]) -> Result<Vec<String>, String> {
    guard(|| {
        extract_text_from_mem_by_pages(bytes)
            .map_err(|error| format!("could not decode the PDF: {error}"))
    })
}

/// Parses the document once and decodes the requested pages.
///
/// A page range only renders the selected pages, so a one-page window does not
/// pay for the text of every other page. Larger page sets are spread across
/// worker threads.
fn decode(bytes: &[u8], pages: Option<PageRange>) -> Result<(Vec<String>, usize), String> {
    guard(|| {
        let mut document = Document::load_mem(bytes)
            .map_err(|error| format!("could not decode the PDF: {error}"))?;
        if document.is_encrypted() {
            document
                .decrypt("")
                .map_err(|error| format!("could not decode the PDF: {error}"))?;
        }

        let page_count = document.get_pages().len();
        if page_count == 0 {
            return Err("the PDF has no pages".to_string());
        }

        let range = match pages {
            Some(range) => {
                if range.start == 0 {
                    return Err("page numbers start at 1".to_string());
                }
                if range.end < range.start {
                    return Err("page range start must not exceed end".to_string());
                }
                if range.start > page_count {
                    return Err(format!(
                        "page range {range} is out of bounds; the PDF has {page_count} pages"
                    ));
                }
                range
            }
            None => PageRange {
                start: 1,
                end: page_count,
            },
        };

        let end = range.end.min(page_count);
        let numbers: Vec<usize> = (range.start..=end).collect();
        let texts = extract_pages(&document, &numbers)?;
        Ok((texts, page_count))
    })
}

/// Decodes selected pages sequentially or across worker threads.
fn extract_pages(document: &Document, pages: &[usize]) -> Result<Vec<String>, String> {
    let workers = worker_count().min(pages.len());
    if pages.len() < PARALLEL_PAGE_THRESHOLD || workers < 2 {
        return extract_pages_sequential(document, pages);
    }

    let chunk_size = pages.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles: Vec<_> = pages
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(|| extract_pages_sequential(document, chunk)))
            .collect();

        let mut texts = Vec::with_capacity(pages.len());
        for handle in handles {
            match handle.join() {
                Ok(result) => texts.extend(result?),
                Err(_) => return Err(UNSUPPORTED_PDF.to_string()),
            }
        }
        Ok(texts)
    })
}

fn extract_pages_sequential(document: &Document, pages: &[usize]) -> Result<Vec<String>, String> {
    pages
        .iter()
        .map(|&page| extract_page(document, page))
        .collect()
}

/// Decodes one page, turning extractor panics into a normal error.
fn extract_page(document: &Document, page: usize) -> Result<String, String> {
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let page_number =
            u32::try_from(page).map_err(|_| format!("page {page} is out of range"))?;
        let mut text = String::new();
        {
            let mut output = PlainTextOutput::new(&mut text);
            output_doc_page(document, &mut output, page_number)
                .map_err(|error| format!("could not decode the PDF: {error}"))?;
        }
        Ok::<String, String>(text)
    }));

    match decoded {
        Ok(result) => result,
        Err(_) => Err(UNSUPPORTED_PDF.to_string()),
    }
}

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

/// Runs the decoder, turning panics into normal errors.
fn guard<T>(
    decoder: impl FnOnce() -> Result<T, String> + std::panic::UnwindSafe,
) -> Result<T, String> {
    match catch_decoder_panic(decoder) {
        Ok(result) => result,
        Err(_) => Err(UNSUPPORTED_PDF.to_string()),
    }
}

/// Runs the extractor and catches panics.
///
/// The CLI is single-threaded while a document is read. The panic hook is
/// silenced for this call so a library panic does not print a backtrace before
/// the normal input error. Unit tests keep the default hook so unexpected
/// panics stay visible.
fn catch_decoder_panic<F, T>(decoder: F) -> std::thread::Result<T>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    #[cfg(not(test))]
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(decoder);
        std::panic::set_hook(previous);
        result
    }

    #[cfg(test)]
    {
        std::panic::catch_unwind(decoder)
    }
}

#[cfg(test)]
pub(crate) fn sample_pdf(pages: &[&str]) -> Vec<u8> {
    let page_ids: Vec<usize> = (0..pages.len()).map(|index| 4 + index * 2).collect();
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
    ];

    for (index, text) in pages.iter().enumerate() {
        let content_id = page_ids[index] + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let stream = format!("BT /F1 12 Tf 72 720 Td ({escaped}) Tj ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }

    let xref_offset = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    pdf.push_str("0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_dense_nul_runs() {
        assert_eq!(sanitize_text("\0P\0a\0s\0s\0 \0@\0Γ"), "Pass @Γ");
    }

    #[test]
    fn sanitize_replaces_isolated_nuls() {
        assert_eq!(sanitize_text("word\0another"), "word another");
    }

    #[test]
    fn sanitize_removes_controls_and_normalizes_separators() {
        let text = "a\u{1}b\u{7f}c\u{9f}d\re\r\nf\u{2028}g\u{2029}h";
        assert_eq!(sanitize_text(text), "abcd\ne\nf\ng\nh");
    }

    #[test]
    fn sanitize_drops_replacement_private_use_and_bidi() {
        assert_eq!(
            sanitize_text("a\u{fffd}b\u{e000}c\u{202e}d\u{feff}e"),
            "abcde"
        );
    }

    #[test]
    fn sanitize_keeps_clean_text() {
        assert_eq!(sanitize_text("Héllo, 世界!\n"), "Héllo, 世界!\n");
    }

    #[test]
    fn strips_script_tags_from_fallback_lines() {
        assert_eq!(strip_script_tags("x<sup>1</sup>y<sub>2</sub>"), "x1y2");
    }

    #[test]
    fn page_filter_clamps_the_end() {
        let filter = page_filter(Some(PageRange { start: 2, end: 9 }), 3);
        assert_eq!(filter, Some(HashSet::from([2, 3])));
    }

    #[test]
    fn detects_pdf_magic_bytes() {
        assert!(is_pdf(b"%PDF-1.7\n"));
        assert!(!is_pdf(b"plain text"));
        assert!(!is_pdf(b""));
    }

    #[test]
    fn extracts_all_pages() {
        let pdf = sample_pdf(&["first page", "second page"]);
        let extracted = extract(&pdf, None).unwrap();
        assert_eq!(extracted.pages, 2);
        assert!(extracted.text.contains("first page"), "{}", extracted.text);
        assert!(extracted.text.contains("second page"), "{}", extracted.text);
        assert!(extracted.text.find("first page") < extracted.text.find("second page"));
    }

    #[test]
    fn extracts_a_selected_page_range() {
        let pdf = sample_pdf(&["first page", "second page", "third page"]);
        let extracted = extract(&pdf, Some(PageRange { start: 2, end: 3 })).unwrap();
        assert_eq!(extracted.pages, 3);
        assert!(!extracted.text.contains("first page"), "{}", extracted.text);
        assert!(extracted.text.contains("second page"), "{}", extracted.text);
        assert!(extracted.text.contains("third page"), "{}", extracted.text);
    }

    #[test]
    fn clamps_the_end_page() {
        let pdf = sample_pdf(&["first page", "second page"]);
        let extracted = extract(&pdf, Some(PageRange { start: 2, end: 9 })).unwrap();
        assert_eq!(extracted.pages, 2);
        assert!(extracted.text.contains("second page"), "{}", extracted.text);
        assert!(!extracted.text.contains("first page"), "{}", extracted.text);
    }

    #[test]
    fn rejects_a_start_page_past_the_end() {
        let pdf = sample_pdf(&["first page", "second page"]);
        let error = extract(&pdf, Some(PageRange { start: 3, end: 4 })).unwrap_err();
        assert!(error.contains("out of bounds"), "{error}");
        assert!(error.contains("2 pages"), "{error}");
    }

    #[test]
    fn returns_empty_text_for_pages_without_a_text_layer() {
        let pdf = sample_pdf(&[""]);
        let extracted = extract(&pdf, None).unwrap();
        assert!(extracted.text.trim().is_empty());
    }

    #[test]
    fn rejects_malformed_pdfs() {
        let error = extract(b"%PDF-1.4\nnot really a pdf", None).unwrap_err();
        assert!(error.contains("could not decode"), "{error}");
    }

    #[test]
    fn contains_extractor_panics() {
        let error =
            guard(|| -> Result<Vec<String>, String> { panic!("simulated extractor panic") })
                .unwrap_err();

        assert!(
            error.contains("extractor failed on an unsupported document structure"),
            "{error}"
        );
    }

    #[test]
    fn preserves_page_order_with_parallel_extraction() {
        let pdf = sample_pdf(&["first page", "second page", "third page", "fourth page"]);
        let extracted = extract(&pdf, None).unwrap();
        let text = extracted.text;
        let first = text.find("first page").unwrap();
        let second = text.find("second page").unwrap();
        let third = text.find("third page").unwrap();
        let fourth = text.find("fourth page").unwrap();

        assert!(first < second && second < third && third < fourth, "{text}");
    }

    #[test]
    fn rejects_invalid_page_ranges_on_the_selected_path() {
        let pdf = sample_pdf(&["first page", "second page"]);

        let error = extract(&pdf, Some(PageRange { start: 0, end: 1 })).unwrap_err();
        assert!(error.contains("page numbers start at 1"), "{error}");

        let error = extract(&pdf, Some(PageRange { start: 2, end: 1 })).unwrap_err();
        assert!(error.contains("start must not exceed end"), "{error}");
    }
}
