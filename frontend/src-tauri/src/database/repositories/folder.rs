use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Folder {
    pub id: String,
    pub parent_folder_id: Option<String>,
    pub name: String,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
}

pub struct FoldersRepository;

impl FoldersRepository {
    pub async fn create(
        pool: &SqlitePool,
        parent_folder_id: Option<&str>,
        name: &str,
        icon: Option<&str>,
    ) -> Result<Folder, sqlx::Error> {
        let id = format!("folder-{}", Uuid::new_v4());
        let created_at = chrono::Utc::now().to_rfc3339();

        // New folders go to the end of their siblings
        let sort_order: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders
             WHERE (parent_folder_id IS ? OR parent_folder_id = ?)",
        )
        .bind(parent_folder_id)
        .bind(parent_folder_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        sqlx::query(
            "INSERT INTO folders (id, parent_folder_id, name, icon, sort_order, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(parent_folder_id)
        .bind(name)
        .bind(icon)
        .bind(sort_order)
        .bind(&created_at)
        .execute(pool)
        .await?;

        Ok(Folder {
            id,
            parent_folder_id: parent_folder_id.map(|s| s.to_string()),
            name: name.to_string(),
            icon: icon.map(|s| s.to_string()),
            sort_order,
            created_at,
        })
    }

    /// Flat list of every folder; the frontend assembles the tree from
    /// parent_folder_id, which keeps this query trivial regardless of depth.
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Folder>, sqlx::Error> {
        sqlx::query_as::<_, Folder>(
            "SELECT * FROM folders ORDER BY parent_folder_id IS NOT NULL, sort_order ASC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn rename(pool: &SqlitePool, id: &str, name: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE folders SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_icon(
        pool: &SqlitePool,
        id: &str,
        icon: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("UPDATE folders SET icon = ? WHERE id = ?")
            .bind(icon)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Reparent a folder and/or move it to a new position among its new
    /// siblings. Rejects moving a folder into its own subtree (which would
    /// create a cycle undetectable by simple foreign keys).
    pub async fn move_folder(
        pool: &SqlitePool,
        id: &str,
        new_parent_id: Option<&str>,
        new_sort_order: i64,
    ) -> Result<(), sqlx::Error> {
        if let Some(new_parent_id) = new_parent_id {
            if new_parent_id == id {
                return Err(sqlx::Error::Protocol("A folder cannot be its own parent".into()));
            }
            let descendants = Self::descendant_ids(pool, id).await?;
            if descendants.contains(&new_parent_id.to_string()) {
                return Err(sqlx::Error::Protocol(
                    "Cannot move a folder into its own subfolder".into(),
                ));
            }
        }

        sqlx::query("UPDATE folders SET parent_folder_id = ?, sort_order = ? WHERE id = ?")
            .bind(new_parent_id)
            .bind(new_sort_order)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// All descendant folder ids (not including `id` itself), via BFS —
    /// SQLite in this app runs without recursive-CTE guarantees assumed, so
    /// this stays a plain loop over list_all rather than a WITH RECURSIVE.
    async fn descendant_ids(pool: &SqlitePool, id: &str) -> Result<Vec<String>, sqlx::Error> {
        let all = Self::list_all(pool).await?;
        let mut result = Vec::new();
        let mut frontier = vec![id.to_string()];
        while let Some(current) = frontier.pop() {
            for folder in &all {
                if folder.parent_folder_id.as_deref() == Some(current.as_str()) {
                    result.push(folder.id.clone());
                    frontier.push(folder.id.clone());
                }
            }
        }
        Ok(result)
    }

    /// Delete a folder and its subfolders. Meetings inside are never
    /// deleted — they're moved to no folder (folder_id = NULL), since
    /// folders are organization, not data.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        let mut ids = Self::descendant_ids(pool, id).await?;
        ids.push(id.to_string());

        let mut tx = pool.begin().await?;
        for folder_id in &ids {
            sqlx::query("UPDATE meetings SET folder_id = NULL WHERE folder_id = ?")
                .bind(folder_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM folders WHERE id = ?")
                .bind(folder_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_and_list_nested_folders() {
        let pool = test_pool().await;
        let root = FoldersRepository::create(&pool, None, "CS33", Some("📚")).await.unwrap();
        let child = FoldersRepository::create(&pool, Some(&root.id), "Lectures", None)
            .await
            .unwrap();

        let all = FoldersRepository::list_all(&pool).await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|f| f.id == root.id && f.parent_folder_id.is_none()));
        assert!(all.iter().any(|f| f.id == child.id && f.parent_folder_id.as_deref() == Some(root.id.as_str())));
    }

    #[tokio::test]
    async fn cannot_move_folder_into_own_subtree() {
        let pool = test_pool().await;
        let root = FoldersRepository::create(&pool, None, "Root", None).await.unwrap();
        let child = FoldersRepository::create(&pool, Some(&root.id), "Child", None).await.unwrap();

        let result = FoldersRepository::move_folder(&pool, &root.id, Some(&child.id), 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_folder_orphans_meetings_not_deletes_them() {
        let pool = test_pool().await;
        let folder = FoldersRepository::create(&pool, None, "Temp", None).await.unwrap();

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES ('m1', 'Test', '2026-01-01', '2026-01-01', ?)")
            .bind(&folder.id)
            .execute(&pool)
            .await
            .unwrap();

        FoldersRepository::delete(&pool, &folder.id).await.unwrap();

        let folders = FoldersRepository::list_all(&pool).await.unwrap();
        assert!(folders.is_empty());

        let meeting_folder: Option<String> =
            sqlx::query_scalar("SELECT folder_id FROM meetings WHERE id = 'm1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(meeting_folder.is_none(), "meeting must survive folder deletion");
    }

    #[tokio::test]
    async fn delete_folder_cascades_to_subfolders() {
        let pool = test_pool().await;
        let root = FoldersRepository::create(&pool, None, "Root", None).await.unwrap();
        let child = FoldersRepository::create(&pool, Some(&root.id), "Child", None).await.unwrap();
        let grandchild = FoldersRepository::create(&pool, Some(&child.id), "Grandchild", None)
            .await
            .unwrap();

        FoldersRepository::delete(&pool, &root.id).await.unwrap();

        let remaining = FoldersRepository::list_all(&pool).await.unwrap();
        assert!(remaining.is_empty());
        let _ = (child, grandchild); // ids only needed above
    }
}
