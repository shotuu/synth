/// PDF export via genpdf, using the shared ExportData/Block model from
/// export.rs. Liberation Sans (OFL, redistributable, metrically compatible
/// with Arial) is embedded in the binary via include_bytes! so this works
/// identically in dev and bundled builds with no runtime resource lookup.
use anyhow::{anyhow, Result};
use genpdf::elements::{Break, Paragraph, UnorderedList};
use genpdf::style::Style;
use genpdf::{fonts, Alignment, Document, Element, SimplePageDecorator};
use std::path::Path;

use super::export::{format_timestamp, is_source_label, parse_markdown_blocks, speaker_color, Block, ExportData};

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-Bold.ttf");
const ITALIC: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-Italic.ttf");
const BOLD_ITALIC: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-BoldItalic.ttf");

fn font_family() -> Result<fonts::FontFamily<fonts::FontData>> {
    Ok(fonts::FontFamily {
        regular: fonts::FontData::new(REGULAR.to_vec(), None).map_err(|e| anyhow!("font load failed: {}", e))?,
        bold: fonts::FontData::new(BOLD.to_vec(), None).map_err(|e| anyhow!("font load failed: {}", e))?,
        italic: fonts::FontData::new(ITALIC.to_vec(), None).map_err(|e| anyhow!("font load failed: {}", e))?,
        bold_italic: fonts::FontData::new(BOLD_ITALIC.to_vec(), None)
            .map_err(|e| anyhow!("font load failed: {}", e))?,
    })
}

fn push_blocks(doc: &mut Document, blocks: &[Block]) {
    let mut list: Option<UnorderedList> = None;
    let flush_list = |doc: &mut Document, list: &mut Option<UnorderedList>| {
        if let Some(l) = list.take() {
            doc.push(l);
            doc.push(Break::new(0.5));
        }
    };

    for block in blocks {
        match block {
            Block::Heading(level, text) => {
                flush_list(doc, &mut list);
                let size = match level {
                    1 => 15,
                    2 => 13,
                    _ => 11,
                };
                doc.push(Paragraph::new(text.as_str()).styled(Style::new().bold().with_font_size(size)));
                doc.push(Break::new(0.3));
            }
            Block::Paragraph(text) => {
                flush_list(doc, &mut list);
                doc.push(Paragraph::new(text.as_str()));
                doc.push(Break::new(0.3));
            }
            Block::ListItem(text) => {
                list.get_or_insert_with(UnorderedList::new).push(Paragraph::new(text.as_str()));
            }
        }
    }
    flush_list(doc, &mut list);
}

