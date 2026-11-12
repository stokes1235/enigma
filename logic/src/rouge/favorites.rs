//! Original-Rouge album facts, separate from drop-pool availability.
//!
//! Client RougeCollectionListView uses FavoriteModel's unlockCollectionIds to
//! distinguish locked/available, and GameRecordInfo.passCollections separately
//! for acquired/unacquired. RougeController sends UnlockStory on StoryStart,
//! not StoryFinish; accept a legitimate current story without pre-finishing it.

use super::{artifact_rules, flow};
use crate::error::AppError;
use database::{
    db::game::{rouge, rouge_favorites as store, rouge_history},
    models::game::rouge::RougeRun,
};
use sonettobuf::{NewReddotNo, RougeGetUnlockCollectionsReply, RougeUnlockStoryReply};
use sqlx::SqlitePool;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Real historical acquisitions, never the entire unlocked drop pool.
    pub pass_collections: Vec<i32>,
    /// Terminal story IDs used by RougeGameRecordInfo/RougeReviewItem.
    pub unlock_story_ids: Vec<i32>,
    pub new_collection_ids: Vec<i32>,
    /// Review-row IDs, not terminal story IDs, used by Story red dots.
    pub new_story_review_ids: Vec<i32>,
}

impl Progress {
    /// All four categories must exist: the client's getters index them directly.
    /// Faction/Illustration progress is not invented from album availability.
    pub fn reddots(&self) -> Vec<NewReddotNo> {
        (1..=4)
            .map(|kind| NewReddotNo {
                r#type: Some(kind),
                ids: match kind {
                    1 => self.new_collection_ids.clone(),
                    4 => self.new_story_review_ids.clone(),
                    _ => Vec::new(),
                },
            })
            .collect()
    }
}

