//! Persist verified artefact growth independently of static Disk enhancements.
//!
//! The battle engine returns cumulative NEW growth within one fight under
//! `growth_pct`, in percentage points. It must never include the old baseline
//! or inlay count2. Frozen sources bind that report to the accepted fight's
//! artefacts and actors. Commit the updated MapState with battle settlement.

use super::{
    artifact_rules,
    bag::{BagItem, RunBag},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Counters = BTreeMap<i64, BTreeMap<String, i32>>;
pub const GROWTH_KEY: &str = "growth_pct";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrowthSource {
    /// Artefact instance UID, not an actor UID.
    pub uid: i64,
    pub item_id: i32,
    /// Effective mechanic tags include matching/mismatching inserted Disks.
    pub syntony_artefact_count: i32,
    /// Physical layout count only, using the client's raw rarity >= 2.
    pub rare_artefact_count: i32,
    /// Frozen allied actor UIDs bound the per-character Scarab triggers.
    pub actor_uids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedGrowth {
    pub uid: i64,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeResult {
    pub added_growth_pct: BTreeMap<i64, i32>,
    pub rejected: Vec<RejectedGrowth>,
}

pub fn attribute_id(item_id: i32) -> Option<i32> {
    match item_id {
        8310405 => Some(205),
        8330418 => Some(206),
        8320310 => Some(214),
        _ => None,
    }
}

/// Preserve this snapshot with the battle seed/checkpoint. Do not reconstruct
/// the qualifying layout or actor list from a bag edited after fight entry.
pub fn frozen_sources(bag: &RunBag, actor_uids: &[i64]) -> Vec<GrowthSource> {
    let active: Vec<_> = bag
        .items
        .iter()
        .filter(|item| artifact_rules::is_active(item))
        .collect();
    let syntony = active
        .iter()
        .filter(|item| artifact_rules::has_tag(item, 4))
        .count() as i32;
    let rare = active
        .iter()
        .filter(|item| {
            artifact_rules::collection_rarity(item.item_id).is_some_and(|rarity| rarity >= 2)
        })
        .count() as i32;
    let actors: Vec<_> = actor_uids
        .iter()
        .copied()
        .filter(|uid| *uid > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut result: Vec<_> = active
        .into_iter()
        .filter(|item| attribute_id(item.item_id).is_some())
        .map(|item| GrowthSource {
            uid: item.uid,
            item_id: item.item_id,
            syntony_artefact_count: syntony,
            rare_artefact_count: rare,
            actor_uids: actors.clone(),
        })
        .collect();
    result.sort_by_key(|source| source.uid);
    result
}

/// Percentage points for the client expression's count2, excluding inlays.
/// Warehouse displays keep this permanent value; their client adds Disk
/// enhancement counters itself, while placed displays receive the sum.
pub fn permanent_count2(item: &BagItem, state: &artifact_rules::MapState) -> i32 {
    attribute_id(item.item_id)
        .and_then(|attr| state.permanent_rates.get(&item.uid)?.get(&attr).copied())
        .unwrap_or_default()
        .max(0)
        / 10
}

/// Freeze this into RougeContext.permanent_growth for provenance/replay only.
/// evaluate_battle_with_state already applies these rates to initial allies;
/// the engine must not apply this baseline again, and returns NEW growth only.
pub fn initial_growth(bag: &RunBag, state: &artifact_rules::MapState) -> Counters {
    bag.items
        .iter()
        .filter(|item| artifact_rules::is_active(item) && attribute_id(item.item_id).is_some())
        .map(|item| {
            (
                item.uid,
                BTreeMap::from([(GROWTH_KEY.to_owned(), permanent_count2(item, state))]),
            )
        })
        .collect()
}

fn battle_limit(source: &GrowthSource) -> i32 {
    match source.item_id {
        8310405 | 8330418 => {
            4 + if source.syntony_artefact_count >= 10 {
                8
            } else {
                0
            }
        }
        // The unique item's global count2 grows on each actor's qualifying
        // cast. The once-per-actor guards belong to the battle event hook;
        // this independent bound rejects accidentally unbounded reports.
        8320310 => {
            let actors = source
                .actor_uids
                .iter()
                .copied()
                .filter(|uid| *uid > 0)
                .collect::<BTreeSet<_>>()
                .len() as i32;
            actors.saturating_mul(if source.rare_artefact_count >= 10 {
                6
            } else {
                2
            })
        }
        _ => 0,
    }
}

/// Reports are monotone checkpoints within one fight, not increments to apply
/// blindly. A repeated/decreased report adds nothing; a later larger report
/// adds only the new difference. A new fight has an independent watermark.
pub fn merge_battle_growth(
    state: &mut artifact_rules::MapState,
    battle_id: i64,
    sources: &[GrowthSource],
    report: &Counters,
) -> MergeResult {
    let mut result = MergeResult::default();
    if battle_id <= 0 {
        result.rejected.push(RejectedGrowth {
            uid: 0,
            reason: "missing persistent fight identity",
        });
        return result;
    }
    let source_map: BTreeMap<_, _> = sources.iter().map(|source| (source.uid, source)).collect();
    for (uid, counters) in report {
        let Some(raw) = counters.get(GROWTH_KEY).copied() else {
            continue;
        };
        let Some(source) = source_map.get(uid).filter(|source| source.uid > 0) else {
            result.rejected.push(RejectedGrowth {
                uid: *uid,
                reason: "growth source absent from frozen fight",
            });
            continue;
        };
        let Some(attr) = attribute_id(source.item_id) else {
            result.rejected.push(RejectedGrowth {
                uid: *uid,
                reason: "unimplemented permanent growth item",
            });
            continue;
        };
        if raw < 0 {
            result.rejected.push(RejectedGrowth {
                uid: *uid,
                reason: "negative new-growth checkpoint",
            });
            continue;
        }
        let limit = battle_limit(source);
        let accepted = raw.min(limit);
        if raw > limit {
            result.rejected.push(RejectedGrowth {
                uid: *uid,
                reason: "growth report exceeds frozen per-battle limit",
            });
        }
        let quantum = if source.item_id == 8320310 { 2 } else { 4 };
        if accepted % quantum != 0 {
            result.rejected.push(RejectedGrowth {
                uid: *uid,
                reason: "growth checkpoint is not a configured increment",
            });
            continue;
        }
        let previous = state
            .battle_growth
            .get(&battle_id)
            .and_then(|items| items.get(uid))
            .copied()
            .unwrap_or_default();
        if accepted <= previous {
            continue;
        }
        let delta = accepted - previous;
        let rate = state
            .permanent_rates
            .entry(*uid)
            .or_default()
            .entry(attr)
            .or_default();
        *rate = rate.saturating_add(delta.saturating_mul(10));
        state
            .battle_growth
            .entry(battle_id)
            .or_default()
            .insert(*uid, accepted);
        result.added_growth_pct.insert(*uid, delta);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(uid: i64, item_id: i32) -> BagItem {
        BagItem {
            uid,
            item_id,
            row: 0,
            col: 0,
            rotation: 0,
            holes: vec![],
            hole_ids: vec![],
        }
    }
    fn bag(items: Vec<BagItem>) -> RunBag {
        RunBag {
            rows: 8,
            cols: 8,
            items,
            uid_counter: 100,
        }
    }
    fn source(uid: i64, item_id: i32, qualified: bool) -> GrowthSource {
        GrowthSource {
            uid,
            item_id,
            syntony_artefact_count: if qualified { 10 } else { 9 },
            rare_artefact_count: if qualified { 10 } else { 9 },
            actor_uids: vec![11, 12],
        }
    }
    fn counters(uid: i64, amount: i32) -> Counters {
        BTreeMap::from([(uid, BTreeMap::from([(GROWTH_KEY.into(), amount)]))])
    }

    #[test]
    fn checkpoint_merge_survives_save_reload_and_is_independent_per_fight() {
        let mut state = artifact_rules::MapState::default();
        let sources = [source(1, 8310405, true)];
        state.permanent_rates.insert(1, BTreeMap::from([(205, 80)]));
        assert_eq!(
            merge_battle_growth(&mut state, 101, &sources, &counters(1, 4))
                .added_growth_pct
                .get(&1),
            Some(&4)
        );
        state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(
            merge_battle_growth(&mut state, 101, &sources, &counters(1, 4))
                .added_growth_pct
                .is_empty()
        );
        assert!(
            merge_battle_growth(&mut state, 101, &sources, &counters(1, 0))
                .added_growth_pct
                .is_empty()
        );
        assert_eq!(
            merge_battle_growth(&mut state, 101, &sources, &counters(1, 12))
                .added_growth_pct
                .get(&1),
            Some(&8)
        );
        assert_eq!(state.permanent_rates[&1][&205], 200);
        merge_battle_growth(&mut state, 102, &sources, &counters(1, 4));
        assert_eq!(state.permanent_rates[&1][&205], 240);
    }

    #[test]
    fn battle_bounds_use_frozen_tag_rarity_and_distinct_actor_counts() {
        let mut state = artifact_rules::MapState::default();
        let mut low = source(1, 8320310, false);
        low.actor_uids = vec![11, 11, 12, 0];
        let capped = merge_battle_growth(&mut state, 1, &[low], &counters(1, 100));
        assert_eq!(capped.added_growth_pct.get(&1), Some(&4));
        assert_eq!(capped.rejected.len(), 1);
        let high = source(2, 8320310, true);
        assert_eq!(
            merge_battle_growth(&mut state, 1, &[high], &counters(2, 12))
                .added_growth_pct
                .get(&2),
            Some(&12)
        );
        assert_eq!(state.permanent_rates[&2][&214], 120);
        let low_syntony = source(3, 8330418, false);
        assert_eq!(
            merge_battle_growth(&mut state, 1, &[low_syntony], &counters(3, 12))
                .added_growth_pct
                .get(&3),
            Some(&4)
        );
    }

    #[test]
    fn unknown_negative_or_wrong_quantum_reports_cannot_create_growth() {
        let mut state = artifact_rules::MapState::default();
        let sources = [source(1, 8310405, true), source(2, 8310309, true)];
        for (battle, report) in [
            (0, counters(1, 4)),
            (1, counters(999, 4)),
            (1, counters(1, -4)),
            (1, counters(1, 3)),
            (1, counters(2, 4)),
        ] {
            assert!(
                merge_battle_growth(&mut state, battle, &sources, &report)
                    .added_growth_pct
                    .is_empty()
            );
        }
        assert!(state.permanent_rates.is_empty() && state.battle_growth.is_empty());
    }

    #[test]
    fn initial_growth_excludes_inlays_and_stored_instances_without_erasing_their_progress() {
        let mut horn = item(1, 8310405);
        horn.holes = vec![8240402];
        horn.hole_ids = vec![21];
        let mut ship = item(2, 8330418);
        ship.row = -1;
        ship.col = -1;
        let layout = bag(vec![horn.clone(), ship.clone()]);
        let mut state = artifact_rules::MapState::default();
        state
            .permanent_rates
            .insert(1, BTreeMap::from([(205, 120)]));
        state.permanent_rates.insert(2, BTreeMap::from([(206, 40)]));
        assert_eq!(initial_growth(&layout, &state), counters(1, 12));
        assert_eq!(permanent_count2(&horn, &state), 12);
        assert_eq!(permanent_count2(&ship, &state), 4);
    }

    #[test]
    fn layout_freeze_counts_real_holders_and_added_mechanic_tags_not_inserted_items() {
        let mut horn = item(1, 8310405);
        horn.holes = vec![8140401];
        horn.hole_ids = vec![21];
        let mut other = item(2, 8110021);
        other.holes = vec![8140401];
        other.hole_ids = vec![22];
        let mut stored = item(3, 8330418);
        stored.row = -1;
        stored.col = -1;
        let sources = frozen_sources(&bag(vec![horn, other, stored]), &[11, 11, 12]);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].syntony_artefact_count, 2);
        assert_eq!(sources[0].rare_artefact_count, 1);
        assert_eq!(sources[0].actor_uids, vec![11, 12]);
    }
}
