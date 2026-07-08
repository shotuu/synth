use super::action_items::{list_action_items, ActionItemFilter, ActionItemWithContext};
use crate::state::AppState;

#[tauri::command]
pub async fn api_list_action_items(
    state: tauri::State<'_, AppState>,
    folder_id: Option<String>,
    done: Option<bool>,
    since: Option<String>,
) -> Result<Vec<ActionItemWithContext>, String> {
    list_action_items(
        state.db_manager.pool(),
        &ActionItemFilter { folder_id, done, since },
    )
    .await
    .map_err(|e| format!("Failed to list action items: {}", e))
}

#[tauri::command]
pub async fn api_toggle_action_item(
    state: tauri::State<'_, AppState>,
    action_item_id: String,
    done: bool,
) -> Result<(), String> {
    let result = sqlx::query("UPDATE action_items SET done = ? WHERE id = ?")
        .bind(done)
        .bind(&action_item_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to update action item: {}", e))?;
    if result.rows_affected() == 0 {
        return Err(format!("Action item not found: {}", action_item_id));
    }
    Ok(())
}
