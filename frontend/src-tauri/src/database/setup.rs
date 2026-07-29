use log::info;
use tauri::{AppHandle, Emitter, Manager};

use super::manager::DatabaseManager;
use crate::state::AppState;

/// Rename the sqlite database (and its WAL/SHM sidecar files, if present) aside with a
/// timestamp suffix so a fresh database can be created in its place. Used as a last-resort
/// recovery path when the database fails to open/migrate on startup (e.g. after a downgrade
/// leaves a migration checksum mismatch) — this preserves the old data for manual recovery
/// instead of deleting it outright.
pub fn backup_and_reset_database(app: &AppHandle) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    for filename in ["meeting_minutes.sqlite", "meeting_minutes.sqlite-wal", "meeting_minutes.sqlite-shm"] {
        let path = app_data_dir.join(filename);
        if path.exists() {
            let backup_path = app_data_dir.join(format!("{}.bak-{}", filename, timestamp));
            std::fs::rename(&path, &backup_path)
                .map_err(|e| format!("Failed to back up {}: {}", filename, e))?;
            info!("Backed up {:?} to {:?}", path, backup_path);
        }
    }

    Ok(())
}

/// Initialize database on app startup
/// Handles first launch detection and conditional initialization
pub async fn initialize_database_on_startup(app: &AppHandle) -> Result<(), String> {
    // Check if this is the first launch (no database exists yet)
    let is_first_launch = DatabaseManager::is_first_launch(app)
        .await
        .map_err(|e| format!("Failed to check first launch status: {}", e))?;

    if is_first_launch {
        info!("First launch detected - will notify window when ready");

        // Delay event emission to ensure window is ready and React listeners are registered
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            app_handle
                .emit("first-launch-detected", ())
                .expect("Failed to emit first-launch-detected event");
            info!("Emitted first-launch-detected after delay");
        });
    } else {
        // Normal flow - initialize database immediately
        let db_manager = DatabaseManager::new_from_app_handle(app)
            .await
            .map_err(|e| format!("Failed to initialize database manager: {}", e))?;

        app.manage(AppState { db_manager });
        info!("Database initialized successfully");
    }

    Ok(())
}
