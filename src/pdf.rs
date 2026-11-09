//! PDF text extraction for the terminal input path.
//!
//! PDFs are detected by their magic header, decoded with `pdf-extract`, and
//! joined into plain text. Page ranges are applied before joining, so the
//! document text only contains the pages the user asked for.

use pdf_extract::extract_text_from_mem_by_pages;

use crate::input::PageRange;

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
    let page_texts = extract_text_from_mem_by_pages(bytes)
        .map_err(|error| format!("could not decode the PDF: {error}"))?;
    let page_count = page_texts.len();
    if page_count == 0 {
        return Err("the PDF has no pages".to_string());
    }

    let range = pages.unwrap_or(PageRange {
        start: 1,
        end: page_count,
    });
    if range.start == 0 {
        return Err("page numbers start at 1".to_string());
    }
    if range.start > page_count {
        return Err(format!(
            "page range {range} is out of bounds; the PDF has {page_count} pages"
        ));
    }

    let end = range.end.min(page_count);
    let text = page_texts[range.start - 1..end]
        .iter()
        .map(|page| page.trim_matches('\n'))
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(ExtractedText {
        text,
        pages: page_count,
    })
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
}
