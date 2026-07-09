/// Text extraction for uploaded attachments (PROJECT_BRIEF.md §5).
///
/// Supported: PDF (pdf-extract), DOCX/PPTX (zip + XML), plain text formats.
/// Images are stored without extraction — OCR is deferred until a proper
/// selective-OCR pass is worth its system dependencies.
use log::warn;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;
use std::path::Path;

/// Plain-text formats read directly, capped to avoid loading huge files.
const MAX_TEXT_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Classify a file by extension into the note_attachments.file_type vocabulary.
pub fn detect_file_type(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "pptx" => "pptx",
        "txt" | "text" | "log" => "txt",
        "md" | "markdown" => "md",
        "csv" | "tsv" => "csv",
        "json" => "json",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" | "tiff" => "image",
        _ => "other",
    }
    .to_string()
}

/// Extract text content from a file. Returns None when the format has no
/// extractable text (images, unknown binaries) or extraction fails —
/// extraction failure must never block attaching the file itself.
pub fn extract_text(path: &Path, file_type: &str) -> Option<String> {
    let result = match file_type {
        "pdf" => extract_pdf(path),
        "docx" => extract_docx(path),
        "pptx" => extract_pptx(path),
        "txt" | "md" | "csv" | "json" => extract_plain_text(path),
        _ => return None,
    };

    match result {
        Ok(text) => {
            let cleaned = sanitize_extracted_text(&text);
            let trimmed = cleaned.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(e) => {
            warn!("Text extraction failed for {}: {}", path.display(), e);
            None
        }
    }
}

/// Strip control characters that shouldn't appear in real text but do leak
/// out of malformed/complex PDFs (custom glyph-to-codepoint font encodings
/// misdecoding as NUL and other C0/C1 control bytes). Left in place, a NUL
/// byte survives all the way into the summarization prompt and breaks
/// tokenization outright: llama-cpp-2's `str_to_token` builds a `CString`,
/// which errors on any interior NUL ("failed to tokenize prompt" — not a
/// prompt-size issue, however large the file). Keeps newline/tab/CR;
/// everything else in the C0 (U+0000-U+001F), DEL (U+007F), and C1
/// (U+0080-U+009F) control ranges is dropped.
pub fn sanitize_extracted_text(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\r' || c == '\t' || !c.is_control())
        .collect()
}

fn extract_plain_text(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_TEXT_FILE_BYTES {
        return Err(format!("file too large for text extraction ({} bytes)", size));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

fn extract_pdf(path: &Path) -> Result<String, String> {
    // pdf-extract can panic on malformed PDFs; contain it.
    let path = path.to_path_buf();
    std::panic::catch_unwind(move || pdf_extract::extract_text(&path))
        .map_err(|_| "PDF parser panicked".to_string())?
        .map_err(|e| e.to_string())
}

/// DOCX: text lives in word/document.xml as <w:t> runs; <w:p> ends paragraphs.
fn extract_docx(path: &Path) -> Result<String, String> {
    let xml = read_zip_entry(path, "word/document.xml")?;
    extract_xml_text(&xml, "w:t", "w:p")
}

/// PPTX: one XML per slide under ppt/slides/, text in <a:t> runs.
fn extract_pptx(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .collect();
    // slide2.xml must sort after slide1.xml but before slide10.xml
    slide_names.sort_by_key(|n| slide_number(n));

    let mut out = String::new();
    for (idx, name) in slide_names.iter().enumerate() {
        let mut xml = String::new();
        archive
            .by_name(name)
            .map_err(|e| e.to_string())?
            .read_to_string(&mut xml)
            .map_err(|e| e.to_string())?;
        let text = extract_xml_text(&xml, "a:t", "a:p")?;
        if !text.trim().is_empty() {
            out.push_str(&format!("[Slide {}]\n{}\n\n", idx + 1, text.trim()));
        }
    }
    Ok(out)
}

fn slide_number(name: &str) -> u32 {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(u32::MAX)
}

fn read_zip_entry(path: &Path, entry: &str) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut content = String::new();
    archive
        .by_name(entry)
        .map_err(|e| format!("missing {}: {}", entry, e))?
        .read_to_string(&mut content)
        .map_err(|e| e.to_string())?;
    Ok(content)
}

/// Pull character content of `text_tag` elements, inserting newlines at the
/// end of each `para_tag` element.
fn extract_xml_text(xml: &str, text_tag: &str, para_tag: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == text_tag.as_bytes() => in_text = true,
            Ok(Event::End(e)) if e.name().as_ref() == text_tag.as_bytes() => in_text = false,
            Ok(Event::End(e)) if e.name().as_ref() == para_tag.as_bytes() => out.push('\n'),
            Ok(Event::Text(t)) if in_text => {
                out.push_str(&t.decode().map_err(|e| e.to_string())?);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_file_type() {
        assert_eq!(detect_file_type(Path::new("slides.pdf")), "pdf");
        assert_eq!(detect_file_type(Path::new("Notes.DOCX")), "docx");
        assert_eq!(detect_file_type(Path::new("deck.pptx")), "pptx");
        assert_eq!(detect_file_type(Path::new("readme.md")), "md");
        assert_eq!(detect_file_type(Path::new("photo.JPG")), "image");
        assert_eq!(detect_file_type(Path::new("mystery.xyz")), "other");
        assert_eq!(detect_file_type(Path::new("no_extension")), "other");
    }

    #[test]
    fn test_extract_xml_text_docx_shape() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t> world</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let text = extract_xml_text(xml, "w:t", "w:p").unwrap();
        assert_eq!(text.trim(), "Hello world\nSecond paragraph");
    }

    #[test]
    fn test_slide_ordering() {
        assert!(slide_number("ppt/slides/slide2.xml") < slide_number("ppt/slides/slide10.xml"));
    }

    /// Regression test for a real failure: a PDF with a custom glyph-encoded
    /// font extracted to text containing embedded NUL bytes, which reached
    /// the summarization prompt and made llama-cpp-2's `CString::new` fail
    /// tokenization with "failed to tokenize prompt" — a content bug that
    /// looked like a prompt-size issue because the error gave no detail.
    #[test]
    fn sanitize_extracted_text_strips_nul_and_control_bytes() {
        let garbled = "UZH\nBlockchain\nCenter\n\0\0\0n \u{0}k\u{0}k\u{0}$\u{0}o \u{7f}text continues";
        let cleaned = sanitize_extracted_text(garbled);
        assert!(!cleaned.contains('\0'));
        assert!(!cleaned.contains('\u{7f}'));
        assert_eq!(cleaned, "UZH\nBlockchain\nCenter\nn kk$o text continues");
    }

    #[test]
    fn sanitize_extracted_text_keeps_newlines_tabs_and_normal_text() {
        let clean = "Hello\tworld\nSecond line\r\n";
        assert_eq!(sanitize_extracted_text(clean), clean);
    }
}
