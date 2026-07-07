/// Context assembler (PROJECT_BRIEF.md §5): gathers every available source
/// for a session — transcript, attachment text, the user's own notes — into
/// one labeled structure. Phase 3 summarization consumes this; the UI uses
/// sources_used to show what a summary drew on.
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::database::repositories::attachment::AttachmentsRepository;
use crate::database::repositories::meeting_notes::MeetingNotesRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentContext {
    pub file_name: String,
    pub file_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub meeting_id: String,
    /// Concatenated transcript segments in playback order, with speaker
    /// labels once diarization lands (Phase 4).
    pub transcript_text: Option<String>,
    pub attachments: Vec<AttachmentContext>,
    pub user_notes_markdown: Option<String>,
    /// Which of 'transcript' / 'attachments' / 'user_notes' are present —
    /// stored alongside generated summaries as sources_used.
    pub sources_used: Vec<String>,
}

impl SessionContext {
    /// Render the assembled sources as a single prompt block whose sections
    /// are explicitly labeled by origin, so the model can reconcile rather
    /// than repeat (brief §5).
    pub fn to_prompt_text(&self) -> String {
        let mut out = String::new();

        if let Some(transcript) = &self.transcript_text {
            out.push_str("=== RECORDING TRANSCRIPT ===\n");
            out.push_str(transcript);
            out.push_str("\n\n");
        }

        for att in &self.attachments {
            out.push_str(&format!(
                "=== UPLOADED FILE: {} ({}) ===\n",
                att.file_name, att.file_type
            ));
            out.push_str(&att.text);
            out.push_str("\n\n");
        }

        if let Some(notes) = &self.user_notes_markdown {
            out.push_str("=== USER'S OWN NOTES ===\n");
            out.push_str(notes);
            out.push('\n');
        }

        out
    }
}

pub async fn assemble_context(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<SessionContext, sqlx::Error> {
    // Transcript segments in playback order. speaker is 'mic'/'system' today;
    // real speaker labels arrive with diarization in Phase 4.
    let segments: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT transcript, speaker FROM transcripts
         WHERE meeting_id = ?
         ORDER BY COALESCE(audio_start_time, 1e18), timestamp",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;

    let transcript_text = if segments.is_empty() {
        None
    } else {
        Some(
            segments
                .iter()
                .map(|(text, speaker)| match speaker {
                    Some(s) if !s.is_empty() => format!("[{}] {}", s, text),
                    _ => text.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };

    let attachments: Vec<AttachmentContext> = AttachmentsRepository::list_with_text(pool, meeting_id)
        .await?
        .into_iter()
        .filter_map(|a| {
            a.extracted_text.map(|text| AttachmentContext {
                file_name: a.file_name,
                file_type: a.file_type,
                text,
            })
        })
        .collect();

    let user_notes_markdown = MeetingNotesRepository::get(pool, meeting_id)
        .await?
        .and_then(|n| n.notes_markdown)
        .filter(|md| !md.trim().is_empty());

    let mut sources_used = Vec::new();
    if transcript_text.is_some() {
        sources_used.push("transcript".to_string());
    }
    if !attachments.is_empty() {
        sources_used.push("attachments".to_string());
    }
    if user_notes_markdown.is_some() {
        sources_used.push("user_notes".to_string());
    }

    Ok(SessionContext {
        meeting_id: meeting_id.to_string(),
        transcript_text,
        attachments,
        user_notes_markdown,
        sources_used,
    })
}
