use log::info as log_info;
use tauri::{AppHandle, Runtime};

use super::export::build_export_html;
use crate::state::AppState;

fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() { "session".to_string() } else { trimmed.to_string() }
}

/// Export a session as a self-contained HTML file the user can send
/// anywhere without the recipient needing an account or this app.
/// Returns the saved path, or None if the user cancelled the save dialog.
#[tauri::command]
pub async fn api_export_session_html<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let pool = state.db_manager.pool();

    let (title, context_type, created_at): (String, String, String) = sqlx::query_as(
        "SELECT title, context_type, created_at FROM meetings WHERE id = ?",
    )
    .bind(&meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to load session: {}", e))?
    .ok_or_else(|| format!("Session not found: {}", meeting_id))?;

    let html = build_export_html(pool, &meeting_id, &title, &context_type, &created_at)
        .await
        .map_err(|e| format!("Failed to assemble export: {}", e))?;

    let default_name = format!("{}.html", sanitize_filename(&title));
    let picked = app
        .dialog()
        .file()
        .add_filter("HTML", &["html"])
        .set_file_name(&default_name)
        .blocking_save_file();

    let Some(path) = picked else {
        return Ok(None); // user cancelled
    };
    let path = path
        .into_path()
        .map_err(|e| format!("Invalid save path: {}", e))?;

    std::fs::write(&path, html).map_err(|e| format!("Failed to write file: {}", e))?;
    log_info!("Exported session {} to {}", meeting_id, path.display());

    Ok(Some(path.to_string_lossy().to_string()))
}
