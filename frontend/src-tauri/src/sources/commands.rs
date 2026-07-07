/// Tauri commands for multi-source sessions (PROJECT_BRIEF.md §5):
/// attachments, user notes, raw-audio metadata, and the assembled context.
use log::{error as log_error, info as log_info};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

use super::assembler::{assemble_context, SessionContext};
use super::extraction::{detect_file_type, extract_text};
use crate::database::repositories::attachment::{
    AttachmentsRepository, NoteAttachment, NoteAttachmentInfo,
};
use crate::database::repositories::meeting_notes::{MeetingNotes, MeetingNotesRepository};
use crate::database::repositories::note_audio::{NoteAudio, NoteAudioRepository};
use crate::state::AppState;

/// Root directory for attachment storage: <app_data>/attachments/<meeting_id>/
fn attachments_dir<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    Ok(base.join("attachments").join(meeting_id))
}

/// Copy the source file into the meeting's attachment dir, deduplicating the
/// file name if needed. Returns the destination path.
fn copy_into_storage(source: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed to create attachments dir: {}", e))?;

    let file_name = source
        .file_name()
        .ok_or_else(|| "Source path has no file name".to_string())?
        .to_string_lossy()
        .to_string();

    let mut dest = dest_dir.join(&file_name);
    let mut counter = 1;
    while dest.exists() {
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let ext = source
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        dest = dest_dir.join(format!("{} ({}){}", stem, counter, ext));
        counter += 1;
    }

    std::fs::copy(source, &dest).map_err(|e| format!("Failed to copy file: {}", e))?;
    Ok(dest)
}

async fn attach_one<R: Runtime>(
    app: &AppHandle<R>,
    state: &tauri::State<'_, AppState>,
    meeting_id: &str,
    source_path: &Path,
) -> Result<NoteAttachment, String> {
    if !source_path.is_file() {
        return Err(format!("Not a file: {}", source_path.display()));
    }

    let dest_dir = attachments_dir(app, meeting_id)?;
    let stored_path = copy_into_storage(source_path, &dest_dir)?;

    let file_type = detect_file_type(&stored_path);
    // Extraction can be slow for large PDFs; keep the async runtime free.
    let extraction_path = stored_path.clone();
    let extraction_type = file_type.clone();
    let extracted = tokio::task::spawn_blocking(move || {
        extract_text(&extraction_path, &extraction_type)
    })
    .await
    .map_err(|e| format!("Extraction task failed: {}", e))?;

    let file_name = stored_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let attachment = AttachmentsRepository::insert(
        state.db_manager.pool(),
        meeting_id,
        &file_name,
        &file_type,
        &stored_path.to_string_lossy(),
        extracted.as_deref(),
    )
    .await
    .map_err(|e| {
        // Don't leave an orphaned file behind if the DB insert failed
        let _ = std::fs::remove_file(&stored_path);
        format!("Failed to save attachment record: {}", e)
    })?;

    log_info!(
        "Attached '{}' ({}) to meeting {} (extracted text: {})",
        file_name,
        file_type,
        meeting_id,
        attachment.extracted_text.is_some()
    );

    Ok(attachment)
}

/// Attach files chosen via the OS file picker.
#[tauri::command]
pub async fn api_attach_files<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<NoteAttachment>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = app
        .dialog()
        .file()
        .add_filter(
            "Documents",
            &["pdf", "docx", "pptx", "txt", "md", "csv", "json", "png", "jpg", "jpeg"],
        )
        .blocking_pick_files();

    let Some(paths) = picked else {
        return Ok(Vec::new()); // user cancelled
    };

    let mut attached = Vec::new();
    for path in paths {
        let path = path
            .into_path()
            .map_err(|e| format!("Invalid file path: {}", e))?;
        match attach_one(&app, &state, &meeting_id, &path).await {
            Ok(a) => attached.push(a),
            Err(e) => log_error!("Failed to attach {}: {}", path.display(), e),
        }
    }
    Ok(attached)
}

/// Attach a file by path (drag-and-drop entry point).
#[tauri::command]
pub async fn api_attach_file_from_path<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    file_path: String,
) -> Result<NoteAttachment, String> {
    attach_one(&app, &state, &meeting_id, Path::new(&file_path)).await
}

#[tauri::command]
pub async fn api_list_attachments(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<NoteAttachmentInfo>, String> {
    AttachmentsRepository::list_for_meeting(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to list attachments: {}", e))
}

#[tauri::command]
pub async fn api_delete_attachment(
    state: tauri::State<'_, AppState>,
    attachment_id: String,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();

    let Some(attachment) = AttachmentsRepository::get(pool, &attachment_id)
        .await
        .map_err(|e| format!("Failed to look up attachment: {}", e))?
    else {
        return Ok(false);
    };

    let deleted = AttachmentsRepository::delete(pool, &attachment_id)
        .await
        .map_err(|e| format!("Failed to delete attachment record: {}", e))?;

    if deleted {
        if let Err(e) = std::fs::remove_file(&attachment.storage_path) {
            // Row is gone; a leftover file is a nuisance, not a failure
            log_error!(
                "Deleted attachment row but couldn't remove file {}: {}",
                attachment.storage_path,
                e
            );
        }
        log_info!("Deleted attachment {} ({})", attachment_id, attachment.file_name);
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn api_save_meeting_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    notes_markdown: Option<String>,
    notes_json: Option<String>,
) -> Result<(), String> {
    MeetingNotesRepository::upsert(
        state.db_manager.pool(),
        &meeting_id,
        notes_markdown.as_deref(),
        notes_json.as_deref(),
    )
    .await
    .map_err(|e| format!("Failed to save notes: {}", e))
}

#[tauri::command]
pub async fn api_get_meeting_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<MeetingNotes>, String> {
    MeetingNotesRepository::get(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load notes: {}", e))
}

#[tauri::command]
pub async fn api_get_note_audio(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<NoteAudio>, String> {
    NoteAudioRepository::get(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to load audio metadata: {}", e))
}

const VALID_CONTEXT_TYPES: &[&str] = &["meeting", "lecture", "discussion", "coffee_chat", "custom"];

#[tauri::command]
pub async fn api_get_context_type(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<String, String> {
    sqlx::query_scalar::<_, String>("SELECT context_type FROM meetings WHERE id = ?")
        .bind(&meeting_id)
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to load context type: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))
}

#[tauri::command]
pub async fn api_set_context_type(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    context_type: String,
) -> Result<(), String> {
    if !VALID_CONTEXT_TYPES.contains(&context_type.as_str()) {
        return Err(format!("Invalid context type: {}", context_type));
    }

    let result = sqlx::query("UPDATE meetings SET context_type = ? WHERE id = ?")
        .bind(&context_type)
        .bind(&meeting_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to set context type: {}", e))?;

    if result.rows_affected() == 0 {
        return Err(format!("Meeting not found: {}", meeting_id));
    }
    log_info!("Context type for {} set to '{}'", meeting_id, context_type);
    Ok(())
}

#[tauri::command]
pub async fn api_get_session_context(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<SessionContext, String> {
    assemble_context(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| format!("Failed to assemble session context: {}", e))
}
