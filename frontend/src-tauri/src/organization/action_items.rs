/// Cross-note action item view (PROJECT_BRIEF.md §12): "all your open todos
/// across every session" — a query against the existing action_items table
/// joined with meetings for folder/date context, no new tables needed.
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::action_item_extraction::ExtractedActionItem;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ActionItemWithContext {
    pub id: String,
    pub meeting_id: String,
    pub description: String,
    pub owner: Option<String>,
    pub due_date: Option<String>,
    pub done: bool,
    pub created_at: String,
    pub meeting_title: String,
    pub folder_id: Option<String>,
    pub context_type: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ActionItemFilter {
    pub folder_id: Option<String>,
    pub done: Option<bool>,
    /// Only items whose meeting was created on/after this RFC3339 date
    pub since: Option<String>,
}

pub async fn list_action_items(
    pool: &SqlitePool,
    filter: &ActionItemFilter,
) -> Result<Vec<ActionItemWithContext>, sqlx::Error> {
    let mut query = String::from(
        "SELECT a.id, a.meeting_id, a.description, a.owner, a.due_date, a.done, a.created_at,
                m.title AS meeting_title, m.folder_id, m.context_type
         FROM action_items a
         JOIN meetings m ON m.id = a.meeting_id
         WHERE 1=1",
    );

    if filter.folder_id.is_some() {
        query.push_str(" AND m.folder_id = ?");
    }
    if filter.done.is_some() {
        query.push_str(" AND a.done = ?");
    }
    if filter.since.is_some() {
        query.push_str(" AND m.created_at >= ?");
    }
    query.push_str(" ORDER BY (a.due_date IS NULL), a.due_date ASC, m.created_at DESC");

    let mut q = sqlx::query_as::<_, ActionItemWithContext>(&query);
    if let Some(folder_id) = &filter.folder_id {
        q = q.bind(folder_id);
    }
    if let Some(done) = filter.done {
        q = q.bind(done);
    }
    if let Some(since) = &filter.since {
        q = q.bind(since);
    }

    q.fetch_all(pool).await
}

/// Replace a meeting's auto-generated (not-yet-done) action items with a
/// fresh extraction from a newly generated summary. Items already marked
/// done are left untouched -- "done" is user-confirmed state, and silently
/// wiping it out because the user regenerated a summary would be a worse
/// surprise than a stale extraction.
pub async fn replace_undone_action_items(
    pool: &SqlitePool,
    meeting_id: &str,
    items: &[ExtractedActionItem],
) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM action_items WHERE meeting_id = ? AND done = 0")
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;

    for item in items {
        sqlx::query(
            "INSERT INTO action_items (id, meeting_id, description, owner, due_date) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(format!("action-{}", Uuid::new_v4()))
        .bind(meeting_id)
        .bind(&item.description)
        .bind(&item.owner)
        .bind(&item.due_date)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(items.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query("INSERT INTO folders (id, name, created_at) VALUES ('f1', 'CS33', '2026-01-01')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_id, context_type)
             VALUES ('m1', 'Lecture 1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'f1', 'lecture')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, context_type)
             VALUES ('m2', 'Standup', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 'meeting')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO action_items (id, meeting_id, description, done, due_date) VALUES ('a1', 'm1', 'Read chapter 3', 0, '2026-01-10')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO action_items (id, meeting_id, description, done) VALUES ('a2', 'm1', 'Finished item', 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO action_items (id, meeting_id, description, done) VALUES ('a3', 'm2', 'Send follow-up email', 0)")
            .execute(&pool).await.unwrap();

        pool
    }

    #[tokio::test]
    async fn no_filter_returns_all_sorted_by_due_date() {
        let pool = seeded_pool().await;
        let items = list_action_items(&pool, &ActionItemFilter::default()).await.unwrap();
        assert_eq!(items.len(), 3);
        // due-dated item first, then undated items (newest meeting first)
        assert_eq!(items[0].id, "a1");
    }

    #[tokio::test]
    async fn filters_by_done_status() {
        let pool = seeded_pool().await;
        let open = list_action_items(
            &pool,
            &ActionItemFilter { done: Some(false), ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(open.len(), 2);
        assert!(open.iter().all(|i| !i.done));
    }

    #[tokio::test]
    async fn filters_by_folder() {
        let pool = seeded_pool().await;
        let in_folder = list_action_items(
            &pool,
            &ActionItemFilter { folder_id: Some("f1".to_string()), ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(in_folder.len(), 2);
        assert!(in_folder.iter().all(|i| i.meeting_id == "m1"));
    }

    #[tokio::test]
    async fn carries_meeting_context() {
        let pool = seeded_pool().await;
        let items = list_action_items(&pool, &ActionItemFilter::default()).await.unwrap();
        let a1 = items.iter().find(|i| i.id == "a1").unwrap();
        assert_eq!(a1.meeting_title, "Lecture 1");
        assert_eq!(a1.context_type, "lecture");
        assert_eq!(a1.folder_id.as_deref(), Some("f1"));
    }

    #[tokio::test]
    async fn replace_undone_action_items_inserts_and_replaces() {
        let pool = seeded_pool().await;
        // Seed data already has, for m1: a1 (not done) and a2 (done) --
        // a2 persists across every replace call below since it's done.

        let first_pass = vec![
            ExtractedActionItem { description: "Read chapter 1".to_string(), owner: None, due_date: None },
            ExtractedActionItem { description: "Read chapter 2".to_string(), owner: None, due_date: Some("Friday".to_string()) },
        ];
        let inserted = replace_undone_action_items(&pool, "m1", &first_pass).await.unwrap();
        assert_eq!(inserted, 2);

        let after_first = list_action_items(&pool, &ActionItemFilter { folder_id: None, done: None, since: None })
            .await
            .unwrap();
        let m1_items: Vec<_> = after_first.iter().filter(|i| i.meeting_id == "m1").collect();
        assert_eq!(m1_items.len(), 3, "seed's a1 replaced by 2 new items, seed's done a2 preserved");
        assert!(m1_items.iter().any(|i| i.id == "a2" && i.done));
        assert!(m1_items.iter().all(|i| i.id != "a1"), "seed's not-done a1 should have been replaced");

        // Regenerating the summary replaces the not-yet-done items again
        let second_pass = vec![ExtractedActionItem {
            description: "Read chapters 1 through 3".to_string(),
            owner: None,
            due_date: None,
        }];
        replace_undone_action_items(&pool, "m1", &second_pass).await.unwrap();

        let after_second = list_action_items(&pool, &ActionItemFilter { folder_id: None, done: None, since: None })
            .await
            .unwrap();
        let m1_items: Vec<_> = after_second.iter().filter(|i| i.meeting_id == "m1").collect();
        assert_eq!(m1_items.len(), 2, "the 2 first-pass items replaced by 1, done a2 still preserved");
        assert!(m1_items.iter().any(|i| i.description == "Read chapters 1 through 3"));
        assert!(m1_items.iter().any(|i| i.id == "a2" && i.done));
    }

    #[tokio::test]
    async fn replace_undone_action_items_preserves_done_items() {
        let pool = seeded_pool().await;

        // a2 in the seed data is already done=1
        let before = list_action_items(&pool, &ActionItemFilter { folder_id: None, done: Some(true), since: None })
            .await
            .unwrap();
        assert!(before.iter().any(|i| i.id == "a2"), "seed data must include a done item to test against");

        let regenerated = vec![ExtractedActionItem {
            description: "A brand new item from the regenerated summary".to_string(),
            owner: None,
            due_date: None,
        }];
        replace_undone_action_items(&pool, "m1", &regenerated).await.unwrap();

        let after = list_action_items(&pool, &ActionItemFilter { folder_id: None, done: None, since: None })
            .await
            .unwrap();
        assert!(
            after.iter().any(|i| i.id == "a2" && i.done),
            "an item the user already marked done must survive regeneration"
        );
    }
}