/// Current base configuration uses only unconditional availability (0) and
/// active outside Genius nodes (9). Unknown/malformed gates fail closed.
/// This function deliberately has no acquired-collection argument.
pub fn unlock_collection_ids(tables: &config::GameDB, genius_ids: &[i32]) -> Vec<i32> {
    let base: BTreeSet<_> = artifact_rules::base_item_ids().iter().copied().collect();
    tables
        .rouge_collection_unlock
        .iter()
        .filter(|row| base.contains(&row.id))
        .filter(|row| match row.unlock_type {
            0 => true,
            9 => row.unlock_param.parse::<i32>().ok().is_some_and(|id| {
                genius_ids.contains(&id)
                    && tables
                        .rouge_genius_branch
                        .get(id)
                        .is_some_and(|node| node.season == 1)
            }),
            _ => false,
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn unlock_collections_reply(
    tables: &config::GameDB,
    season: i32,
    genius_ids: &[i32],
) -> Result<RougeGetUnlockCollectionsReply, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    Ok(RougeGetUnlockCollectionsReply {
        season: Some(season),
        unlock_collection_ids: unlock_collection_ids(tables, genius_ids),
    })
}

pub async fn load(db: &SqlitePool, user_id: i64, season: i32) -> Result<Progress, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    let collections = store::collections(db, user_id, season).await?;
    let stories = store::stories(db, user_id, season).await?;
    Ok(Progress {
        pass_collections: collections.iter().map(|row| row.collection_id).collect(),
        unlock_story_ids: stories.iter().map(|row| row.story_id).collect(),
        new_collection_ids: collections
            .iter()
            .filter(|row| !row.seen)
            .map(|row| row.collection_id)
            .collect(),
        new_story_review_ids: stories
            .iter()
            .filter(|row| !row.seen)
            .map(|row| row.review_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    })
}

/// FavoriteConfig maps only the final story in each review row to an unlock
/// request. Earlier parts of a multi-part ending are not separate reviews.
pub fn story_review_id(tables: &config::GameDB, season: i32, story_id: i32) -> Option<i32> {
    if season != 1 {
        return None;
    }
    tables
        .rouge_story_list
        .iter()
        .filter(|row| row.season == season)
        .find_map(|row| {
            (row.story_id_list
                .split('#')
                .next_back()?
                .parse::<i32>()
                .ok()?
                == story_id)
                .then_some(row.id)
        })
}

/// Pure playback eligibility for a requested review. Do not infer which old
/// layers/events were completed from a later layer number or another ending.
pub fn story_is_eligible(
    tables: &config::GameDB,
    season: i32,
    story_id: i32,
    run: Option<&RougeRun>,
    passed_endings: &[i32],
    observed_story_ids: &[i32],
) -> bool {
    if story_review_id(tables, season, story_id).is_none() {
        return false;
    }
    if observed_story_ids.contains(&story_id) {
        return true;
    }
    if tables.rouge_ending.iter().any(|ending| {
        ending.version.is_empty()
            && ending.ending_story_id == story_id
            && passed_endings.contains(&ending.id)
    }) {
        return true;
    }
    let Some(run) = run.filter(|run| run.season == season) else {
        return false;
    };
    if let Some(layer) = tables
        .rouge_layer
        .get(run.layer_id)
        .filter(|layer| layer.version.is_empty())
    {
        if layer.start_story_id == story_id {
            return true;
        }
        let layer_complete = run.map_type == super::MAP_TYPE_MIDDLE || run.terminal_state == 1;
        if layer_complete
            && layer
                .end_story_id
                .split('|')
                .any(|part| part.parse::<i32>().ok() == Some(story_id))
        {
            return true;
        }
    }
    run.terminal_state == 1
        && tables.rouge_ending.iter().any(|ending| {
            ending.version.is_empty()
                && ending.ending_story_id == story_id
                && flow::details(run).end_id == ending.id
        })
}

pub async fn unlock_story(
    db: &SqlitePool,
    user_id: i64,
    season: i32,
    story_id: i32,
) -> Result<RougeUnlockStoryReply, AppError> {
    let tables = config::configs::get();
    let review_id = story_review_id(tables, season, story_id).ok_or(AppError::InvalidRequest)?;
    let old = store::stories(db, user_id, season).await?;
    if old.iter().any(|row| row.story_id == story_id) {
        return Ok(RougeUnlockStoryReply {
            season: Some(season),
            story_id: Some(story_id),
        });
    }
    let run = rouge::get_run(db, user_id, season).await?;
    let endings = rouge_history::passed_endings(db, user_id, season).await?;
    let observed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_finished_stories WHERE user_id=? AND story_id=? UNION ALL SELECT 1 FROM user_processing_stories WHERE user_id=? AND story_id=?)")
        .bind(user_id).bind(story_id).bind(user_id).bind(story_id).fetch_one(db).await?;
    let observed_ids = if observed { vec![story_id] } else { Vec::new() };
    if !story_is_eligible(
        tables,
        season,
        story_id,
        run.as_ref(),
        &endings,
        &observed_ids,
    ) {
        return Err(AppError::InvalidRequest);
    }
    let mut tx = db.begin().await?;
    store::record_story_in_transaction(
        &mut tx,
        user_id,
        season,
        story_id,
        review_id,
        common::time::ServerTime::now_ms(),
    )
    .await?;
    tx.commit().await?;
    Ok(RougeUnlockStoryReply {
        season: Some(season),
        story_id: Some(story_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> &'static config::GameDB {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(path.to_str().unwrap()).unwrap();
        config::configs::get()
    }

    #[test]
    fn drop_pool_availability_is_separate_from_acquisitions_and_dlc() {
        let tables = tables();
        let initial = unlock_collection_ids(tables, &[]);
        assert!(initial.contains(&8110021));
        assert!(
            !initial.contains(&8110410),
            "outside Genius 401 is required even if previously acquired"
        );
        let grown = unlock_collection_ids(tables, &[401]);
        assert!(grown.contains(&8110410));
        assert!(
            initial
                .iter()
                .all(|id| artifact_rules::base_item_ids().contains(id))
        );
        assert!(
            !grown.contains(&8320062),
            "DLC remains outside the base catalog"
        );
        let progress = Progress::default();
        assert!(
            progress.pass_collections.is_empty(),
            "default availability does not record acquisition"
        );
    }

    #[test]
    fn story_requests_accept_only_terminal_review_ids_and_matching_evidence() {
        let tables = tables();
        assert_eq!(story_review_id(tables, 1, 600909), Some(8));
        assert_eq!(story_review_id(tables, 1, 600908), None);
        assert!(!story_is_eligible(tables, 1, 600909, None, &[], &[]));
        assert!(story_is_eligible(tables, 1, 600909, None, &[101], &[]));
        assert!(!story_is_eligible(tables, 1, 600911, None, &[101], &[]));
        assert!(story_is_eligible(tables, 1, 600902, None, &[], &[600902]));
        assert!(!story_is_eligible(tables, 2, 600909, None, &[101], &[]));
        assert!(!story_is_eligible(tables, 1, 999, None, &[], &[999]));
    }

    #[test]
    fn first_story_is_available_on_start_but_layer_end_and_ending_need_progress() {
        let tables = tables();
        let mut run = RougeRun {
            user_id: 1,
            season: 1,
            layer_index: 0,
            layer_id: 111,
            stage: 0,
            node_id: 0,
            seed: 1,
            graph: "{}".into(),
            coin: 0,
            power: 0,
            updated_at: 0,
            map_type: super::super::MAP_TYPE_NORMAL,
            middle_layer_id: 0,
            position_index: 0,
            choices: "{}".into(),
            finished: "[]".into(),
            bag: String::new(),
            shop: "{}".into(),
            spent_coin: 0,
            retry_num: 0,
            terminal_state: 0,
            minor_count: 0,
            danger_count: 0,
            crisis_count: 0,
            step_count: 0,
            team: "{}".into(),
            power_limit: 50,
            interactive_index: 0,
            interactive_refresh: 0,
            battle_interactive: String::new(),
            team_exp: 0,
            middle_state: "{}".into(),
            details: "{}".into(),
        };
        assert!(story_is_eligible(tables, 1, 600901, Some(&run), &[], &[]));
        assert!(!story_is_eligible(tables, 1, 600902, Some(&run), &[], &[]));
        run.map_type = super::super::MAP_TYPE_MIDDLE;
        assert!(story_is_eligible(tables, 1, 600902, Some(&run), &[], &[]));
        assert!(story_is_eligible(tables, 1, 600903, Some(&run), &[], &[]));
        run.layer_id = 121;
        run.map_type = super::super::MAP_TYPE_NORMAL;
        assert!(
            !story_is_eligible(tables, 1, 600902, Some(&run), &[], &[]),
            "do not guess old missing history from a later layer"
        );
        assert!(!story_is_eligible(tables, 1, 600904, Some(&run), &[], &[]));
        run.layer_id = 153;
        run.terminal_state = 2;
        run.details = r#"{"end_id":103}"#.into();
        assert!(!story_is_eligible(tables, 1, 600915, Some(&run), &[], &[]));
        run.terminal_state = 1;
        assert!(story_is_eligible(tables, 1, 600915, Some(&run), &[], &[]));
        assert!(!story_is_eligible(tables, 1, 600919, Some(&run), &[], &[]));
    }

    #[test]
    fn reddots_always_initialize_all_types_and_story_ids_use_review_rows() {
        let progress = Progress {
            pass_collections: vec![8110021],
            unlock_story_ids: vec![600909],
            new_collection_ids: vec![8110021],
            new_story_review_ids: vec![8],
        };
        let dots = progress.reddots();
        assert_eq!(dots.len(), 4);
        assert_eq!(dots[0].ids, vec![8110021]);
        assert!(dots[1].ids.is_empty() && dots[2].ids.is_empty());
        assert_eq!(dots[3].ids, vec![8]);
        assert_eq!(dots[3].r#type, Some(4));
    }
}
