use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NoteAttachment {
    pub id: String,
    pub meeting_id: String,
    pub file_name: String,
    pub file_type: String,
    pub storage_path: String,
    pub extracted_text: Option<String>,
    pub uploaded_at: String,
}

/// Lightweight listing row: omits extracted_text so the UI can list
/// attachments without shipping whole documents over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NoteAttachmentInfo {
    pub id: String,
    pub meeting_id: String,
    pub file_name: String,
    pub file_type: String,
    pub storage_path: String,
    pub has_extracted_text: bool,
    pub uploaded_at: String,
}

pub struct AttachmentsRepository;

impl AttachmentsRepository {
    pub async fn insert(
        pool: &SqlitePool,
        meeting_id: &str,
        file_name: &str,
        file_type: &str,
        storage_path: &str,
        extracted_text: Option<&str>,
    ) -> Result<NoteAttachment, sqlx::Error> {
        let id = format!("attachment-{}", Uuid::new_v4());
        let uploaded_at = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO note_attachments (id, meeting_id, file_name, file_type, storage_path, extracted_text, uploaded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(meeting_id)
        .bind(file_name)
        .bind(file_type)
        .bind(storage_path)
        .bind(extracted_text)
        .bind(&uploaded_at)
        .execute(pool)
        .await?;

        Ok(NoteAttachment {
            id,
            meeting_id: meeting_id.to_string(),
            file_name: file_name.to_string(),
            file_type: file_type.to_string(),
            storage_path: storage_path.to_string(),
            extracted_text: extracted_text.map(|s| s.to_string()),
            uploaded_at,
        })
    }

    pub async fn list_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<NoteAttachmentInfo>, sqlx::Error> {
        sqlx::query_as::<_, NoteAttachmentInfo>(
            "SELECT id, meeting_id, file_name, file_type, storage_path,
                    extracted_text IS NOT NULL AS has_extracted_text, uploaded_at
             FROM note_attachments WHERE meeting_id = ? ORDER BY uploaded_at ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    /// Full rows including extracted text, for the context assembler.
    pub async fn list_with_text(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<NoteAttachment>, sqlx::Error> {
        sqlx::query_as::<_, NoteAttachment>(
            "SELECT id, meeting_id, file_name, file_type, storage_path, extracted_text, uploaded_at
             FROM note_attachments WHERE meeting_id = ? ORDER BY uploaded_at ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    pub async fn get(
        pool: &SqlitePool,
        attachment_id: &str,
    ) -> Result<Option<NoteAttachment>, sqlx::Error> {
        sqlx::query_as::<_, NoteAttachment>(
            "SELECT id, meeting_id, file_name, file_type, storage_path, extracted_text, uploaded_at
             FROM note_attachments WHERE id = ?",
        )
        .bind(attachment_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn delete(pool: &SqlitePool, attachment_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM note_attachments WHERE id = ?")
            .bind(attachment_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
