/// Static HTML export — the practical answer to PROJECT_BRIEF.md §12's
/// "share a session without an account," given this app has no server to
/// host a live share link on. Produces one self-contained HTML file
/// (transcript + summary + notes, inline CSS, no external requests) that
/// the user sends however they like.
use pulldown_cmark::{html, Options, Parser};
use sqlx::SqlitePool;

use crate::database::repositories::meeting_notes::MeetingNotesRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;

struct SpeakerColor {
    bg: &'static str,
    fg: &'static str,
}

/// Mirrors frontend/src/lib/speaker-colors.ts so an exported file's speaker
/// colors match what the user sees in the app.
const PALETTE: &[SpeakerColor] = &[
    SpeakerColor { bg: "#dbeafe", fg: "#1e40af" },
    SpeakerColor { bg: "#d1fae5", fg: "#065f46" },
    SpeakerColor { bg: "#fef3c7", fg: "#92400e" },
    SpeakerColor { bg: "#ede9fe", fg: "#5b21b6" },
    SpeakerColor { bg: "#ffe4e6", fg: "#9f1239" },
    SpeakerColor { bg: "#cffafe", fg: "#155e75" },
    SpeakerColor { bg: "#ecfccb", fg: "#3f6212" },
    SpeakerColor { bg: "#fed7aa", fg: "#9a3412" },
];

fn speaker_color(label: &str) -> &'static SpeakerColor {
    let mut hash: i32 = 0;
    for byte in label.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as i32);
    }
    &PALETTE[(hash.unsigned_abs() as usize) % PALETTE.len()]
}

fn is_source_label(speaker: &Option<String>) -> bool {
    matches!(speaker.as_deref(), None | Some("mic") | Some("system"))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn format_timestamp(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) if s >= 0.0 => {
            let total = s.floor() as u64;
            format!("{:02}:{:02}", total / 60, total % 60)
        }
        _ => "--:--".to_string(),
    }
}

pub struct ExportRow {
    pub text: String,
    pub speaker: Option<String>,
    pub audio_start_time: Option<f64>,
}

/// Assemble the export HTML. Kept separate from file I/O so it can be unit
/// tested without a filesystem or file-picker dialog.
pub async fn build_export_html(
    pool: &SqlitePool,
    meeting_id: &str,
    meeting_title: &str,
    context_type: &str,
    created_at: &str,
) -> Result<String, sqlx::Error> {
    let summary_markdown = SummaryProcessesRepository::get_summary_data_for_meeting(pool, meeting_id)
        .await?
        .and_then(|p| p.result)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("markdown").and_then(|m| m.as_str()).map(str::to_string));

    let rows: Vec<(String, Option<String>, Option<f64>)> = sqlx::query_as(
        "SELECT transcript, speaker, audio_start_time FROM transcripts
         WHERE meeting_id = ? ORDER BY COALESCE(audio_start_time, 1e18), timestamp",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;

    let user_notes = MeetingNotesRepository::get(pool, meeting_id)
        .await?
        .and_then(|n| n.notes_markdown)
        .filter(|md| !md.trim().is_empty());

    Ok(render_html(
        meeting_title,
        context_type,
        created_at,
        summary_markdown.as_deref(),
        &rows,
        user_notes.as_deref(),
    ))
}

fn render_html(
    title: &str,
    context_type: &str,
    created_at: &str,
    summary_markdown: Option<&str>,
    transcript_rows: &[(String, Option<String>, Option<f64>)],
    user_notes_markdown: Option<&str>,
) -> String {
    let summary_html = summary_markdown
        .map(markdown_to_html)
        .unwrap_or_else(|| "<p><em>No summary generated for this session.</em></p>".to_string());

    let mut transcript_html = String::new();
    if transcript_rows.is_empty() {
        transcript_html.push_str("<p><em>No transcript available.</em></p>");
    }
    let mut last_speaker: Option<&str> = None;
    for (text, speaker, start) in transcript_rows {
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

    let notes_section = user_notes_markdown
        .map(|md| {
            format!(
                "<section><h2>Notes</h2><div class=\"prose\">{}</div></section>",
                markdown_to_html(md)
            )
        })
        .unwrap_or_default();

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
  .prose :is(h1,h2,h3) {{ font-size: 1rem; text-transform: none; letter-spacing: normal; color: #1f2937; }}
  .prose ul {{ padding-left: 1.25rem; }}
  .segment {{ margin: 0.35rem 0; }}
  .ts {{ color: #9ca3af; font-size: 0.75rem; margin-right: 0.4rem; }}
  .speaker-chip {{ display: inline-block; padding: 0.1rem 0.5rem; border-radius: 4px;
                    font-size: 0.75rem; font-weight: 600; margin: 0.75rem 0 0.15rem; }}
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
<section><h2>Transcript</h2>{transcript_html}</section>
<footer>Exported from Synth &mdash; a local-only session, shared as a static page.</footer>
</body>
</html>
"#,
        title = escape_html(title),
        context_type = escape_html(context_type),
        created_at = escape_html(created_at),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_summary_transcript_and_notes() {
        let rows = vec![
            ("Hello everyone.".to_string(), Some("Speaker 1".to_string()), Some(0.0)),
            ("Hi, thanks for joining.".to_string(), Some("Speaker 2".to_string()), Some(3.5)),
            ("mic-only fallback line".to_string(), Some("mic".to_string()), Some(10.0)),
        ];
        let html = render_html(
            "Team Sync <script>",
            "meeting",
            "2026-01-01T10:00:00Z",
            Some("# Team Sync\n## Key Decisions\n- Ship it"),
            &rows,
            Some("My private takeaway"),
        );

        assert!(html.contains("Team Sync &lt;script&gt;"), "title must be escaped");
        assert!(!html.contains("<script>"), "must not allow script injection from title");
        assert!(html.contains("Ship it"));
        assert!(html.contains("Speaker 1"));
        assert!(html.contains("Speaker 2"));
        assert!(html.contains("Hello everyone."));
        assert!(html.contains("My private takeaway"));
        // 'mic' is a source label, not a speaker chip
        assert!(!html.contains(">mic<"));
        assert!(html.contains("[00:00]"));
        assert!(html.contains("[00:03]"));
    }

    #[test]
    fn handles_missing_summary_and_notes_gracefully() {
        let html = render_html("Empty Session", "custom", "2026-01-01", None, &[], None);
        assert!(html.contains("No summary generated"));
        assert!(html.contains("No transcript available"));
        assert!(!html.contains("<section><h2>Notes</h2>"));
    }
}
