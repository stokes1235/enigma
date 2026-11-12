//! Append-only reports plus proven event/layer facts for original Rouge.
//! Report insertion belongs in settlement's transaction after its run guard.

use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Clone, Debug, FromRow)]
pub struct ReviewRecord {
    pub run_seed: Option<i64>,
    pub review_info: String,
    pub finish_time: i64,
}

/// Older local settlements wrote seconds; the client expects milliseconds.
/// Real R1999 timestamps fall well above this bound in milliseconds. Zero is
/// retained as unknown, not turned into an invented completion time.
pub fn normalize_finish_time(value: i64) -> i64 {
    match value {
        1..100_000_000_000 => value * 1000,
        _ => value.max(0),
    }
}

pub async fn record_review_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    run_seed: i64,
    result_info: &str,
    now: i64,
) -> sqlx::Result<bool> {
    if season != 1 || now < 0 {
        return Err(sqlx::Error::Protocol("invalid Rouge review scope".into()));
    }
    let result: sonettobuf::RougeResultInfo = serde_json::from_str(result_info)
        .map_err(|error| sqlx::Error::Protocol(format!("invalid Rouge review: {error}")))?;
    let Some(mut review) = result.review_info else {
        return Ok(false);
    };
    if review.season.is_some_and(|value| value != season) {
        return Err(sqlx::Error::Protocol("Rouge review season mismatch".into()));
    }
    review.season = Some(season);
    let finish_time = normalize_finish_time(review.finish_time.unwrap_or(now));
    review.finish_time = Some(finish_time);
    let raw = serde_json::to_string(&review)
        .map_err(|error| sqlx::Error::Protocol(format!("invalid Rouge review: {error}")))?;
    let inserted = sqlx::query(
        "INSERT INTO user_rouge_reviews
         (user_id,season,run_seed,review_info,finish_time,recorded_at)
         VALUES (?,?,?,?,?,?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(run_seed)
    .bind(raw)
    .bind(finish_time)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

/// Local retention policy: return every report, newest first. There is no
/// exported official cap, and this query never silently prunes older runs.
pub async fn list(pool: &SqlitePool, user_id: i64, season: i32) -> sqlx::Result<Vec<ReviewRecord>> {
    sqlx::query_as("SELECT run_seed,review_info,finish_time FROM user_rouge_reviews WHERE user_id=? AND season=? ORDER BY finish_time DESC,id DESC")
        .bind(user_id).bind(season).fetch_all(pool).await
}

pub async fn passed_events(pool: &SqlitePool, user_id: i64, season: i32) -> sqlx::Result<Vec<i32>> {
    sqlx::query_scalar("SELECT event_id FROM user_rouge_passed_events WHERE user_id=? AND season=? ORDER BY event_id")
        .bind(user_id).bind(season).fetch_all(pool).await
}

pub async fn passed_layers(pool: &SqlitePool, user_id: i64, season: i32) -> sqlx::Result<Vec<i32>> {
    sqlx::query_scalar("SELECT layer_id FROM user_rouge_passed_layers WHERE user_id=? AND season=? ORDER BY layer_id")
        .bind(user_id).bind(season).fetch_all(pool).await
}

pub async fn seen_illustrations(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> sqlx::Result<Vec<i32>> {
    sqlx::query_scalar("SELECT illustration_id FROM user_rouge_illustration_seen WHERE user_id=? AND season=? ORDER BY illustration_id")
        .bind(user_id).bind(season).fetch_all(pool).await
}

/// Logic supplies only configured, currently unlocked illustration IDs.
pub async fn mark_illustrations_seen(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    ids: &[i32],
) -> sqlx::Result<()> {
    if season != 1 || ids.iter().any(|id| *id <= 0) {
        return Err(sqlx::Error::Protocol(
            "invalid Rouge illustration mark".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    for id in ids {
        sqlx::query("INSERT INTO user_rouge_illustration_seen(user_id,season,illustration_id) VALUES(?,?,?) ON CONFLICT DO NOTHING")
            .bind(user_id).bind(season).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await
}

#[cfg(test)]
#[path = "rouge_reviews_tests.rs"]
mod tests;
