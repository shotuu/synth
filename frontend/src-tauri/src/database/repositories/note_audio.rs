use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NoteAudio {
    pub meeting_id: String,
    pub storage_path: Option<String>,
    pub origin: String,
    pub original_file_name: Option<String>,
    pub format: Option<String>,
    pub bitrate_kbps: Option<i64>,
    pub original_size_bytes: Option<i64>,
    pub current_size_bytes: Option<i64>,
    pub retained: bool,
    pub last_compressed_at: Option<String>,
    pub created_at: String,
}

pub struct NoteAudioRepository;

impl NoteAudioRepository {
    pub async fn get(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<NoteAudio>, sqlx::Error> {
        sqlx::query_as::<_, NoteAudio>("SELECT * FROM note_audio WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    /// Record the audio file backing a meeting. Called after a recording is
    /// saved or an import completes; best-effort by design — callers must not
    /// fail the meeting save if this errors.
    pub async fn upsert_from_file(
        pool: &SqlitePool,
        meeting_id: &str,
        audio_path: &Path,
        origin: &str,
        original_file_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let size_bytes = std::fs::metadata(audio_path).map(|m| m.len() as i64).ok();
        let format = audio_path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO note_audio (meeting_id, storage_path, origin, original_file_name, format,
                                     original_size_bytes, current_size_bytes, retained, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET
                storage_path = excluded.storage_path,
                format = excluded.format,
                current_size_bytes = excluded.current_size_bytes,
                retained = 1",
        )
        .bind(meeting_id)
        .bind(audio_path.to_string_lossy().as_ref())
        .bind(origin)
        .bind(original_file_name)
        .bind(&format)
        .bind(size_bytes)
        .bind(size_bytes)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(())
    }
}
