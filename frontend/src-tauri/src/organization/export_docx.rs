/// DOCX export via docx-rs, using the shared ExportData/Block model.
use anyhow::{anyhow, Result};
use docx_rs::{AlignmentType, Docx, Paragraph, Run};
use std::fs::File;
use std::path::Path;

use super::export::{format_timestamp, is_source_label, parse_markdown_blocks, Block, ExportData};

fn heading_size(level: u8) -> usize {
    match level {
        1 => 32,
        2 => 26,
        _ => 22,
    }
}

fn push_blocks(mut docx: Docx, blocks: &[Block]) -> Docx {
    for block in blocks {
        docx = match block {
            Block::Heading(level, text) => docx.add_paragraph(
                Paragraph::new().add_run(Run::new().add_text(text.as_str()).bold().size(heading_size(*level))),
            ),
            Block::Paragraph(text) => {
                docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(text.as_str()).size(20)))
            }
            Block::ListItem(text) => docx.add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text(format!("\u{2022} {}", text)).size(20)),
            ),
        };
    }
    docx
}

fn section_heading(docx: Docx, text: &str) -> Docx {
    docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(text).bold().size(22)))
}

pub fn render_docx(data: &ExportData, out_path: &Path) -> Result<()> {
    let mut docx = Docx::new();

    // Header
    docx = docx.add_paragraph(
        Paragraph::new()
            .add_run(Run::new().add_text(data.context_type.to_uppercase()).size(16)),
    );
    docx = docx.add_paragraph(
        Paragraph::new().add_run(Run::new().add_text(data.title.as_str()).bold().size(36)),
    );
    docx = docx.add_paragraph(
        Paragraph::new()
            .add_run(Run::new().add_text(data.created_at.as_str()).italic().size(18))
            .align(AlignmentType::Left),
    );
    docx = docx.add_paragraph(Paragraph::new());

    // Summary
    docx = section_heading(docx, "Summary");
    docx = match &data.summary_markdown {
        Some(md) => push_blocks(docx, &parse_markdown_blocks(md)),
        None => docx.add_paragraph(
            Paragraph::new().add_run(Run::new().add_text("No summary generated for this session.").italic().size(20)),
        ),
    };
    docx = docx.add_paragraph(Paragraph::new());

    // Notes
    if let Some(notes) = &data.user_notes_markdown {
        docx = section_heading(docx, "Notes");
        docx = push_blocks(docx, &parse_markdown_blocks(notes));
        docx = docx.add_paragraph(Paragraph::new());
    }

    // Attachments
    if !data.attachments.is_empty() {
        docx = section_heading(docx, "Attached Files");
        for att in &data.attachments {
            docx = docx.add_paragraph(
                Paragraph::new().add_run(
                    Run::new()
                        .add_text(format!("{} ({})", att.file_name, att.file_type))
                        .bold()
                        .size(20),
                ),
            );
            docx = match &att.extracted_text {
                Some(text) if !text.trim().is_empty() => {
                    let truncated: String = text.chars().take(4000).collect();
                    docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(truncated).size(18)))
                }
                _ => docx.add_paragraph(
                    Paragraph::new().add_run(Run::new().add_text("No text extracted from this file.").italic().size(18)),
                ),
            };
        }
        docx = docx.add_paragraph(Paragraph::new());
    }

    // Transcript
    docx = section_heading(docx, "Transcript");
    if data.transcript_rows.is_empty() {
        docx = docx.add_paragraph(
            Paragraph::new().add_run(Run::new().add_text("No transcript available.").italic().size(20)),
        );
    } else {
        let mut last_speaker: Option<&str> = None;
        for (text, speaker, start) in &data.transcript_rows {
            if !is_source_label(speaker) && speaker.as_deref() != last_speaker {
                let label = speaker.as_deref().unwrap();
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(label).bold().size(18)));
                last_speaker = speaker.as_deref();
            }
            docx = docx.add_paragraph(
                Paragraph::new().add_run(
                    Run::new()
                        .add_text(format!("[{}] {}", format_timestamp(*start), text))
                        .size(18),
                ),
            );
        }
    }

    let file = File::create(out_path).map_err(|e| anyhow!("Failed to create {}: {}", out_path.display(), e))?;
    docx.build()
        .pack(file)
        .map_err(|e| anyhow!("Failed to write DOCX: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organization::export::AttachmentExport;
    use std::io::Read;

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
    fn renders_a_valid_docx_zip_with_expected_content() {
        let tmp = std::env::temp_dir().join(format!("synth-docx-test-{}.docx", std::process::id()));
        render_docx(&sample_data(), &tmp).expect("DOCX rendering should succeed");

        // A .docx is a zip archive; confirm it's actually openable as one
        // and contains the mandatory Word document part, not just bytes on disk.
        let file = File::open(&tmp).unwrap();
        let mut archive = zip::ZipArchive::new(file).expect("docx must be a valid zip archive");
        let mut doc_xml = String::new();
        archive
            .by_name("word/document.xml")
            .expect("docx must contain word/document.xml")
            .read_to_string(&mut doc_xml)
            .unwrap();

        assert!(doc_xml.contains("Team Sync"));
        assert!(doc_xml.contains("Ship it"));
        assert!(doc_xml.contains("Speaker 1"));
        assert!(doc_xml.contains("Budget review"));

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn renders_empty_session_without_error() {
        let tmp = std::env::temp_dir().join(format!("synth-docx-empty-test-{}.docx", std::process::id()));
        let data = ExportData {
            title: "Empty".to_string(),
            context_type: "custom".to_string(),
            created_at: "2026-01-01".to_string(),
            summary_markdown: None,
            transcript_rows: vec![],
            user_notes_markdown: None,
            attachments: vec![],
        };
        render_docx(&data, &tmp).expect("DOCX rendering should handle empty content");
        assert!(tmp.exists());
        std::fs::remove_file(&tmp).ok();
    }
}