pub fn render_pdf(data: &ExportData, out_path: &Path) -> Result<()> {
    let family = font_family()?;
    let mut doc = Document::new(family);
    doc.set_title(&data.title);
    doc.set_font_size(10);
    doc.set_line_spacing(1.3);

    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(20);
    doc.set_page_decorator(decorator);

    // Header
    doc.push(
        Paragraph::new(data.context_type.to_uppercase())
            .aligned(Alignment::Left)
            .styled(Style::new().with_font_size(8)),
    );
    doc.push(Paragraph::new(data.title.as_str()).styled(Style::new().bold().with_font_size(18)));
    doc.push(Paragraph::new(data.created_at.as_str()).styled(Style::new().with_font_size(9).italic()));
    doc.push(Break::new(1.0));

    // Summary
    doc.push(Paragraph::new("SUMMARY").styled(Style::new().bold().with_font_size(11)));
    doc.push(Break::new(0.3));
    match &data.summary_markdown {
        Some(md) => push_blocks(&mut doc, &parse_markdown_blocks(md)),
        None => doc.push(Paragraph::new("No summary generated for this session.").styled(Style::new().italic())),
    }
    doc.push(Break::new(1.0));

    // Notes
    if let Some(notes) = &data.user_notes_markdown {
        doc.push(Paragraph::new("NOTES").styled(Style::new().bold().with_font_size(11)));
        doc.push(Break::new(0.3));
        push_blocks(&mut doc, &parse_markdown_blocks(notes));
        doc.push(Break::new(1.0));
    }

    // Attachments
    if !data.attachments.is_empty() {
        doc.push(Paragraph::new("ATTACHED FILES").styled(Style::new().bold().with_font_size(11)));
        doc.push(Break::new(0.3));
        for att in &data.attachments {
            doc.push(
                Paragraph::new(format!("{} ({})", att.file_name, att.file_type))
                    .styled(Style::new().bold().with_font_size(10)),
            );
            match &att.extracted_text {
                Some(text) if !text.trim().is_empty() => {
                    // Keep PDF size sane for large attachments
                    let truncated: String = text.chars().take(4000).collect();
                    doc.push(Paragraph::new(truncated).styled(Style::new().with_font_size(9)));
                }
                _ => doc.push(Paragraph::new("No text extracted from this file.").styled(Style::new().italic())),
            }
            doc.push(Break::new(0.5));
        }
        doc.push(Break::new(0.5));
    }

    // Transcript
    doc.push(Paragraph::new("TRANSCRIPT").styled(Style::new().bold().with_font_size(11)));
    doc.push(Break::new(0.3));
    if data.transcript_rows.is_empty() {
        doc.push(Paragraph::new("No transcript available.").styled(Style::new().italic()));
    } else {
        let mut last_speaker: Option<&str> = None;
        for (text, speaker, start) in &data.transcript_rows {
            if !is_source_label(speaker) && speaker.as_deref() != last_speaker {
                let label = speaker.as_deref().unwrap();
                let _ = speaker_color(label); // color info isn't representable in this simple PDF layout
                doc.push(Paragraph::new(label).styled(Style::new().bold().with_font_size(9)));
                last_speaker = speaker.as_deref();
            }
            doc.push(Paragraph::new(format!("[{}] {}", format_timestamp(*start), text)).styled(Style::new().with_font_size(9)));
        }
    }

    doc.render_to_file(out_path)
        .map_err(|e| anyhow!("Failed to render PDF: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organization::export::AttachmentExport;

    fn sample_data() -> ExportData {
        ExportData {
            title: "Team Sync".to_string(),
            context_type: "meeting".to_string(),
            created_at: "2026-01-01T10:00:00Z".to_string(),
            summary_markdown: Some("# Overview\n## Decisions\n- Ship it\n- Cut scope".to_string()),
            transcript_rows: vec![
                ("Hello everyone.".to_string(), Some("Speaker 1".to_string()), Some(0.0)),
                ("Hi there.".to_string(), Some("Speaker 2".to_string()), Some(3.0)),
            ],
            user_notes_markdown: Some("Remember to follow up".to_string()),
            attachments: vec![AttachmentExport {
                file_name: "agenda.txt".to_string(),
                file_type: "txt".to_string(),
                extracted_text: Some("Budget review, hiring plan".to_string()),
            }],
        }
    }

    #[test]
    fn renders_a_valid_nonempty_pdf_file() {
        let tmp = std::env::temp_dir().join(format!("synth-pdf-test-{}.pdf", std::process::id()));
        render_pdf(&sample_data(), &tmp).expect("PDF rendering should succeed");

        let bytes = std::fs::read(&tmp).expect("PDF file should exist");
        assert!(bytes.starts_with(b"%PDF-"), "output must be a valid PDF file");
        assert!(bytes.len() > 1000, "PDF should have real content, got {} bytes", bytes.len());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn renders_empty_session_without_error() {
        let tmp = std::env::temp_dir().join(format!("synth-pdf-empty-test-{}.pdf", std::process::id()));
        let data = ExportData {
            title: "Empty".to_string(),
            context_type: "custom".to_string(),
            created_at: "2026-01-01".to_string(),
            summary_markdown: None,
            transcript_rows: vec![],
            user_notes_markdown: None,
            attachments: vec![],
        };
        render_pdf(&data, &tmp).expect("PDF rendering should handle empty content");
        assert!(tmp.exists());
        std::fs::remove_file(&tmp).ok();
    }
}
