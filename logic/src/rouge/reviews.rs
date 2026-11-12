//! Full run reports and evidence-based CG/faction album state.
//! Client evidence: RougeFavoriteModel:initReviews, ResultReportListModel's
//! timestamp ordering, IllustrationListItem's passedAnyEventId, and
//! RougeOutsideModel:isOpenedStyle. No unobserved past route is reconstructed.

use crate::error::AppError;
use database::db::game::rouge_reviews as store;
use sonettobuf::RougeReviewInfo;
use sqlx::SqlitePool;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub reviews: Vec<RougeReviewInfo>,
    pub pass_event_ids: Vec<i32>,
    pub pass_layer_ids: Vec<i32>,
    pub unlock_illustration_ids: Vec<i32>,
    pub new_illustration_ids: Vec<i32>,
    pub last_game_time: i64,
}

/// Match faction album and selection eligibility, using only supported base
/// prerequisites. Initial availability is not a claim of historical usage.
pub fn unlocked_styles(
    tables: &config::GameDB,
    season: i32,
    genius_ids: &[i32],
    passed_endings: &[i32],
) -> Vec<i32> {
    if season != 1 {
        return Vec::new();
    }
    tables
        .rouge_style
        .iter()
        .filter(|row| row.season == season && row.version.is_empty())
        .filter(|row| match row.unlock_type {
            0 => true,
            9 => row.unlock_param.parse::<i32>().ok().is_some_and(|id| {
                genius_ids.contains(&id)
                    && tables
                        .rouge_genius_branch
                        .get(id)
                        .is_some_and(|node| node.season == season)
            }),
            12 => passed_endings.iter().any(|id| {
                tables
                    .rouge_ending
                    .get(*id)
                    .is_some_and(|ending| ending.version.is_empty())
            }),
            13 => row
                .unlock_param
                .parse::<i32>()
                .ok()
                .is_some_and(|id| passed_endings.contains(&id)),
            _ => false,
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn unlocked_illustrations(
    tables: &config::GameDB,
    season: i32,
    passed_events: &[i32],
) -> Vec<i32> {
    if season != 1 {
        return Vec::new();
    }
    let passed: BTreeSet<_> = passed_events
        .iter()
        .copied()
        .filter(|id| {
            tables
                .rouge_event(*id)
                .is_some_and(|event| event.version.is_empty())
        })
        .collect();
    tables
        .rouge_illustration_list
        .iter()
        .filter(|row| row.season == season && row.dlc == 0)
        .filter(|row| {
            row.event_id
                .split('|')
                .filter_map(|id| id.parse::<i32>().ok())
                .any(|id| passed.contains(&id))
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// A malformed/partial old snapshot remains stored, but is not sent to Lua
/// views that dereference team, faction and difficulty data without nil checks.
fn playable_review(
    tables: &config::GameDB,
    season: i32,
    row: store::ReviewRecord,
) -> Option<RougeReviewInfo> {
    let mut review: RougeReviewInfo = serde_json::from_str(&row.review_info).ok()?;
    if review.season.is_some_and(|value| value != season)
        || !review.version.is_empty()
        || row.finish_time <= 0
        || tables.rouge_difficulty(review.difficulty?).is_none()
        || !tables
            .rouge_style(season, review.style?)
            .is_some_and(|style| style.version.is_empty())
        || review.team_info.is_none()
        || review.player_name.is_none()
        || review.player_level.is_none()
        || review.portrait.is_none()
        || review.team_level.is_none()
        || review.collection_num.is_none()
        || review.gain_coin.is_none()
    {
        return None;
    }
    let end_id = review.end_id.unwrap_or(0);
    if end_id == 0 {
        let middle = review.middle_layer_id.unwrap_or(0);
        if middle > 0 {
            tables
                .rouge_middle_layer
                .get(middle)
                .filter(|row| row.version.is_empty())?;
        } else {
            tables
                .rouge_layer
                .get(review.layer_id?)
                .filter(|row| row.version.is_empty())?;
        }
    } else {
        tables
            .rouge_ending
            .get(end_id)
            .filter(|row| row.version.is_empty())?;
    }
    review.season = Some(season);
    review.end_id = Some(end_id);
    review.middle_layer_id = Some(review.middle_layer_id.unwrap_or(0));
    review.finish_time = Some(row.finish_time);
    Some(review)
}

pub async fn load(db: &SqlitePool, user_id: i64, season: i32) -> Result<Progress, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    let tables = config::configs::get();
    let records = store::list(db, user_id, season).await?;
    let last_game_time = records.iter().map(|row| row.finish_time).max().unwrap_or(0);
    let reviews: Vec<_> = records
        .into_iter()
        .filter_map(|row| playable_review(tables, season, row))
        .collect();
    let pass_event_ids: Vec<_> = store::passed_events(db, user_id, season)
        .await?
        .into_iter()
        .filter(|id| {
            tables
                .rouge_event(*id)
                .is_some_and(|event| event.version.is_empty())
        })
        .collect();
    let pass_layer_ids = store::passed_layers(db, user_id, season)
        .await?
        .into_iter()
        .filter(|id| {
            tables
                .rouge_layer
                .get(*id)
                .is_some_and(|layer| layer.version.is_empty())
        })
        .collect();
    let unlock_illustration_ids = unlocked_illustrations(tables, season, &pass_event_ids);
    let seen: BTreeSet<_> = store::seen_illustrations(db, user_id, season)
        .await?
        .into_iter()
        .collect();
    let new_illustration_ids = unlock_illustration_ids
        .iter()
        .copied()
        .filter(|id| !seen.contains(id))
        .collect();
    Ok(Progress {
        reviews,
        pass_event_ids,
        pass_layer_ids,
        unlock_illustration_ids,
        new_illustration_ids,
        last_game_time,
    })
}

/// Mark only CGs whose real event prerequisites are present. ID zero marks the
/// currently unlocked set; an unrelated or still locked ID cannot be forged.
pub async fn mark_illustration_seen(
    db: &SqlitePool,
    user_id: i64,
    season: i32,
    id: i32,
) -> Result<bool, AppError> {
    if season != 1 || id < 0 {
        return Ok(false);
    }
    let events = store::passed_events(db, user_id, season).await?;
    let unlocked = unlocked_illustrations(config::configs::get(), season, &events);
    let ids = if id == 0 {
        unlocked
    } else if unlocked.contains(&id) {
        vec![id]
    } else {
        return Ok(false);
    };
    store::mark_illustrations_seen(db, user_id, season, &ids).await?;
    Ok(true)
}

#[cfg(test)]
#[path = "reviews_tests.rs"]
mod tests;
