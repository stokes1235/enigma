//! Persistence for original-Rouge Craftmaster's Favors (金匠预言).
//!
//! Refreshing is a weekly login action. It never accumulates multiple grants
//! for weeks in which the player did not log in. Spending is called inside the
//! same transaction as settlement, its reward credit and its replay snapshot.

use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRow)]
pub struct ExtraPoints {
    pub balance: i32,
    pub recovery_week: Option<i32>,
}

/// Atomically initializes/refills an account at the existing server's weekly
/// boundary (Monday 05:00). A backwards clock cannot earn a second grant.
pub async fn refresh_weekly(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    now_ms: i64,
    recover: i32,
    limit: i32,
) -> sqlx::Result<ExtraPoints> {
    if season != 1 || recover < 0 || limit < 0 {
        return Err(sqlx::Error::Protocol(
            "invalid original Rouge extra-point settings".to_owned(),
        ));
    }
    let week = common::time::ServerTime::server_week(now_ms);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_rouge_extra_points (user_id, season)
         VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .execute(&mut *tx)
    .await?;
    // Use 64-bit SQL arithmetic and cap before converting back to i32.
    sqlx::query(
        "UPDATE user_rouge_extra_points
         SET balance = MIN(?, balance + ?), recovery_week = ?
         WHERE user_id = ? AND season = ?
           AND (recovery_week IS NULL OR recovery_week < ?)",
    )
    .bind(limit)
    .bind(recover)
    .bind(week)
    .bind(user_id)
    .bind(season)
    .bind(week)
    .execute(&mut *tx)
    .await?;
    let result = sqlx::query_as::<_, ExtraPoints>(
        "SELECT balance, recovery_week FROM user_rouge_extra_points
         WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result)
}

/// Consumes precisely the balance used to calculate a settlement. If a weekly
/// refill or another settlement changed it, the caller must roll back its whole
/// settlement and recompute, including the serialized result snapshot.
pub async fn consume_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    expected_balance: i32,
    amount: i32,
) -> sqlx::Result<bool> {
    if season != 1 || expected_balance < 0 || amount < 0 || amount > expected_balance {
        return Ok(false);
    }
    let result = sqlx::query(
        "UPDATE user_rouge_extra_points SET balance = balance - ?
         WHERE user_id = ? AND season = ? AND balance = ? AND balance >= ?",
    )
    .bind(amount)
    .bind(user_id)
    .bind(season)
    .bind(expected_balance)
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::time::ServerTime;

    async fn db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (1, 'rouge-extra-points', 0, 0)",
        )
        .execute(&db)
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn weekly_grant_obeys_monday_boundary_and_does_not_repeat_or_move_backwards() {
        let db = db().await;
        let monday = 1_794_218_400_000_i64; // 2026-11-09 10:00 UTC: server Monday 05:00.
        assert_eq!(ServerTime::server_weekday(monday), 1);
        assert_ne!(
            ServerTime::server_week(monday - 1),
            ServerTime::server_week(monday)
        );
        let initial = refresh_weekly(&db, 1, 1, monday - 1, 8, 16).await.unwrap();
        assert_eq!(
            initial.balance, 8,
            "first local login gets one weekly grant"
        );
        assert_eq!(
            refresh_weekly(&db, 1, 1, monday - 1, 8, 16).await.unwrap(),
            initial
        );
        let next = refresh_weekly(&db, 1, 1, monday, 8, 16).await.unwrap();
        assert_eq!(next.balance, 16);
        assert_eq!(
            refresh_weekly(&db, 1, 1, monday + 1, 8, 16).await.unwrap(),
            next
        );
        assert_eq!(
            refresh_weekly(&db, 1, 1, monday - 1, 8, 16).await.unwrap(),
            next,
            "clock rollback must not issue another grant"
        );
        // Spending and returning weeks later grants 8 once, not 8 per missed week.
        let mut tx = db.begin().await.unwrap();
        assert!(consume_in_transaction(&mut tx, 1, 1, 16, 16).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(
            refresh_weekly(&db, 1, 1, monday + 4 * 7 * 86_400_000, 8, 16)
                .await
                .unwrap()
                .balance,
            8
        );
    }

    #[tokio::test]
    async fn captured_fourteen_to_six_and_failed_settlement_rolls_back_the_spend() {
        let db = db().await;
        let now = 1_794_218_400_000;
        refresh_weekly(&db, 1, 1, now, 8, 16).await.unwrap();
        sqlx::query("UPDATE user_rouge_extra_points SET balance = 14 WHERE user_id = 1")
            .execute(&db)
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        assert!(consume_in_transaction(&mut tx, 1, 1, 14, 8).await.unwrap());
        tx.rollback().await.unwrap();
        assert_eq!(
            refresh_weekly(&db, 1, 1, now, 8, 16).await.unwrap().balance,
            14
        );

        let mut tx = db.begin().await.unwrap();
        assert!(consume_in_transaction(&mut tx, 1, 1, 14, 8).await.unwrap());
        assert!(!consume_in_transaction(&mut tx, 1, 1, 14, 8).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(
            refresh_weekly(&db, 1, 1, now, 8, 16).await.unwrap().balance,
            6
        );
    }

    #[tokio::test]
    async fn zero_balance_is_valid_and_invalid_or_stale_quotes_are_rejected() {
        let db = db().await;
        refresh_weekly(&db, 1, 1, 1_794_218_400_000, 0, 16)
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        assert!(consume_in_transaction(&mut tx, 1, 1, 0, 0).await.unwrap());
        assert!(!consume_in_transaction(&mut tx, 1, 1, 0, 1).await.unwrap());
        assert!(!consume_in_transaction(&mut tx, 1, 1, -1, 0).await.unwrap());
        assert!(!consume_in_transaction(&mut tx, 1, 2, 0, 0).await.unwrap());
        tx.commit().await.unwrap();
    }
}
