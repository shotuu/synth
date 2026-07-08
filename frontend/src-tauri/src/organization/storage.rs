/// Storage Manager (PROJECT_BRIEF.md §4): aggregate disk usage plus a
/// per-session audio listing, so cleanup decisions are informed rather than
/// "go delete files in Finder."
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SessionAudioRow {
    pub meeting_id: String,
    pub title: String,
    pub context_type: String,
    pub folder_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub current_size_bytes: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    pub retained: bool,
    pub last_compressed_at: Option<String>,
    pub storage_path: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct StorageStats {
    pub audio_bytes: i64,
    pub attachments_bytes: i64,
    pub database_bytes: i64,
    pub session_count: i64,
    pub sessions_with_retained_audio: i64,
}

/// List sessions that have ever had audio tracked, newest first by default;
/// the frontend sorts client-side once loaded (small dataset by design --
/// see PROJECT_BRIEF.md §4 on why text/metadata stays cheap at scale).
pub async fn list_session_audio(pool: &SqlitePool) -> Result<Vec<SessionAudioRow>, sqlx::Error> {
    sqlx::query_as::<_, SessionAudioRow>(
        "SELECT m.id AS meeting_id, m.title, m.context_type, m.folder_id, m.created_at, m.updated_at,
                a.current_size_bytes, a.bitrate_kbps, a.retained, a.last_compressed_at, a.storage_path
         FROM note_audio a
         JOIN meetings m ON m.id = a.meeting_id
         ORDER BY m.created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// Sessions whose audio is still retained and hasn't been touched
/// (recompressed) in `older_than_days` -- the "suggested cleanup" surface.
/// A session counts as retained-and-stale by `updated_at` when it's never
/// been compressed, so newly recorded sessions aren't flagged just because
/// last_compressed_at is null.
pub async fn suggested_cleanup(
    pool: &SqlitePool,
    older_than_days: i64,
) -> Result<Vec<SessionAudioRow>, sqlx::Error> {
    sqlx::query_as::<_, SessionAudioRow>(
        "SELECT m.id AS meeting_id, m.title, m.context_type, m.folder_id, m.created_at, m.updated_at,
                a.current_size_bytes, a.bitrate_kbps, a.retained, a.last_compressed_at, a.storage_path
         FROM note_audio a
         JOIN meetings m ON m.id = a.meeting_id
         WHERE a.retained = 1
           AND julianday('now') - julianday(COALESCE(a.last_compressed_at, m.created_at)) >= ?
         ORDER BY a.current_size_bytes DESC",
    )
    .bind(older_than_days)
    .fetch_all(pool)
    .await
}

fn dir_size(path: &Path) -> i64 {
    if !path.exists() {
        return 0;
    }
    walk_size(path)
}

fn walk_size(path: &Path) -> i64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len() as i64;
    }
    if !metadata.is_dir() {
        return 0; // skip symlinks -- e.g. a dev data dir sharing prod's models/
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| walk_size(&entry.path()))
        .sum()
}

pub async fn compute_stats(
    pool: &SqlitePool,
    app_data_dir: &Path,
) -> Result<StorageStats, sqlx::Error> {
    let audio_bytes = dir_size(&app_data_dir.join("audio"));
    let attachments_bytes = dir_size(&app_data_dir.join("attachments"));

    let mut database_bytes = 0i64;
    for name in ["meeting_minutes.sqlite", "meeting_minutes.sqlite-wal", "meeting_minutes.sqlite-shm"] {
        if let Ok(meta) = std::fs::metadata(app_data_dir.join(name)) {
            database_bytes += meta.len() as i64;
        }
    }

    let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(pool)
        .await?;
    let sessions_with_retained_audio: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM note_audio WHERE retained = 1")
            .fetch_one(pool)
            .await?;

    Ok(StorageStats {
        audio_bytes,
        attachments_bytes,
        database_bytes,
        session_count,
        sessions_with_retained_audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dates are computed relative to SQLite's own `now` (not hardcoded
    /// absolute dates) so this test is stable regardless of when it runs.
    async fn seeded_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, context_type) VALUES
             ('m1', 'Old Lecture', datetime('now', '-400 days'), datetime('now', '-400 days'), 'lecture'),
             ('m2', 'Recent Standup', datetime('now', '-2 days'), datetime('now', '-2 days'), 'meeting')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO note_audio (meeting_id, storage_path, origin, current_size_bytes, retained, created_at)
             VALUES ('m1', '/tmp/m1.mp4', 'recorded', 50000000, 1, datetime('now', '-400 days'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO note_audio (meeting_id, storage_path, origin, current_size_bytes, retained, created_at)
             VALUES ('m2', '/tmp/m2.mp4', 'recorded', 20000000, 1, datetime('now', '-2 days'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn lists_session_audio_with_context() {
        let pool = seeded_pool().await;
        let rows = list_session_audio(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        let m1 = rows.iter().find(|r| r.meeting_id == "m1").unwrap();
        assert_eq!(m1.title, "Old Lecture");
        assert_eq!(m1.context_type, "lecture");
        assert_eq!(m1.current_size_bytes, Some(50000000));
    }

    #[tokio::test]
    async fn suggested_cleanup_flags_old_untouched_audio_only() {
        let pool = seeded_pool().await;
        // m1 is ~400 days old, m2 ~2 days old; a 30-day threshold catches only m1
        let stale = suggested_cleanup(&pool, 30).await.unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].meeting_id, "m1");

        // A threshold longer than either session's age excludes everything
        let none = suggested_cleanup(&pool, 1000).await.unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn suggested_cleanup_excludes_non_retained_audio() {
        let pool = seeded_pool().await;
        sqlx::query("UPDATE note_audio SET retained = 0 WHERE meeting_id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();
        let stale = suggested_cleanup(&pool, 30).await.unwrap();
        assert!(stale.iter().all(|r| r.meeting_id != "m1"));
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = std::env::temp_dir().join(format!("synth-storage-test-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        std::fs::write(tmp.join("a.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(tmp.join("sub/b.txt"), vec![0u8; 250]).unwrap();

        assert_eq!(dir_size(&tmp), 350);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn dir_size_of_missing_path_is_zero() {
        let missing = std::env::temp_dir().join("synth-storage-does-not-exist-xyz");
        assert_eq!(dir_size(&missing), 0);
    }
}
