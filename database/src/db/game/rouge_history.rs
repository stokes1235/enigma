//! Persistent original-Rouge endings, independent of the latest review cache.
//! Insert inside settlement's transaction so a failed/retried settlement cannot
//! grant an ending separately from the run's progression and rewards.

use sqlx::{Sqlite, SqlitePool, Transaction};

pub async fn passed_endings(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> sqlx::Result<Vec<i32>> {
    sqlx::query_scalar(
        "SELECT ending_id FROM user_rouge_ending_history
         WHERE user_id = ? AND season = ? ORDER BY ending_id",
    )
    .bind(user_id)
    .bind(season)
    .fetch_all(pool)
    .await
}

/// Records the first completion time. Replaying or clearing the same ending
/// again does not replace its original timestamp or remove another ending.
pub async fn record_ending_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    ending_id: i32,
    now: i64,
) -> sqlx::Result<()> {
    if !(101..=104).contains(&ending_id) || season <= 0 || now < 0 {
        return Err(sqlx::Error::Protocol(
            "invalid original Rouge ending history".to_owned(),
        ));
    }
    sqlx::query(
        "INSERT INTO user_rouge_ending_history (user_id, season, ending_id, completed_at)
         VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(ending_id)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATION: &str = include_str!("../../../migrations/122_rouge_ending_history.sql");

    async fn old_database() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE users (id INTEGER PRIMARY KEY);
             CREATE TABLE user_rouge_settlement (
                 user_id INTEGER NOT NULL, season INTEGER NOT NULL,
                 rouge_info TEXT NOT NULL, result_info TEXT NOT NULL,
                 PRIMARY KEY (user_id, season));
             INSERT INTO users (id) VALUES (1), (2), (3), (4), (5), (6), (7);",
        )
        .execute(&db)
        .await
        .unwrap();
        db
    }

    async fn cache(db: &SqlitePool, user: i64, info: &str, result: &str) {
        sqlx::query(
            "INSERT INTO user_rouge_settlement (user_id, season, rouge_info, result_info)
             VALUES (?, 1, ?, ?) ON CONFLICT (user_id, season) DO UPDATE SET
                 rouge_info = excluded.rouge_info, result_info = excluded.result_info",
        )
        .bind(user)
        .bind(info)
        .bind(result)
        .execute(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn migration_recovers_success_and_preserves_invalid_or_failed_snapshots() {
        let db = old_database().await;
        cache(
            &db,
            1,
            "{}",
            r#"{"end2Score":"101#500","reviewInfo":{"endId":101,"finishTime":123}}"#,
        )
        .await;
        cache(&db, 2, r#"{"endId":102}"#, r#"{"end2Score":"102#500"}"#).await;
        cache(&db, 3, r#"{"endId":101}"#, "broken json").await;
        cache(&db, 4, r#"{"endId":101}"#, r#"{"end2Score":"0#0"}"#).await;
        cache(&db, 5, r#"{"endId":101}"#, r#"{"end2Score":""}"#).await;
        cache(&db, 6, r#"{"endId":999}"#, r#"{"end2Score":"999#500"}"#).await;
        cache(&db, 7, r#"{"endId":101}"#, r#"{"end2Score":"102#500"}"#).await;
        sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
        assert_eq!(passed_endings(&db, 1, 1).await.unwrap(), vec![101]);
        assert_eq!(passed_endings(&db, 2, 1).await.unwrap(), vec![102]);
        for user in 3..=7 {
            assert!(passed_endings(&db, user, 1).await.unwrap().is_empty());
        }
        let timestamp: i64 = sqlx::query_scalar(
            "SELECT completed_at FROM user_rouge_ending_history WHERE user_id = 1",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(timestamp, 123_000);
        let cached: String =
            sqlx::query_scalar("SELECT result_info FROM user_rouge_settlement WHERE user_id = 3")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(cached, "broken json");
    }

    #[tokio::test]
    async fn another_ending_or_failed_review_never_erases_a_prior_clear() {
        let db = old_database().await;
        sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
        let mut tx = db.begin().await.unwrap();
        record_ending_in_transaction(&mut tx, 1, 1, 101, 100)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        cache(&db, 1, r#"{"endId":0}"#, r#"{"end2Score":"0#0"}"#).await;
        assert_eq!(passed_endings(&db, 1, 1).await.unwrap(), vec![101]);

        let mut tx = db.begin().await.unwrap();
        record_ending_in_transaction(&mut tx, 1, 1, 102, 200)
            .await
            .unwrap();
        record_ending_in_transaction(&mut tx, 1, 1, 101, 300)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        cache(&db, 1, r#"{"endId":102}"#, r#"{"end2Score":"102#500"}"#).await;
        assert_eq!(passed_endings(&db, 1, 1).await.unwrap(), vec![101, 102]);
        assert!(passed_endings(&db, 2, 1).await.unwrap().is_empty());
        assert!(passed_endings(&db, 1, 2).await.unwrap().is_empty());
        let timestamp: i64 = sqlx::query_scalar(
            "SELECT completed_at FROM user_rouge_ending_history
             WHERE user_id = 1 AND season = 1 AND ending_id = 101",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(timestamp, 100);
    }

    #[tokio::test]
    async fn rollback_does_not_unlock_an_ending() {
        let db = old_database().await;
        sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
        let mut tx = db.begin().await.unwrap();
        record_ending_in_transaction(&mut tx, 1, 1, 103, 100)
            .await
            .unwrap();
        assert!(
            record_ending_in_transaction(&mut tx, 1, 1, 0, 100)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        assert!(passed_endings(&db, 1, 1).await.unwrap().is_empty());
    }
}
