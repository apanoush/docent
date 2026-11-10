//! EPUB detection and text extraction.
//!
//! EPUB is a ZIP archive with an OPF package document. Text is read in the
//! order of the linear spine. A "page" in the CLI is a 1-based spine section,
//! because reflowable EPUBs have no fixed pages.

use std::io::Cursor;

use html2text::render::TrivialDecorator;
use rbook::Epub;
use rbook::epub::reader::LinearBehavior;
use zip::ZipArchive;

use crate::input::PageRange;

/// Largest uncompressed ZIP entry accepted before parsing.
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
/// Largest total uncompressed size accepted before parsing.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// Largest joined text returned to the caller.
const MAX_TEXT_BYTES: usize = 32 * 1024 * 1024;
/// `html2text` wraps at this width. A large width keeps paragraphs on one line.
const TEXT_WIDTH: usize = 1_000_000;

/// Whether `bytes` start with a ZIP local-file, end-of-central-directory, or
/// spanned marker.
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

/// Extracted text and the number of linear spine sections in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedText {
    /// Joined section text.
    pub text: String,
    /// Total number of linear spine sections.
    pub sections: usize,
}

/// Extracts text from every linear spine section, or only from `pages` when
/// given. Section numbers are 1-based and inclusive. The end of the range is
/// clamped to the last section; a start past the last section is an error.
pub fn extract(bytes: Vec<u8>, pages: Option<PageRange>) -> Result<ExtractedText, String> {
    preflight(&bytes)?;

    let epub = Epub::read(Cursor::new(bytes))
        .map_err(|error| format!("could not parse the EPUB package: {error}"))?;
    reject_encrypted_content(&epub)?;

    let reader = epub
        .reader_builder()
        .linear_behavior(LinearBehavior::LinearOnly)
        .create();
    let sections = reader.len();
    if sections == 0 {
        return Err("the EPUB has no linear spine sections".to_string());
    }

    let range = pages.unwrap_or(PageRange {
        start: 1,
        end: sections,
    });
    if range.start > sections {
        return Err(format!(
            "page range {range} is out of bounds; the EPUB has {sections} sections"
        ));
    }
    let end = range.end.min(sections);

    let mut parts = Vec::new();
    let mut text_bytes: usize = 0;
    for position in (range.start - 1)..end {
        let content = reader.get(position).map_err(|error| {
            format!(
                "could not read EPUB section {}: {error}",
                position.saturating_add(1)
            )
        })?;
        let section = html_to_text(content.content())?;
        if section.trim().is_empty() {
            continue;
        }
        text_bytes = text_bytes.saturating_add(section.len());
        if text_bytes > MAX_TEXT_BYTES {
            return Err(format!(
                "EPUB text is larger than the {MAX_TEXT_BYTES} byte limit"
            ));
        }
        parts.push(section);
    }

    if parts.is_empty() {
        return Err(if pages.is_some() {
            "the selected EPUB sections have no extractable text".to_string()
        } else {
            "the EPUB has no extractable text; it may be image-only or DRM-protected".to_string()
        });
    }

    Ok(ExtractedText {
        text: parts.join("\n\n"),
        sections,
    })
}

/// Checks that the archive is readable, is an EPUB, and is not a decompression
/// bomb before the EPUB parser touches it.
fn preflight(bytes: &[u8]) -> Result<(), String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("not a valid ZIP archive: {error}"))?;
    let mut has_container = false;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("could not read ZIP entry: {error}"))?;
        let name = file
            .name()
            .map_err(|error| format!("could not read ZIP entry name: {error}"))?;
        if name == "META-INF/container.xml" {
            has_container = true;
        }
        let size = file.size();
        if size > MAX_ENTRY_BYTES {
            return Err(format!(
                "ZIP entry {name:?} is larger than the {MAX_ENTRY_BYTES} byte limit"
            ));
        }
        total = total.saturating_add(size);
        if total > MAX_TOTAL_BYTES {
            return Err(format!(
                "EPUB uncompressed size is larger than the {MAX_TOTAL_BYTES} byte limit"
            ));
        }
    }
    if !has_container {
        return Err("ZIP archive but not an EPUB; META-INF/container.xml is missing".to_string());
    }
    Ok(())
}

/// Rejects encrypted spine content while allowing the font-obfuscation
/// algorithms that some DRM-free EPUBs use.
fn reject_encrypted_content(epub: &Epub) -> Result<(), String> {
    if !epub.contains_resource("/META-INF/encryption.xml") {
        return Ok(());
    }
    let xml = epub
        .read_resource_str("/META-INF/encryption.xml")
        .map_err(|error| format!("could not read EPUB encryption metadata: {error}"))?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut encrypted_data = 0usize;
    let mut algorithms = Vec::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(element))
            | Ok(quick_xml::events::Event::Empty(element)) => match element.local_name().as_ref() {
                "EncryptedData" => encrypted_data += 1,
                "EncryptionMethod" => {
                    for attribute in element.attributes().with_checks(false) {
                        let attribute = attribute.map_err(|error| {
                            format!("could not read EPUB encryption metadata: {error}")
                        })?;
                        if attribute.key.local_name().as_ref() == "Algorithm" {
                            algorithms.push(attribute.value.to_string());
                        }
                    }
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(format!("could not parse EPUB encryption metadata: {error}"));
            }
        }
    }

    let encrypted = encrypted_data > 0
        && (algorithms.is_empty()
            || algorithms
                .iter()
                .any(|algorithm| !is_font_obfuscation(algorithm)));
    if encrypted {
        return Err(
            "the EPUB contains encrypted content; DRM-protected EPUBs are not supported"
                .to_string(),
        );
    }
    Ok(())
}

/// Font obfuscation is not content encryption; the text can still be read.
fn is_font_obfuscation(algorithm: &str) -> bool {
    matches!(
        algorithm.trim(),
        "http://www.idpf.org/2008/embedding" | "http://ns.adobe.com/pdf/enc#RC"
    )
}

/// Converts one XHTML document to normalized plain text.
fn html_to_text(html: &str) -> Result<String, String> {
    let text = html2text::config::with_decorator(TrivialDecorator::new())
        .string_from_read(html.as_bytes(), TEXT_WIDTH)
        .map_err(|error| format!("could not convert EPUB XHTML to text: {error}"))?;
    Ok(normalize_text(&text))
}

/// Normalizes whitespace without destroying the indentation of `<pre>` blocks.
fn normalize_text(text: &str) -> String {
    let text = text.replace('\r', "").replace('\u{a0}', " ");
    let mut result = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            blank_lines += 1;
            if blank_lines == 1 {
                result.push('\n');
            }
        } else {
            blank_lines = 0;
            result.push_str(line);
            result.push('\n');
        }
    }
    result.trim_matches('\n').to_string()
}

#[cfg(test)]
pub(crate) mod tests;
