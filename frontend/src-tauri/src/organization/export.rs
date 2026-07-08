/// Session export: HTML (self-contained, the practical stand-in for a share
/// link — see the module doc on export_commands.rs), plus PDF and DOCX for
/// use in other tools. All three formats share one data-gathering pass
/// (fetch_export_data) and, for PDF/DOCX, one simplified block model
/// (parse_markdown_blocks) since those renderers need structured elements
/// rather than raw markdown/HTML.
use pulldown_cmark::{html, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use sqlx::SqlitePool;

use crate::database::repositories::attachment::AttachmentsRepository;
use crate::database::repositories::meeting_notes::MeetingNotesRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;

pub struct SpeakerColor {
    pub bg: &'static str,
    pub fg: &'static str,
}

/// Mirrors frontend/src/lib/speaker-colors.ts so an exported file's speaker
/// colors match what the user sees in the app.
pub const PALETTE: &[SpeakerColor] = &[
    SpeakerColor { bg: "#dbeafe", fg: "#1e40af" },
    SpeakerColor { bg: "#d1fae5", fg: "#065f46" },
    SpeakerColor { bg: "#fef3c7", fg: "#92400e" },
    SpeakerColor { bg: "#ede9fe", fg: "#5b21b6" },
    SpeakerColor { bg: "#ffe4e6", fg: "#9f1239" },
    SpeakerColor { bg: "#cffafe", fg: "#155e75" },
    SpeakerColor { bg: "#ecfccb", fg: "#3f6212" },
    SpeakerColor { bg: "#fed7aa", fg: "#9a3412" },
];

pub fn speaker_color(label: &str) -> &'static SpeakerColor {
    let mut hash: i32 = 0;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as i32);
    }
    &PALETTE[(hash.unsigned_abs() as usize) % PALETTE.len()]
}

pub fn is_source_label(speaker: &Option<String>) -> bool {
    matches!(speaker.as_deref(), None | Some("mic") | Some("system"))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_to_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, cmark_options());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn cmark_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

pub fn format_timestamp(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) if s >= 0.0 => {
            let total = s.floor() as u64;
            format!("{:02}:{:02}", total / 60, total % 60)
        }
        _ => "--:--".to_string(),
    }
}

/// A deliberately simplified view of markdown for renderers (PDF, DOCX)
/// that need structured elements rather than raw markup. Inline formatting
/// (bold/italic/links) is flattened to plain text -- summaries are
/// heading/paragraph/list heavy, and preserving that structure while
/// dropping inline styling is a reasonable trade for keeping the PDF/DOCX
/// renderers simple.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Paragraph(String),
    ListItem(String),
}

pub fn parse_markdown_blocks(markdown: &str) -> Vec<Block> {
    let parser = Parser::new_ext(markdown, cmark_options());
    let mut blocks = Vec::new();
    let mut buffer = String::new();
    let mut heading_level: Option<u8> = None;
    let mut in_item = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(heading_level_to_u8(level));
                buffer.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level.take() {
                    let text = buffer.trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block::Heading(level, text));
                    }
                }
                buffer.clear();
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                buffer.clear();
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                let text = buffer.trim().to_string();
                if !text.is_empty() {
                    blocks.push(Block::ListItem(text));
                }
                buffer.clear();
            }
            Event::Start(Tag::Paragraph) => {
                buffer.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_item {
                    let text = buffer.trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block::Paragraph(text));
                    }
                }
                buffer.clear();
            }
            Event::Text(t) => buffer.push_str(&t),
            Event::Code(t) => buffer.push_str(&t),
            Event::SoftBreak | Event::HardBreak => buffer.push(' '),
            _ => {}
        }
    }

    blocks
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub struct AttachmentExport {
    pub file_name: String,
    pub file_type: String,
    pub extracted_text: Option<String>,
}

pub struct ExportData {
    pub title: String,
    pub context_type: String,
    pub created_at: String,
    pub summary_markdown: Option<String>,
    pub transcript_rows: Vec<(String, Option<String>, Option<f64>)>,
    pub user_notes_markdown: Option<String>,
    pub attachments: Vec<AttachmentExport>,
}

