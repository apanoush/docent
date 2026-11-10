use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;

fn zip_entries(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(b"application/epub+zip").unwrap();
    for (name, content) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn container(opf_path: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="{opf_path}" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
    )
}

fn package(version: &str, chapters: &[(&str, &str)]) -> String {
    let manifest = chapters
        .iter()
        .map(|(id, href)| {
            format!(r#"<item id="{id}" href="{href}" media-type="application/xhtml+xml"/>"#)
        })
        .collect::<String>();
    let spine = chapters
        .iter()
        .map(|(id, _)| format!(r#"<itemref idref="{id}"/>"#))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="{version}" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:test</dc:identifier>
    <dc:title>Sample Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>{manifest}</manifest>
  <spine>{spine}</spine>
</package>"#
    )
}

/// Builds a minimal EPUB 3 with one XHTML document per section body.
pub(crate) fn sample_epub(sections: &[&str]) -> Vec<u8> {
    let mut chapters = Vec::new();
    let mut documents = Vec::new();
    for (index, body) in sections.iter().enumerate() {
        let id = format!("c{}", index + 1);
        let href = format!("text/ch{}.xhtml", index + 1);
        chapters.push((id, href.clone()));
        documents.push((
            format!("OEBPS/{href}"),
            format!(r#"<html><body>{body}</body></html>"#),
        ));
    }
    let package = package(
        "3.0",
        &chapters
            .iter()
            .map(|(id, href)| (id.as_str(), href.as_str()))
            .collect::<Vec<_>>(),
    );
    let mut entries = vec![
        (
            "META-INF/container.xml".to_string(),
            container("OEBPS/content.opf"),
        ),
        ("OEBPS/content.opf".to_string(), package),
    ];
    entries.extend(documents);
    let borrowed = entries
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    zip_entries(&borrowed)
}

fn encrypted_epub(algorithm: &str) -> Vec<u8> {
    let encryption = format!(
        r#"<?xml version="1.0"?>
<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container"
            xmlns:enc="http://www.w3.org/2001/04/xmlenc#">
  <enc:EncryptedData>
    <enc:EncryptionMethod Algorithm="{algorithm}"/>
    <enc:CipherData><enc:CipherReference URI="OEBPS/text/ch1.xhtml"/></enc:CipherData>
  </enc:EncryptedData>
</encryption>"#
    );
    zip_entries(&[
        ("META-INF/container.xml", &container("OEBPS/content.opf")),
        (
            "OEBPS/content.opf",
            &package("3.0", &[("c1", "text/ch1.xhtml")]),
        ),
        (
            "OEBPS/text/ch1.xhtml",
            "<html><body><p>Secret text.</p></body></html>",
        ),
        ("META-INF/encryption.xml", &encryption),
    ])
}

#[test]
fn detects_zip_magic() {
    let epub = sample_epub(&["<p>alpha</p>"]);
    assert!(is_zip(&epub));
    assert!(!is_zip(b"%PDF-1.7\n"));
    assert!(!is_zip(b"plain text"));
}

#[test]
fn extracts_sections_in_spine_order() {
    let extracted = extract(sample_epub(&["<p>alpha</p>", "<p>beta</p>"]), None).unwrap();
    assert_eq!(extracted.sections, 2);
    let alpha = extracted.text.find("alpha").unwrap();
    let beta = extracted.text.find("beta").unwrap();
    assert!(alpha < beta);
}

#[test]
fn scopes_to_a_section_range() {
    let extracted = extract(
        sample_epub(&["<p>alpha</p>", "<p>beta</p>"]),
        Some(PageRange { start: 2, end: 2 }),
    )
    .unwrap();
    assert_eq!(extracted.sections, 2);
    assert!(!extracted.text.contains("alpha"));
    assert!(extracted.text.contains("beta"));
}

#[test]
fn clamps_the_end_and_rejects_a_start_past_the_last_section() {
    let epub = sample_epub(&["<p>alpha</p>", "<p>beta</p>"]);
    let extracted = extract(epub.clone(), Some(PageRange { start: 2, end: 9 })).unwrap();
    assert!(extracted.text.contains("beta"));

    let error = extract(epub, Some(PageRange { start: 3, end: 4 })).unwrap_err();
    assert!(error.contains("out of bounds"), "{error}");
    assert!(error.contains("2 sections"), "{error}");
}

#[test]
fn rejects_a_zip_that_is_not_an_epub() {
    let bytes = zip_entries(&[("readme.txt", "not an epub")]);
    let error = extract(bytes, None).unwrap_err();
    assert!(error.contains("not an EPUB"), "{error}");
}

#[test]
fn rejects_encrypted_content() {
    let error = extract(
        encrypted_epub("http://www.w3.org/2001/04/xmlenc#aes256-cbc"),
        None,
    )
    .unwrap_err();
    assert!(error.contains("encrypted content"), "{error}");
}

#[test]
fn allows_font_obfuscation() {
    let extracted = extract(encrypted_epub("http://www.idpf.org/2008/embedding"), None).unwrap();
    assert!(extracted.text.contains("Secret text"));
}

#[test]
fn rejects_an_epub_without_extractable_text() {
    let error = extract(sample_epub(&[r#"<img src="cover.jpg"/>"#]), None).unwrap_err();
    assert!(error.contains("no extractable text"), "{error}");
}

#[test]
fn rejects_a_selected_range_without_text() {
    let error = extract(
        sample_epub(&[r#"<img src="cover.jpg"/>"#, "<p>chapter</p>"]),
        Some(PageRange { start: 1, end: 1 }),
    )
    .unwrap_err();
    assert!(error.contains("selected EPUB sections"), "{error}");
}

#[test]
fn skips_scripts_and_styles_and_normalizes_whitespace() {
    let extracted = extract(
        sample_epub(&[
            r#"<script>bad script</script><style>bad style</style><p>Hello&nbsp;<em>world</em>.</p><p></p><p></p><p>Next</p>"#,
        ]),
        None,
    )
    .unwrap();
    assert!(!extracted.text.contains("bad script"));
    assert!(!extracted.text.contains("bad style"));
    assert!(
        extracted.text.contains("Hello world."),
        "{}",
        extracted.text
    );
    assert!(!extracted.text.contains('\u{a0}'));
    assert!(!extracted.text.contains("\n\n\n"));
}

#[test]
fn recovers_from_malformed_xhtml() {
    let extracted = extract(
        sample_epub(&["<p>Unclosed <b>bold <i>italic<p>Next paragraph &bogus; text"]),
        None,
    )
    .unwrap();
    assert!(extracted.text.contains("Unclosed bold italic"));
    assert!(extracted.text.contains("Next paragraph"));
}

#[test]
fn reads_epub2_packages() {
    let bytes = zip_entries(&[
        ("META-INF/container.xml", &container("OEBPS/content.opf")),
        (
            "OEBPS/content.opf",
            &package("2.0", &[("c1", "text/ch1.xhtml")]),
        ),
        (
            "OEBPS/text/ch1.xhtml",
            "<html><body><p>Old EPUB text.</p></body></html>",
        ),
    ]);
    let extracted = extract(bytes, None).unwrap();
    assert!(extracted.text.contains("Old EPUB text"));
}

#[test]
fn resolves_a_nested_package_path() {
    let bytes = zip_entries(&[
        ("META-INF/container.xml", &container("EPUB/package.opf")),
        (
            "EPUB/package.opf",
            &package("3.0", &[("c1", "text/ch1.xhtml")]),
        ),
        (
            "EPUB/text/ch1.xhtml",
            "<html><body><p>Nested text.</p></body></html>",
        ),
    ]);
    let extracted = extract(bytes, None).unwrap();
    assert!(extracted.text.contains("Nested text"));
}

#[test]
fn resolves_a_percent_encoded_href() {
    let bytes = zip_entries(&[
        ("META-INF/container.xml", &container("OEBPS/content.opf")),
        (
            "OEBPS/content.opf",
            &package("3.0", &[("c1", "text/ch%201.xhtml")]),
        ),
        (
            "OEBPS/text/ch 1.xhtml",
            "<html><body><p>Encoded path.</p></body></html>",
        ),
    ]);
    let extracted = extract(bytes, None).unwrap();
    assert!(extracted.text.contains("Encoded path"));
}

#[test]
fn rejects_an_empty_spine() {
    let bytes = zip_entries(&[
        ("META-INF/container.xml", &container("OEBPS/content.opf")),
        ("OEBPS/content.opf", &package("3.0", &[])),
    ]);
    let error = extract(bytes, None).unwrap_err();
    assert!(
        error.contains("no linear spine sections") || error.contains("package"),
        "{error}"
    );
}

#[test]
fn normalizes_text_without_collapsing_preformatted_indentation() {
    let text = normalize_text("  indented\n\n\n\nnext\r\nline \u{a0} here\n");
    assert_eq!(text, "  indented\n\nnext\nline   here");
}
