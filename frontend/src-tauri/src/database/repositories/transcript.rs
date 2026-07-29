use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        // Suggest a context type from the transcript (best-effort; the user
        // can override in the UI). Schema default stays 'meeting' when the
        // classifier is unsure.
        let full_text: String = transcripts
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(suggested) = crate::sources::classify::suggest_context_type(&full_text) {
            if let Err(e) = sqlx::query("UPDATE meetings SET context_type = ? WHERE id = ?")
                .bind(suggested)
                .bind(&meeting_id)
                .execute(pool)
                .await
            {
                error!("Failed to set suggested context_type for {}: {}", meeting_id, e);
            } else {
                info!("Auto-classified meeting {} as '{}'", meeting_id, suggested);
            }
        }

        // Track the raw recording in note_audio (best-effort: the meeting
        // save must succeed even if the audio file can't be located)
        if let Some(folder) = &folder_path {
            match crate::audio::retranscription::find_audio_file(std::path::Path::new(folder)) {
                Ok(audio_path) => {
                    if let Err(e) = super::note_audio::NoteAudioRepository::upsert_from_file(
                        pool,
                        &meeting_id,
                        &audio_path,
                        "recorded",
                        None,
                    )
                    .await
                    {
                        error!("Failed to record note_audio for {}: {}", meeting_id, e);
                    }
                }
                Err(e) => {
                    info!("No audio file found for meeting {} in {}: {}", meeting_id, folder, e);
                }
            }
        }

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                // `match_index` is a byte offset into `transcript_lower`, which is a
                // lowercased copy of `transcript` — lowercasing can change a character's
                // UTF-8 byte length (e.g. 'İ' -> "i̇"), so byte offsets from
                // `transcript_lower` aren't guaranteed to land on a char boundary in
                // `transcript`. Walk outward to the nearest valid boundaries before
                // slicing `transcript`, instead of slicing at raw byte offsets, which
                // panics on any transcript with multi-byte characters near a match.
                let raw_start = match_index.saturating_sub(100);
                let raw_end = (match_index + query.len() + 100).min(transcript.len());

                let mut start_index = raw_start;
                while start_index > 0 && !transcript.is_char_boundary(start_index) {
                    start_index -= 1;
                }
                let mut end_index = raw_end;
                while end_index < transcript.len() && !transcript.is_char_boundary(end_index) {
                    end_index += 1;
                }

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_match_context_does_not_panic_on_multibyte_chars_near_match() {
        // A match near a run of multi-byte characters (é is 2 bytes in UTF-8) used to
        // panic when start_index/end_index landed mid-character.
        let filler_a: String = std::iter::repeat('é').take(60).collect();
        let filler_b: String = std::iter::repeat('日').take(60).collect();
        let transcript = format!("{}keyword{}", filler_a, filler_b);

        let context = TranscriptsRepository::get_match_context(&transcript, "keyword");
        assert!(context.contains("keyword"));
    }

    #[test]
    fn get_match_context_falls_back_to_start_when_no_match() {
        let transcript = "hello world";
        let context = TranscriptsRepository::get_match_context(transcript, "missing");
        assert_eq!(context, "hello world");
    }
}
