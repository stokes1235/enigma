//! Original-Rouge Craftmaster's Favors (金匠预言).
//!
//! Evidence: the main wiki says cleared-area costs are 2/3/4/6/8. Exported
//! constants 8001/8002/8003 give 8 weekly recovery, 16 cap and the cost table.
//! Client text `rougemainview_extrapointtips` -> `language_10071605` explicitly
//! says login recovery every Monday 05:00. A captured five-area settlement
//! spends 14 -> 6 and adds 8 bonus Keepsakes.

use crate::error::AppError;
use sqlx::SqlitePool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub recover: i32,
    pub limit: i32,
}

pub fn settings() -> Settings {
    let tables = config::configs::get();
    Settings {
        recover: tables
            .rouge_const
            .get(8001)
            .and_then(|row| row.value.parse().ok())
            .unwrap_or(8)
            .max(0),
        limit: tables
            .rouge_const
            .get(8002)
            .and_then(|row| row.value.parse().ok())
            .unwrap_or(16)
            .max(0),
    }
}

pub async fn balance(db: &SqlitePool, user_id: i64, season: i32) -> Result<i32, AppError> {
    balance_at(db, user_id, season, common::time::ServerTime::now_ms()).await
}

pub async fn balance_at(
    db: &SqlitePool,
    user_id: i64,
    season: i32,
    now_ms: i64,
) -> Result<i32, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    let settings = settings();
    Ok(database::db::game::rouge_extra::refresh_weekly(
        db,
        user_id,
        season,
        now_ms,
        settings.recover,
        settings.limit,
    )
    .await?
    .balance)
}

/// Calculates a bonus without mutating the account. The expected balance and
/// returned amount must both be passed to the settlement transaction's CAS.
pub fn quote(layers: i32, balance: i32) -> i32 {
    if layers <= 0 || balance <= 0 {
        return 0;
    }
    let tables = config::configs::get();
    let cost = tables
        .rouge_const
        .get(8003)
        .into_iter()
        .flat_map(|row| row.value.split('|'))
        .filter_map(|entry| {
            let (layer, cost) = entry.split_once('#')?;
            Some((layer.parse::<i32>().ok()?, cost.parse::<i32>().ok()?))
        })
        .filter(|(layer, cost)| *layer > 0 && *layer <= layers && *cost >= 0)
        .max_by_key(|(layer, _)| *layer)
        .map(|(_, cost)| cost)
        .unwrap_or(0);
    cost.min(balance.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
    }

    #[test]
    fn weekly_settings_and_area_costs_match_exported_rules() {
        init();
        assert_eq!(
            settings(),
            Settings {
                recover: 8,
                limit: 16
            }
        );
        assert_eq!(
            (1..=5).map(|layers| quote(layers, 16)).collect::<Vec<_>>(),
            vec![2, 3, 4, 6, 8]
        );
        assert_eq!(quote(5, 14), 8);
        assert_eq!(quote(5, 6), 6);
        assert_eq!(quote(5, 1), 1);
        assert_eq!(quote(5, 0), 0);
        assert_eq!(quote(0, 14), 0);
        assert_eq!(quote(-1, 14), 0);
        assert_eq!(quote(5, -1), 0);
    }
}
