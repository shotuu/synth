use sqlx::{migrate::MigrateDatabase, Result, Sqlite, SqlitePool, Transaction};
use std::fs;
use std::path::Path;
use tauri::Manager;

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    pub async fn new(tauri_db_path: &str, backend_db_path: &str) -> Result<Self> {
        if let Some(parent_dir) = Path::new(tauri_db_path).parent() {
            if !parent_dir.exists() {
                fs::create_dir_all(parent_dir).map_err(|e| sqlx::Error::Io(e))?;
            }
        }

        if !Path::new(tauri_db_path).exists() {
            if Path::new(backend_db_path).exists() {
                log::info!(
                    "Copying database from {} to {}",
                    backend_db_path,
                    tauri_db_path
                );
                fs::copy(backend_db_path, tauri_db_path).map_err(|e| sqlx::Error::Io(e))?;
            } else {
                log::info!("Creating database at {}", tauri_db_path);
                Sqlite::create_database(tauri_db_path).await?;
            }
        }

        let pool = SqlitePool::connect(tauri_db_path).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        sanitize_leaked_ai_title_clauses(&pool).await?;

        Ok(DatabaseManager { pool })
    }

    // NOTE: So for the first time users they needs to start the application
    // after they can just delete the existing .sqlite file and then copy the existing .db file to
    // the current app dir, So the system detects legacy db and copy it and starts with that data
    // (Newly created .sqlite with the copied content from .db)
    pub async fn new_from_app_handle(app_handle: &tauri::AppHandle) -> Result<Self> {
        // Resolve the app's data directory
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");
        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(|e| sqlx::Error::Io(e))?;
        }

        // Define database paths
        let tauri_db_path = app_data_dir
            .join("meeting_minutes.sqlite")
            .to_string_lossy()
            .to_string();
        // Legacy backend DB path (for auto-migration if exists)
        let backend_db_path = app_data_dir
            .join("meeting_minutes.db")
            .to_string_lossy()
            .to_string();

        // WAL file paths for defensive cleanup
        let wal_path = app_data_dir.join("meeting_minutes.sqlite-wal");
        let shm_path = app_data_dir.join("meeting_minutes.sqlite-shm");

        log::info!("Tauri DB path: {}", tauri_db_path);
        log::info!("Legacy backend DB path: {}", backend_db_path);

        // Try to open database with defensive WAL handling
        match Self::new(&tauri_db_path, &backend_db_path).await {
            Ok(db_manager) => {
                log::info!("Database opened successfully");
                Ok(db_manager)
            }
            Err(e) => {
                // Check if error is due to corrupted WAL file
                let error_msg = e.to_string();
                if error_msg.contains("malformed") || error_msg.contains("corrupt") {
                    log::warn!("Database appears corrupted, likely due to orphaned WAL file. Attempting recovery...");
                    log::warn!("Error details: {}", error_msg);

                    // Delete potentially corrupted WAL/SHM files
                    if wal_path.exists() {
                        match fs::remove_file(&wal_path) {
                            Ok(_) => log::info!("Removed orphaned WAL file: {:?}", wal_path),
                            Err(e) => log::warn!("Failed to remove WAL file: {}", e),
                        }
                    }
                    if shm_path.exists() {
                        match fs::remove_file(&shm_path) {
                            Ok(_) => log::info!("Removed orphaned SHM file: {:?}", shm_path),
                            Err(e) => log::warn!("Failed to remove SHM file: {}", e),
                        }
                    }

                    // Retry connection without WAL files
                    log::info!("Retrying database connection after WAL cleanup...");
                    match Self::new(&tauri_db_path, &backend_db_path).await {
                        Ok(db_manager) => {
                            log::info!("Database opened successfully after WAL recovery");
                            Ok(db_manager)
                        }
                        Err(retry_err) => {
                            log::error!("Database connection failed even after WAL cleanup: {}", retry_err);
                            Err(retry_err)
                        }
                    }
                } else {
                    // Not a WAL-related error, propagate original error
                    log::error!("Database connection failed: {}", error_msg);
                    Err(e)
                }
            }
        }
    }

    /// Check if this is the first launch (sqlite database doesn't exist yet)
    pub async fn is_first_launch(app_handle: &tauri::AppHandle) -> Result<bool> {
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");

        let tauri_db_path = app_data_dir.join("meeting_minutes.sqlite");

        Ok(!tauri_db_path.exists())
    }

    /// Import a legacy database from the specified path and initialize
    pub async fn import_legacy_database(
        app_handle: &tauri::AppHandle,
        legacy_db_path: &str,
    ) -> Result<Self> {
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");

        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(|e| sqlx::Error::Io(e))?;
        }

        // Copy legacy database to app data directory as meeting_minutes.db
        let target_legacy_path = app_data_dir.join("meeting_minutes.db");
        log::info!(
            "Copying legacy database from {} to {}",
            legacy_db_path,
            target_legacy_path.display()
        );

        fs::copy(legacy_db_path, &target_legacy_path).map_err(|e| sqlx::Error::Io(e))?;

        // Now use the standard initialization which will detect and migrate the legacy db
        Self::new_from_app_handle(app_handle).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn with_transaction<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_, Sqlite>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut tx = self.pool.begin().await?;
        let result = f(&mut tx).await;

        match result {
            Ok(val) => {
                tx.commit().await?;
                Ok(val)
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

    /// Cleanup database connection and checkpoint WAL
    /// This should be called on application shutdown to ensure:
    /// - All WAL changes are written to the main database file
    /// - The .wal and .shm files are deleted
    /// - Connection pool is gracefully closed
    pub async fn cleanup(&self) -> Result<()> {
        log::info!("Starting database cleanup...");

        // Force checkpoint of WAL to main database file and remove WAL file
        // TRUNCATE mode: checkpoints all pages AND deletes the WAL file
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
        {
            Ok(_) => log::info!("WAL checkpoint completed successfully"),
            Err(e) => log::warn!("WAL checkpoint failed (non-fatal): {}", e),
        }

        // Close the connection pool gracefully
        self.pool.close().await;
        log::info!("Database connection pool closed");

        Ok(())
    }
}

/// One-time cleanup for meetings whose AI-generated title leaked the old
/// prompt's "AI-Generated Title" clause (fixed 2026-07 — see
/// summary::processor::strip_ai_generated_title_clause) into the saved
/// title before the prompt itself was fixed. Runs on every startup after
/// migrations; naturally idempotent since already-clean titles never match
/// the pattern, so no "has this run" flag is needed.
async fn sanitize_leaked_ai_title_clauses(pool: &SqlitePool) -> Result<()> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, title FROM meetings")
        .fetch_all(pool)
        .await?;

    for (id, title) in rows {
        let cleaned = crate::summary::processor::strip_ai_generated_title_clause(&title);
        if cleaned == title {
            continue;
        }

        let mut transaction = pool.begin().await?;
        sqlx::query("UPDATE meetings SET title = ? WHERE id = ?")
            .bind(&cleaned)
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        // Kept in sync with meetings.title by every other title-update path
        // (see MeetingsRepository::update_meeting_name) — do the same here.
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(&cleaned)
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        log::info!(
            "Cleaned leaked AI-title clause from meeting {}: '{}' -> '{}'",
            id,
            title,
            cleaned
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// Regression test: sqlx's SqliteConnectOptions enables `PRAGMA foreign_keys` by
    /// default (see sqlx-sqlite's options/mod.rs — "SQLx chooses to enable this by
    /// default so that foreign keys function as expected"), and DatabaseManager::new's
    /// SqlitePool::connect(bare_path) goes through that same default. This locks that
    /// behavior in: if a future sqlx upgrade or a switch to an explicit
    /// SqliteConnectOptions/connection string ever silently drops that default, this
    /// test catches it. (A comment in meeting.rs used to claim FK enforcement was never
    /// on in this app at all — it wasn't accurate; this test is what disproved it.)
    #[tokio::test]
    async fn foreign_keys_are_enforced_on_the_default_bare_path_connection() {
        // Faithfully reproduce DatabaseManager::new's exact connection path: a bare
        // filesystem path with no "sqlite:" scheme, not the "sqlite::memory:" shortcut
        // test_pool() uses above, in case that shortcut parses differently.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fk_check.sqlite");
        let db_path_str = db_path.to_str().unwrap();
        sqlx::Sqlite::create_database(db_path_str).await.unwrap();
        let pool = SqlitePool::connect(db_path_str).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let result = sqlx::query(
            "INSERT INTO transcript_chunks (meeting_id, meeting_name, transcript_text, model, model_name, created_at) VALUES ('nonexistent-meeting-id', 'x', 'hello', 'ollama', 'llama3.1', datetime('now'))",
        )
        .execute(&pool)
        .await;
        assert!(
            result.is_err(),
            "insert with a meeting_id referencing a nonexistent meeting should have been rejected by the foreign key constraint"
        );
    }

    async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))",
        )
        .bind(id)
        .bind(title)
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO transcript_chunks (meeting_id, meeting_name, transcript_text, model, model_name, created_at)
             VALUES (?, ?, 'hello', 'ollama', 'llama3.1', datetime('now'))",
        )
        .bind(id)
        .bind(title)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sanitize_cleans_leaked_clause_from_both_tables() {
        let pool = test_pool().await;
        insert_meeting(&pool, "m1", "AI-Generated Title: Sprint Planning").await;
        insert_meeting(&pool, "m2", "Design Review: Export Pipeline").await; // already clean

        sanitize_leaked_ai_title_clauses(&pool).await.unwrap();

        let (t1,): (String,) = sqlx::query_as("SELECT title FROM meetings WHERE id = 'm1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t1, "Sprint Planning");

        let (n1,): (String,) =
            sqlx::query_as("SELECT meeting_name FROM transcript_chunks WHERE meeting_id = 'm1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n1, "Sprint Planning");

        let (t2,): (String,) = sqlx::query_as("SELECT title FROM meetings WHERE id = 'm2'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t2, "Design Review: Export Pipeline", "clean titles must be left untouched");
    }

    #[tokio::test]
    async fn sanitize_is_a_no_op_on_second_run() {
        let pool = test_pool().await;
        insert_meeting(&pool, "m1", "AI Generated Title - Q3 Roadmap").await;

        sanitize_leaked_ai_title_clauses(&pool).await.unwrap();
        sanitize_leaked_ai_title_clauses(&pool).await.unwrap(); // idempotent re-run

        let (t1,): (String,) = sqlx::query_as("SELECT title FROM meetings WHERE id = 'm1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t1, "Q3 Roadmap");
    }
}