/// One data-gathering pass shared by every export format.
pub async fn fetch_export_data(
    pool: &SqlitePool,
    meeting_id: &str,
    title: &str,
    context_type: &str,
    created_at: &str,
) -> Result<ExportData, sqlx::Error> {
    let summary_markdown = SummaryProcessesRepository::get_summary_data_for_meeting(pool, meeting_id)
        .await?
        .and_then(|p| p.result)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("markdown").and_then(|m| m.as_str()).map(str::to_string));

    let transcript_rows: Vec<(String, Option<String>, Option<f64>)> = sqlx::query_as(
        "SELECT transcript, speaker, audio_start_time FROM transcripts
         WHERE meeting_id = ? ORDER BY COALESCE(audio_start_time, 1e18), timestamp",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;

    let user_notes_markdown = MeetingNotesRepository::get(pool, meeting_id)
        .await?
        .and_then(|n| n.notes_markdown)
        .filter(|md| !md.trim().is_empty());

    let attachments = AttachmentsRepository::list_with_text(pool, meeting_id)
        .await?
        .into_iter()
        .map(|a| AttachmentExport {
            file_name: a.file_name,
            file_type: a.file_type,
            extracted_text: a.extracted_text,
        })
        .collect();

    Ok(ExportData {
        title: title.to_string(),
        context_type: context_type.to_string(),
        created_at: created_at.to_string(),
        summary_markdown,
        transcript_rows,
        user_notes_markdown,
        attachments,
    })
}

