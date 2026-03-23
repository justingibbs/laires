use std::io::Read;
use std::path::Path;

use crate::error::{LairesError, Result};

/// Extract plain text from a .docx file.
///
/// A .docx is a ZIP archive containing `word/document.xml` where text
/// lives in `<w:t>` elements within `<w:p>` paragraphs.
pub fn extract_text_from_docx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)
        .map_err(|e| LairesError::DocxError(format!("cannot open {}: {e}", path.display())))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| LairesError::DocxError(format!("invalid ZIP in {}: {e}", path.display())))?;

    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|e| {
            LairesError::DocxError(format!(
                "word/document.xml not found in {}: {e}",
                path.display()
            ))
        })?
        .read_to_string(&mut xml)
        .map_err(|e| {
            LairesError::DocxError(format!(
                "failed to read word/document.xml in {}: {e}",
                path.display()
            ))
        })?;

    parse_document_xml(&xml)
}

/// Parse the XML from `word/document.xml` and extract paragraph text.
///
/// Text lives in `<w:t>` elements inside `<w:p>` paragraphs.
/// Paragraphs are joined with double newlines; empty paragraphs are skipped.
fn parse_document_xml(xml: &str) -> Result<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_para = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"t" {
                    in_t = false;
                } else if local.as_ref() == b"p" {
                    let trimmed = current_para.trim().to_string();
                    if !trimmed.is_empty() {
                        paragraphs.push(trimmed);
                    }
                    current_para.clear();
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_t {
                    let text = e.unescape().map_err(|err| {
                        LairesError::DocxError(format!("XML decode error: {err}"))
                    })?;
                    current_para.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(LairesError::DocxError(format!("XML parse error: {e}")));
            }
            _ => {}
        }
    }

    // Flush any remaining paragraph
    let trimmed = current_para.trim().to_string();
    if !trimmed.is_empty() {
        paragraphs.push(trimmed);
    }

    Ok(paragraphs.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_simple_paragraphs() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body>
            <w:p><w:r><w:t>Hello world</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>
          </w:body>
        </w:document>"#;

        let result = parse_document_xml(xml).unwrap();
        assert_eq!(result, "Hello world\n\nSecond paragraph");
    }

    #[test]
    fn test_parse_multiple_runs_in_paragraph() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body>
            <w:p>
              <w:r><w:rPr><w:b/></w:rPr><w:t>Bold text</w:t></w:r>
              <w:r><w:t> and normal text</w:t></w:r>
            </w:p>
          </w:body>
        </w:document>"#;

        let result = parse_document_xml(xml).unwrap();
        assert_eq!(result, "Bold text and normal text");
    }

    #[test]
    fn test_parse_empty_paragraphs_skipped() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body>
            <w:p><w:r><w:t>First</w:t></w:r></w:p>
            <w:p><w:pPr><w:spacing w:after="200"/></w:pPr></w:p>
            <w:p><w:r><w:t>Third</w:t></w:r></w:p>
          </w:body>
        </w:document>"#;

        let result = parse_document_xml(xml).unwrap();
        assert_eq!(result, "First\n\nThird");
    }

    #[test]
    fn test_parse_empty_document() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body/>
        </w:document>"#;

        let result = parse_document_xml(xml).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_character_bible_docx() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/character_bible.docx");
        if !path.exists() {
            eprintln!("Skipping: test file not found at {}", path.display());
            return;
        }
        let text = extract_text_from_docx(&path).unwrap();
        assert!(!text.is_empty(), "extracted text should not be empty");
        let word_count = text.split_whitespace().count();
        assert!(word_count > 100, "expected >100 words, got {word_count}");
    }

    #[test]
    fn test_extract_story_outline_docx() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/story_outline.docx");
        if !path.exists() {
            eprintln!("Skipping: test file not found at {}", path.display());
            return;
        }
        let text = extract_text_from_docx(&path).unwrap();
        assert!(!text.is_empty(), "extracted text should not be empty");
    }

    #[test]
    fn test_extract_nonexistent_file() {
        let path = PathBuf::from("/tmp/nonexistent_laires_test.docx");
        let result = extract_text_from_docx(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_hash_consistency() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/character_bible.docx");
        if !path.exists() {
            eprintln!("Skipping: test file not found at {}", path.display());
            return;
        }
        let text1 = extract_text_from_docx(&path).unwrap();
        let text2 = extract_text_from_docx(&path).unwrap();
        assert_eq!(
            text1, text2,
            "two extractions should produce identical text"
        );
    }
}
