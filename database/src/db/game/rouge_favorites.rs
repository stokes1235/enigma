//! Original-Rouge acquisitions and explicitly unlocked story reviews.
//! Availability/drop-pool rules belong to logic; these rows only record facts.

use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct CollectionRecord {
    pub collection_id: i32,
    pub first_obtained_at: i64,
    pub seen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct StoryRecord {
    pub story_id: i32,
    pub review_id: i32,
    pub unlocked_at: i64,
    pub seen: bool,
}

pub async fn collections(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> sqlx::Result<Vec<CollectionRecord>> {
    sqlx::query_as("SELECT collection_id,first_obtained_at,seen FROM user_rouge_favorite_collections WHERE user_id=? AND season=? ORDER BY collection_id")
        .bind(user_id).bind(season).fetch_all(pool).await
}

pub async fn stories(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> sqlx::Result<Vec<StoryRecord>> {
    sqlx::query_as("SELECT story_id,review_id,unlocked_at,seen FROM user_rouge_favorite_stories WHERE user_id=? AND season=? ORDER BY story_id")
        .bind(user_id).bind(season).fetch_all(pool).await
}

/// Use inside the real action transaction for an item acquired and consumed
/// before its final bag is saved. Ordinary bag writers are covered by triggers.
/// Duplicate facts preserve both first time and the user's cleared red dot.
pub async fn record_collection_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    collection_id: i32,
    now: i64,
) -> sqlx::Result<bool> {
    if season != 1 || now < 0 {
        return Ok(false);
    }
    let result=sqlx::query("INSERT INTO user_rouge_favorite_collections (user_id,season,collection_id,first_obtained_at) SELECT ?,?,?,? WHERE EXISTS (SELECT 1 FROM rouge_favorite_base_catalog WHERE collection_id=?) ON CONFLICT DO NOTHING")
        .bind(user_id).bind(season).bind(collection_id).bind(now).bind(collection_id)
        .execute(&mut **tx).await?;
    Ok(result.rows_affected() == 1)
}

/// Caller validates the configured review and playback eligibility. The
/// request is emitted on StoryStart, so completed-story state is not required.
pub async fn record_story_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    story_id: i32,
    review_id: i32,
    now: i64,
) -> sqlx::Result<bool> {
    if season != 1 || story_id <= 0 || review_id <= 0 || now < 0 {
        return Ok(false);
    }
    let result=sqlx::query("INSERT INTO user_rouge_favorite_stories (user_id,season,story_id,review_id,unlocked_at) VALUES (?,?,?,?,?) ON CONFLICT DO NOTHING")
        .bind(user_id).bind(season).bind(story_id).bind(review_id).bind(now)
        .execute(&mut **tx).await?;
    Ok(result.rows_affected() == 1)
}

/// Type 1 is Collection, type 4 is Story. ID zero clears only that category;
/// the client sends review IDs for Story red dots, not terminal story IDs.
pub async fn mark_seen(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    kind: i32,
    id: i32,
) -> sqlx::Result<bool> {
    if season != 1 || id < 0 {
        return Ok(false);
    }
    let statement = match kind {
        1 => {
            "UPDATE user_rouge_favorite_collections SET seen=1 WHERE user_id=? AND season=? AND (?=0 OR collection_id=?)"
        }
        4 => {
            "UPDATE user_rouge_favorite_stories SET seen=1 WHERE user_id=? AND season=? AND (?=0 OR review_id=?)"
        }
        // No Faction/Illustration red dots are fabricated in this module.
        2 | 3 => return Ok(id == 0),
        _ => return Ok(false),
    };
    sqlx::query(statement)
        .bind(user_id)
        .bind(season)
        .bind(id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    const MIGRATION: &str = include_str!("../../../migrations/124_rouge_favorites.sql");

    async fn legacy() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON; CREATE TABLE users(id INTEGER PRIMARY KEY); INSERT INTO users VALUES(1),(2); CREATE TABLE user_rouge_run(user_id INTEGER NOT NULL,season INTEGER NOT NULL,bag TEXT NOT NULL DEFAULT '',updated_at INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(user_id,season));")
            .execute(&pool).await.unwrap();
        pool
    }

    async fn save(pool: &SqlitePool, bag: &str, now: i64) {
        sqlx::query("INSERT INTO user_rouge_run(user_id,season,bag,updated_at) VALUES(1,1,?,?) ON CONFLICT(user_id,season) DO UPDATE SET bag=excluded.bag,updated_at=excluded.updated_at")
            .bind(bag).bind(now).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn migration_recovers_only_real_current_items_and_inlays_with_safe_json_guards() {
        let pool = legacy().await;
        save(&pool,r#"{"items":[{"uid":999,"item_id":8310405,"holes":[8140401,0,8320062]}, {"item_id":38010101}, {"item_id":"8310609"},"text",1]}"#,90).await;
        sqlx::query("INSERT INTO user_rouge_run(user_id,season,bag) VALUES(2,1,'not json')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
        assert_eq!(
            collections(&pool, 1, 1)
                .await
                .unwrap()
                .iter()
                .map(|r| r.collection_id)
                .collect::<Vec<_>>(),
            vec![8140401, 8310405]
        );
        assert!(collections(&pool, 2, 1).await.unwrap().is_empty());
        let count: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM rouge_favorite_base_catalog")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 198);
    }

    #[tokio::test]
    async fn trigger_covers_initial_bags_direct_rewards_and_forging_without_resetting_seen() {
        let pool = legacy().await;
        sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
        save(
            &pool,
            r#"{"items":[{"item_id":8110312,"holes":[8140301]}]}"#,
            10,
        )
        .await;
        mark_seen(&pool, 1, 1, 1, 8110312).await.unwrap();
        save(
            &pool,
            r#"{"items":[{"item_id":8210313,"holes":[8140301]},{"item_id":8330002}]}"#,
            20,
        )
        .await;
        save(&pool, r#"{"items":[{"item_id":8110312}]}"#, 30).await;
        let rows = collections(&pool, 1, 1).await.unwrap();
        assert_eq!(rows.len(), 4);
        let old = rows.iter().find(|r| r.collection_id == 8110312).unwrap();
        assert_eq!((old.first_obtained_at, old.seen), (10, true));
        assert!(
            rows.iter()
                .find(|r| r.collection_id == 8330002)
                .unwrap()
                .seen
                == false
        );
        sqlx::query("DELETE FROM user_rouge_run WHERE user_id=1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            collections(&pool, 1, 1).await.unwrap().len(),
            4,
            "settlement must not erase the album"
        );
    }

    #[tokio::test]
    async fn rollback_erases_trigger_and_explicit_transient_facts_together() {
        let pool = legacy().await;
        sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO user_rouge_run VALUES(1,1,'{\"items\":[{\"item_id\":8310609}]}',100)",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        assert!(
            record_collection_in_transaction(&mut tx, 1, 1, 8230606, 100)
                .await
                .unwrap()
        );
        assert!(
            !record_collection_in_transaction(&mut tx, 1, 1, 8230606, 101)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
        assert!(collections(&pool, 1, 1).await.unwrap().is_empty());
        let mut tx = pool.begin().await.unwrap();
        assert!(
            record_collection_in_transaction(&mut tx, 1, 1, 8230606, 100)
                .await
                .unwrap()
        );
        assert!(
            !record_collection_in_transaction(&mut tx, 1, 1, 8320062, 100)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            collections(&pool, 1, 1).await.unwrap()[0].collection_id,
            8230606
        );
    }

    #[tokio::test]
    async fn story_idempotency_and_category_clear_do_not_clear_other_progress() {
        let pool = legacy().await;
        sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        record_collection_in_transaction(&mut tx, 1, 1, 8110021, 10)
            .await
            .unwrap();
        assert!(
            record_story_in_transaction(&mut tx, 1, 1, 600909, 8, 10)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        mark_seen(&pool, 1, 1, 4, 8).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            !record_story_in_transaction(&mut tx, 1, 1, 600909, 8, 20)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert!(stories(&pool, 1, 1).await.unwrap()[0].seen);
        assert!(!collections(&pool, 1, 1).await.unwrap()[0].seen);
        mark_seen(&pool, 1, 1, 1, 0).await.unwrap();
        assert!(collections(&pool, 1, 1).await.unwrap()[0].seen);
        assert_eq!(stories(&pool, 1, 1).await.unwrap()[0].unlocked_at, 10);
        assert!(!mark_seen(&pool, 1, 1, 0, 0).await.unwrap());
    }
}