pub fn render_html(data: &ExportData) -> String {
    let summary_html = data
        .summary_markdown
        .as_deref()
        .map(markdown_to_html)
        .unwrap_or_else(|| "<p><em>No summary generated for this session.</em></p>".to_string());

    let mut transcript_html = String::new();
    if data.transcript_rows.is_empty() {
        transcript_html.push_str("<p><em>No transcript available.</em></p>");
    }
    let mut last_speaker: Option<&str> = None;
    for (text, speaker, start) in &data.transcript_rows {
        let is_source = is_source_label(speaker);
        if !is_source && speaker.as_deref() != last_speaker {
            let label = speaker.as_deref().unwrap();
            let color = speaker_color(label);
            transcript_html.push_str(&format!(
                "<div class=\"speaker-chip\" style=\"background:{};color:{}\">{}</div>\n",
                color.bg,
                color.fg,
                escape_html(label)
            ));
            last_speaker = speaker.as_deref();
        }
        transcript_html.push_str(&format!(
            "<p class=\"segment\"><span class=\"ts\">[{}]</span> {}</p>\n",
            format_timestamp(*start),
            escape_html(text)
        ));
    }

    let notes_section = data
        .user_notes_markdown
        .as_deref()
        .map(|md| {
            format!(
                "<section><h2>Notes</h2><div class=\"prose\">{}</div></section>",
                markdown_to_html(md)
            )
        })
        .unwrap_or_default();

    let attachments_section = if data.attachments.is_empty() {
        String::new()
    } else {
        let mut inner = String::from("<section><h2>Attached Files</h2>");
        for att in &data.attachments {
            inner.push_str(&format!(
                "<div class=\"attachment\"><h3>{} <span class=\"filetype\">({})</span></h3>",
                escape_html(&att.file_name),
                escape_html(&att.file_type)
            ));
            match &att.extracted_text {
                Some(text) if !text.trim().is_empty() => {
                    inner.push_str(&format!("<pre class=\"attachment-text\">{}</pre>", escape_html(text)));
                }
                _ => inner.push_str("<p class=\"attachment-empty\"><em>No text extracted from this file.</em></p>"),
            }
            inner.push_str("</div>");
        }
        inner.push_str("</section>");
        inner
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ color-scheme: light; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
          max-width: 760px; margin: 0 auto; padding: 2.5rem 1.5rem;
          color: #1f2937; line-height: 1.6; background: #fff; }}
  header {{ margin-bottom: 2rem; border-bottom: 1px solid #e5e7eb; padding-bottom: 1rem; }}
  h1 {{ font-size: 1.75rem; margin: 0 0 0.25rem; }}
  .meta {{ color: #6b7280; font-size: 0.875rem; }}
  .badge {{ display: inline-block; padding: 0.15rem 0.6rem; border-radius: 999px;
            background: #eef2ff; color: #4338ca; font-size: 0.75rem; font-weight: 600;
            text-transform: capitalize; margin-right: 0.5rem; }}
  section {{ margin: 2rem 0; }}
  h2 {{ font-size: 1.1rem; text-transform: uppercase; letter-spacing: 0.03em;
        color: #6b7280; margin-bottom: 0.75rem; }}
  h3 {{ font-size: 0.95rem; color: #374151; margin: 1rem 0 0.35rem; }}
  .prose :is(h1,h2,h3) {{ font-size: 1rem; text-transform: none; letter-spacing: normal; color: #1f2937; }}
  .prose ul {{ padding-left: 1.25rem; }}
  .segment {{ margin: 0.35rem 0; }}
  .ts {{ color: #9ca3af; font-size: 0.75rem; margin-right: 0.4rem; }}
  .speaker-chip {{ display: inline-block; padding: 0.1rem 0.5rem; border-radius: 4px;
                    font-size: 0.75rem; font-weight: 600; margin: 0.75rem 0 0.15rem; }}
  .filetype {{ color: #9ca3af; font-weight: 400; font-size: 0.8rem; }}
  .attachment-text {{ white-space: pre-wrap; font-family: inherit; font-size: 0.85rem;
                       background: #f9fafb; border: 1px solid #f3f4f6; border-radius: 6px;
                       padding: 0.75rem; max-height: 20rem; overflow-y: auto; }}
  .attachment-empty {{ color: #9ca3af; font-size: 0.85rem; }}
  footer {{ margin-top: 3rem; padding-top: 1rem; border-top: 1px solid #e5e7eb;
            color: #9ca3af; font-size: 0.75rem; }}
</style>
</head>
<body>
<header>
  <span class="badge">{context_type}</span>
  <h1>{title}</h1>
  <div class="meta">{created_at}</div>
</header>
<section><h2>Summary</h2><div class="prose">{summary_html}</div></section>
{notes_section}
{attachments_section}
<section><h2>Transcript</h2>{transcript_html}</section>
<footer>Exported from Synth &mdash; a local-only session, shared as a static page.</footer>
</body>
</html>
"#,
        title = escape_html(&data.title),
        context_type = escape_html(&data.context_type),
        created_at = escape_html(&data.created_at),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> ExportData {
        ExportData {
            title: "Team Sync <script>".to_string(),
            context_type: "meeting".to_string(),
            created_at: "2026-01-01T10:00:00Z".to_string(),
            summary_markdown: Some("# Team Sync\n## Key Decisions\n- Ship it".to_string()),
            transcript_rows: vec![
                ("Hello everyone.".to_string(), Some("Speaker 1".to_string()), Some(0.0)),
                ("Hi, thanks for joining.".to_string(), Some("Speaker 2".to_string()), Some(3.5)),
                ("mic-only fallback line".to_string(), Some("mic".to_string()), Some(10.0)),
            ],
            user_notes_markdown: Some("My private takeaway".to_string()),
            attachments: vec![],
        }
    }

    #[test]
    fn renders_summary_transcript_and_notes() {
        let html = render_html(&sample_data());

        assert!(html.contains("Team Sync &lt;script&gt;"), "title must be escaped");
        assert!(!html.contains("<script>"), "must not allow script injection from title");
        assert!(html.contains("Ship it"));
        assert!(html.contains("Speaker 1"));
        assert!(html.contains("Speaker 2"));
        assert!(html.contains("Hello everyone."));
        assert!(html.contains("My private takeaway"));
        assert!(!html.contains(">mic<"));
        assert!(html.contains("[00:00]"));
        assert!(html.contains("[00:03]"));
    }

    #[test]
    fn handles_missing_summary_and_notes_gracefully() {
        let mut data = sample_data();
        data.summary_markdown = None;
        data.user_notes_markdown = None;
        data.transcript_rows.clear();

        let html = render_html(&data);
        assert!(html.contains("No summary generated"));
        assert!(html.contains("No transcript available"));
        assert!(!html.contains("<section><h2>Notes</h2>"));
    }

    #[test]
    fn includes_attachments_with_extracted_text() {
        let mut data = sample_data();
        data.attachments.push(AttachmentExport {
            file_name: "slides.pdf".to_string(),
            file_type: "pdf".to_string(),
            extracted_text: Some("Q3 roadmap details".to_string()),
        });
        data.attachments.push(AttachmentExport {
            file_name: "photo.png".to_string(),
            file_type: "image".to_string(),
            extracted_text: None,
        });

        let html = render_html(&data);
        assert!(html.contains("slides.pdf"));
        assert!(html.contains("Q3 roadmap details"));
        assert!(html.contains("photo.png"));
        assert!(html.contains("No text extracted"));
    }

    #[test]
    fn parses_markdown_into_blocks() {
        let blocks = parse_markdown_blocks(
            "# Title\nIntro paragraph.\n## Section\n- item one\n- item two\n",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Heading(1, "Title".to_string()),
                Block::Paragraph("Intro paragraph.".to_string()),
                Block::Heading(2, "Section".to_string()),
                Block::ListItem("item one".to_string()),
                Block::ListItem("item two".to_string()),
            ]
        );
    }
}
