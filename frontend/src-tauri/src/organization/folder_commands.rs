/// Tauri commands for the folder tree and per-session tags/folder
/// assignment (PROJECT_BRIEF.md §5 roadmap Phase 5, §8's Notion-style tree).
use log::info as log_info;

use crate::database::repositories::folder::{Folder, FoldersRepository};
use crate::state::AppState;

#[tauri::command]
pub async fn api_create_folder(
    state: tauri::State<'_, AppState>,
    parent_folder_id: Option<String>,
    name: String,
    icon: Option<String>,
) -> Result<Folder, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Folder name cannot be empty".to_string());
    }
    FoldersRepository::create(
        state.db_manager.pool(),
        parent_folder_id.as_deref(),
        name,
        icon.as_deref(),
    )
    .await
    .map_err(|e| format!("Failed to create folder: {}", e))
}

#[tauri::command]
pub async fn api_list_folders(state: tauri::State<'_, AppState>) -> Result<Vec<Folder>, String> {
    FoldersRepository::list_all(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to list folders: {}", e))
}

#[tauri::command]
pub async fn api_rename_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
    name: String,
) -> Result<bool, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Folder name cannot be empty".to_string());
    }
    FoldersRepository::rename(state.db_manager.pool(), &folder_id, name)
        .await
        .map_err(|e| format!("Failed to rename folder: {}", e))
}

#[tauri::command]
pub async fn api_set_folder_icon(
    state: tauri::State<'_, AppState>,
    folder_id: String,
    icon: Option<String>,
) -> Result<bool, String> {
    FoldersRepository::set_icon(state.db_manager.pool(), &folder_id, icon.as_deref())
        .await
        .map_err(|e| format!("Failed to set folder icon: {}", e))
}

#[tauri::command]
pub async fn api_move_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
    new_parent_id: Option<String>,
    new_sort_order: i64,
) -> Result<(), String> {
    FoldersRepository::move_folder(
        state.db_manager.pool(),
        &folder_id,
        new_parent_id.as_deref(),
        new_sort_order,
    )
    .await
    .map_err(|e| format!("Failed to move folder: {}", e))
}

#[tauri::command]
pub async fn api_delete_folder(
    state: tauri::State<'_, AppState>,
    folder_id: String,
) -> Result<(), String> {
    FoldersRepository::delete(state.db_manager.pool(), &folder_id)
        .await
        .map_err(|e| format!("Failed to delete folder: {}", e))?;
    log_info!("Deleted folder {} (contained sessions moved to no folder)", folder_id);
    Ok(())
}

/// Assign (or clear, with folder_id: None) a session's folder.
#[tauri::command]
pub async fn api_set_meeting_folder(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    folder_id: Option<String>,
) -> Result<(), String> {
    let result = sqlx::query("UPDATE meetings SET folder_id = ? WHERE id = ?")
        .bind(&folder_id)
        .bind(&meeting_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to set folder: {}", e))?;
    if result.rows_affected() == 0 {
        return Err(format!("Meeting not found: {}", meeting_id));
    }
    Ok(())
}

/// Replace a session's full tag set (JSON array of strings).
#[tauri::command]
pub async fn api_set_meeting_tags(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let cleaned: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;

    let result = sqlx::query("UPDATE meetings SET tags = ? WHERE id = ?")
        .bind(&json)
        .bind(&meeting_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to set tags: {}", e))?;
    if result.rows_affected() == 0 {
        return Err(format!("Meeting not found: {}", meeting_id));
    }
    Ok(())
}

/// All distinct tags currently in use, for filter dropdowns / autocomplete.
#[tauri::command]
pub async fn api_list_all_tags(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let rows: Vec<Option<String>> = sqlx::query_scalar("SELECT tags FROM meetings WHERE tags IS NOT NULL")
        .fetch_all(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to list tags: {}", e))?;

    let mut tags: Vec<String> = rows
        .into_iter()
        .flatten()
        .filter_map(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .flatten()
        .collect();
    tags.sort();
    tags.dedup();
    Ok(tags)
}
