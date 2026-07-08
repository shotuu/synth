use log::{error as log_error, info as log_info, warn as log_warn};
use tauri::{AppHandle, Manager, Runtime};

use super::compress::{compress_to_opus, TARGET_BITRATE_KBPS};
use super::storage::{compute_stats, list_session_audio, suggested_cleanup, SessionAudioRow, StorageStats};
use crate::database::repositories::attachment::AttachmentsRepository;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;

#[tauri::command]
pub async fn api_get_storage_stats<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<StorageStats, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    compute_stats(state.db_manager.pool(), &app_data_dir)
        .await
        .map_err(|e| format!("Failed to compute storage stats: {}", e))
}

#[tauri::command]
pub async fn api_list_session_audio(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SessionAudioRow>, String> {
    list_session_audio(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to list session audio: {}", e))
}

#[tauri::command]
pub async fn api_suggested_cleanup(
    state: tauri::State<'_, AppState>,
    older_than_days: i64,
) -> Result<Vec<SessionAudioRow>, String> {
    suggested_cleanup(state.db_manager.pool(), older_than_days)
        .await
        .map_err(|e| format!("Failed to compute suggested cleanup: {}", e))
}

/// Delete the raw audio for each session but keep everything else
/// (transcript, summary, attachments) -- the reversible-in-spirit action
/// from PROJECT_BRIEF.md §4.
#[tauri::command]
pub async fn api_delete_session_audio(
    state: tauri::State<'_, AppState>,
    meeting_ids: Vec<String>,
) -> Result<usize, String> {
    let pool = state.db_manager.pool();
    let mut deleted = 0usize;

    for meeting_id in &meeting_ids {
        let path: Option<String> =
            sqlx::query_scalar("SELECT storage_path FROM note_audio WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("Failed to look up audio for {}: {}", meeting_id, e))?
                .flatten();

        if let Some(path) = &path {
            if let Err(e) = std::fs::remove_file(path) {
                log_warn!("Could not remove audio file {} for {}: {}", path, meeting_id, e);
            }
        }

        let result = sqlx::query(
            "UPDATE note_audio SET storage_path = NULL, retained = 0, current_size_bytes = NULL WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update audio record for {}: {}", meeting_id, e))?;

        if result.rows_affected() > 0 {
            deleted += 1;
        }
    }

    log_info!("Deleted audio for {}/{} sessions", deleted, meeting_ids.len());
    Ok(deleted)
}

/// Re-encode each session's retained audio to low-bitrate Opus, replacing
/// the original file and updating size/bitrate bookkeeping. Best-effort
/// per session -- one failure (e.g. missing ffmpeg) doesn't abort the batch.
#[tauri::command]
pub async fn api_compress_session_audio(
    state: tauri::State<'_, AppState>,
    meeting_ids: Vec<String>,
) -> Result<usize, String> {
    let pool = state.db_manager.pool();
    let mut compressed = 0usize;

    for meeting_id in &meeting_ids {
        let path: Option<String> =
            sqlx::query_scalar("SELECT storage_path FROM note_audio WHERE meeting_id = ? AND retained = 1")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("Failed to look up audio for {}: {}", meeting_id, e))?
                .flatten();

        let Some(path) = path else {
            log_warn!("No retained audio to compress for {}", meeting_id);
            continue;
        };

        let source = std::path::PathBuf::from(&path);
        let meeting_id_owned = meeting_id.clone();
        let result = tokio::task::spawn_blocking(move || compress_to_opus(&source))
            .await
            .map_err(|e| format!("Compression task failed for {}: {}", meeting_id_owned, e))?;

        match result {
            Ok((new_path, new_size)) => {
                if let Err(e) = std::fs::remove_file(&path) {
                    log_warn!("Could not remove pre-compression file {}: {}", path, e);
                }
                let now = chrono::Utc::now().to_rfc3339();
                sqlx::query(
                    "UPDATE note_audio SET storage_path = ?, current_size_bytes = ?, bitrate_kbps = ?, last_compressed_at = ?
                     WHERE meeting_id = ?",
                )
                .bind(new_path.to_string_lossy().as_ref())
                .bind(new_size as i64)
                .bind(TARGET_BITRATE_KBPS)
                .bind(&now)
                .bind(meeting_id)
                .execute(pool)
                .await
                .map_err(|e| format!("Failed to update audio record for {}: {}", meeting_id, e))?;
                compressed += 1;
            }
            Err(e) => log_error!("Failed to compress audio for {}: {}", meeting_id, e),
        }
    }

    log_info!("Compressed audio for {}/{} sessions", compressed, meeting_ids.len());
    Ok(compressed)
}

/// Fully delete sessions: transcript, summary, attachments, audio, notes,
/// action items, and every file on disk. The destructive option -- kept
/// clearly distinct in the UI from delete-audio-only.
#[tauri::command]
pub async fn api_delete_sessions(
    state: tauri::State<'_, AppState>,
    meeting_ids: Vec<String>,
) -> Result<usize, String> {
    let pool = state.db_manager.pool();
    let mut deleted = 0usize;

    for meeting_id in &meeting_ids {
        // Gather file paths before the DB rows referencing them are gone
        let attachments = AttachmentsRepository::list_with_text(pool, meeting_id)
            .await
            .unwrap_or_default();
        let audio_path: Option<String> =
            sqlx::query_scalar("SELECT storage_path FROM note_audio WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .flatten();
        let folder_path: Option<String> =
            sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();

        match MeetingsRepository::delete_meeting(pool, meeting_id).await {
            Ok(true) => {
                for attachment in &attachments {
                    let _ = std::fs::remove_file(&attachment.storage_path);
                }
                if let Some(path) = &audio_path {
                    let _ = std::fs::remove_file(path);
                }
                // The recording folder holds the audio file plus
                // transcripts.json/metadata.json/checkpoints -- safe to
                // remove entirely once the session itself is gone.
                if let Some(folder) = &folder_path {
                    if let Err(e) = std::fs::remove_dir_all(folder) {
                        log_warn!("Could not remove recording folder {} for {}: {}", folder, meeting_id, e);
                    }
                }
                deleted += 1;
            }
            Ok(false) => log_warn!("Session {} not found for deletion", meeting_id),
            Err(e) => log_error!("Failed to delete session {}: {}", meeting_id, e),
        }
    }

    log_info!("Deleted {}/{} sessions entirely", deleted, meeting_ids.len());
    Ok(deleted)
}
