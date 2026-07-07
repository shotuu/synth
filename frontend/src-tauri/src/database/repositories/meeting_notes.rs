use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MeetingNotes {
    pub meeting_id: String,
    pub notes_markdown: Option<String>,
    pub notes_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct MeetingNotesRepository;

impl MeetingNotesRepository {
    pub async fn get(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingNotes>, sqlx::Error> {
        sqlx::query_as::<_, MeetingNotes>(
            "SELECT meeting_id, notes_markdown, notes_json, created_at, updated_at
             FROM meeting_notes WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn upsert(
        pool: &SqlitePool,
        meeting_id: &str,
        notes_markdown: Option<&str>,
        notes_json: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown, notes_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET
                notes_markdown = excluded.notes_markdown,
                notes_json = excluded.notes_json,
                updated_at = excluded.updated_at",
        )
        .bind(meeting_id)
        .bind(notes_markdown)
        .bind(notes_json)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(())
    }
}
