use log::info as log_info;
use tauri::{AppHandle, Runtime};

use super::export::{fetch_export_data, render_html, ExportData};
use super::export_docx::render_docx;
use super::export_pdf::render_pdf;
use super::transcript_cleanup::clean_transcript;
use crate::state::AppState;

async fn maybe_clean_transcript<R: Runtime>(
    app: &AppHandle<R>,
    state: &tauri::State<'_, AppState>,
    data: &mut ExportData,
    ai_cleaned: bool,
) -> Result<(), String> {
    if !ai_cleaned || data.transcript_rows.is_empty() {
        return Ok(());
    }
    data.transcript_rows = clean_transcript(app, state.db_manager.pool(), &data.transcript_rows).await?;
    Ok(())
}

fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() { "session".to_string() } else { trimmed.to_string() }
}

async fn load_session_header(
    state: &tauri::State<'_, AppState>,
    meeting_id: &str,
) -> Result<(String, String, String), String> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT title, context_type, created_at FROM meetings WHERE id = ?",
    )
    .bind(meeting_id)
    .fetch_optional(state.db_manager.pool())
    .await
    .map_err(|e| format!("Failed to load session: {}", e))?
    .ok_or_else(|| format!("Session not found: {}", meeting_id))
}

/// Export a session as a self-contained HTML file the user can send
/// anywhere without the recipient needing an account or this app.
/// Returns the saved path, or None if the user cancelled the save dialog.
#[tauri::command]
pub async fn api_export_session_html<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    ai_cleaned: Option<bool>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (title, context_type, created_at) = load_session_header(&state, &meeting_id).await?;
    let mut data = fetch_export_data(state.db_manager.pool(), &meeting_id, &title, &context_type, &created_at)
        .await
        .map_err(|e| format!("Failed to assemble export: {}", e))?;
    maybe_clean_transcript(&app, &state, &mut data, ai_cleaned.unwrap_or(false)).await?;
    let html = render_html(&data);

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
    let path = path.into_path().map_err(|e| format!("Invalid save path: {}", e))?;

    std::fs::write(&path, html).map_err(|e| format!("Failed to write file: {}", e))?;
    log_info!("Exported session {} to {} (html)", meeting_id, path.display());

    Ok(Some(path.to_string_lossy().to_string()))
}

/// Export a session as a PDF.
#[tauri::command]
pub async fn api_export_session_pdf<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    ai_cleaned: Option<bool>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (title, context_type, created_at) = load_session_header(&state, &meeting_id).await?;
    let mut data = fetch_export_data(state.db_manager.pool(), &meeting_id, &title, &context_type, &created_at)
        .await
        .map_err(|e| format!("Failed to assemble export: {}", e))?;
    maybe_clean_transcript(&app, &state, &mut data, ai_cleaned.unwrap_or(false)).await?;

    let default_name = format!("{}.pdf", sanitize_filename(&title));
    let picked = app
        .dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_file_name(&default_name)
        .blocking_save_file();

    let Some(path) = picked else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| format!("Invalid save path: {}", e))?;

    let path_clone = path.clone();
    tokio::task::spawn_blocking(move || render_pdf(&data, &path_clone))
        .await
        .map_err(|e| format!("PDF render task failed: {}", e))?
        .map_err(|e| format!("Failed to render PDF: {}", e))?;

    log_info!("Exported session {} to {} (pdf)", meeting_id, path.display());
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Export a session as a DOCX.
#[tauri::command]
pub async fn api_export_session_docx<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    ai_cleaned: Option<bool>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (title, context_type, created_at) = load_session_header(&state, &meeting_id).await?;
    let mut data = fetch_export_data(state.db_manager.pool(), &meeting_id, &title, &context_type, &created_at)
        .await
        .map_err(|e| format!("Failed to assemble export: {}", e))?;
    maybe_clean_transcript(&app, &state, &mut data, ai_cleaned.unwrap_or(false)).await?;

    let default_name = format!("{}.docx", sanitize_filename(&title));
    let picked = app
        .dialog()
        .file()
        .add_filter("Word Document", &["docx"])
        .set_file_name(&default_name)
        .blocking_save_file();

    let Some(path) = picked else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| format!("Invalid save path: {}", e))?;

    let path_clone = path.clone();
    tokio::task::spawn_blocking(move || render_docx(&data, &path_clone))
        .await
        .map_err(|e| format!("DOCX render task failed: {}", e))?
        .map_err(|e| format!("Failed to render DOCX: {}", e))?;

    log_info!("Exported session {} to {} (docx)", meeting_id, path.display());
    Ok(Some(path.to_string_lossy().to_string()))
}
