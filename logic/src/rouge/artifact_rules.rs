//! Base-Rouge artefact rules with explicit activation and layout semantics.
//!
//! Evidence: `rouge_desc`/`language_en`, `rouge_item_static_desc`, client editor
//! geometry, and the user-supplied 2026-11-12 Collection of Artefacts MHTML.
//! The Wiki confirms 195 of the 198 base editor IDs (the other three are ending
//! props), but this evaluator implements only a subset of their effects.
//! This module never interprets arbitrary prose at runtime. Unsupported
//! scripted effects are reported instead of invented.
use super::bag::{BagItem, RunBag};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[path = "artifact_metadata.rs"]
mod geometry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedEffect {
    pub uid: i64,
    pub item_id: i32,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleModifiers {
    /// Base-stat percentage rates or extended-stat points, in per-mille.
    /// IDs are the battle engine's AttrId values (205/206 are DMG Bonus/Taken
    /// Reduction; 207/208 are their separate final-damage counterparts).
    pub attribute_rates: BTreeMap<i32, i32>,
    pub slot_attribute_rates: BTreeMap<i32, BTreeMap<i32, i32>>,
    pub coin_drop_bonus_per_mille: i32,
    pub shop_price_reduction_per_mille: i32,
    pub power_limit_bonus: i32,
    pub unsupported: Vec<UnsupportedEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapTrigger {
    Step,
    EnterSafeZone,
    BattleWin,
    EnterBattle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapState {
    /// Persistent per-instance counters; moving an item to the warehouse does
    /// not reset its progress, and stored items do not advance their counters.
    pub counters: BTreeMap<i64, i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapOutcome {
    pub coin: i32,
    pub power: i32,
    /// Fraction of maximum life, per-mille. This does not revive Lost heroes.
    pub heal_per_mille: i32,
    pub consume_uids: Vec<i64>,
    pub state: MapState,
    pub unsupported: Vec<UnsupportedEffect>,
}

fn unsupported(item: &BagItem, reason: &'static str) -> UnsupportedEffect {
    UnsupportedEffect {
        uid: item.uid,
        item_id: item.item_id,
        reason,
    }
}

fn active(item: &BagItem) -> bool {
    RunBag::is_placed(item) && (0..=3).contains(&item.rotation)
}

/// Inserting a Disk adds its mechanic tag to the holder, not the Disk's
/// category tag. Inserted disks are not independent placed artefacts.
pub fn effective_tags(item: &BagItem) -> BTreeSet<i32> {
    let mut tags: BTreeSet<i32> = geometry::get(item.item_id)
        .map(|meta| meta.tags.iter().copied().collect())
        .unwrap_or_default();
    for disk in item.holes.iter().filter(|id| **id > 0) {
        if let Some(meta) = geometry::get(*disk) {
            tags.extend(meta.tags.iter().copied().filter(|tag| *tag < 100));
        }
    }
    tags
}

/// Explicit enhancement counters from the static table, including repeated
/// inserted disks. No bonus is guessed for a mismatching slot/tag.
pub fn inlay_counters(tables: &config::GameDB, item: &BagItem) -> BTreeMap<String, i32> {
    let mut result = BTreeMap::new();
    let Some(row) = tables.rouge_item_static_desc.get(item.item_id) else {
        return result;
    };
    for disk in item.holes.iter().filter(|id| **id > 0) {
        for (id, attributes) in [
            (row.item1, &row.item1_attr),
            (row.item2, &row.item2_attr),
            (row.item3, &row.item3_attr),
        ] {
            if id != *disk {
                continue;
            }
            for attribute in attributes.split('|') {
                if let Some((key, amount)) = attribute.split_once('#')
                    && let Ok(amount) = amount.parse::<i32>()
                {
                    let total: &mut i32 = result.entry(key.to_owned()).or_default();
                    *total = total.saturating_add(amount);
                }
            }
        }
    }
    result
}

fn rotate((row, col): (i32, i32), meta: geometry::Metadata, rotation: i32) -> (i32, i32) {
    match rotation {
        0 => (row, col),
        1 => (col, meta.height - row - 1),
        2 => (meta.height - row - 1, meta.width - col - 1),
        3 => (meta.width - col - 1, row),
        _ => (row, col),
    }
}

/// A target is within range when any of its occupied cells touches the
/// source's editor effect-area cells. Diagonals are not assumed to count.
pub fn within_range(source: &BagItem, target: &BagItem) -> bool {
    if !active(source) || !active(target) || source.uid == target.uid {
        return false;
    }
    let (Some(source_meta), Some(target_meta)) =
        (geometry::get(source.item_id), geometry::get(target.item_id))
    else {
        return false;
    };
    let cells: BTreeSet<(i32, i32)> = source_meta
        .effect_area
        .iter()
        .map(|point| {
            let (row, col) = rotate(*point, source_meta, source.rotation);
            (row + source.row, col + source.col)
        })
        .collect();
    target_meta.shape.iter().any(|point| {
        let (row, col) = rotate(*point, target_meta, target.rotation);
        cells.contains(&(row + target.row, col + target.col))
    })
}

fn add_rate(result: &mut BattleModifiers, id: i32, rate: i32) {
    let current = result.attribute_rates.entry(id).or_default();
    *current = current.saturating_add(rate);
}

fn add_four_stats(result: &mut BattleModifiers, rate: i32) {
    for id in [101, 102, 103, 104] {
        add_rate(result, id, rate);
    }
}

/// Numeric static effects only. Same-attribute contributions add in this
/// local implementation; special multiplicative interactions remain pending.
pub fn evaluate_battle(tables: &config::GameDB, bag: &RunBag, coin: i32) -> BattleModifiers {
    let mut result = BattleModifiers::default();
    let items: Vec<&BagItem> = bag.items.iter().filter(|item| active(item)).collect();
    let count_category = |category| {
        items
            .iter()
            .filter(|item| {
                geometry::get(item.item_id).is_some_and(|meta| meta.category == category)
            })
            .count() as i32
    };
    // Disks inside an active holder remain part of the layout for explicit
    // Disk-count effects. They do not independently trigger "when in layout"
    // entry effects. Maze Chaperone below explicitly excludes inserted disks.
    // This common interpretation is kept separate from physical cell counts.
    let disks = count_category(105)
        + items
            .iter()
            .flat_map(|item| &item.holes)
            .filter(|id| geometry::get(**id).is_some_and(|meta| meta.category == 105))
            .count() as i32;
    for item in &items {
        let Some(_meta) = geometry::get(item.item_id) else {
            result
                .unsupported
                .push(unsupported(item, "missing base-season editor metadata"));
            continue;
        };
        let counters = inlay_counters(tables, item);
        let count2 = counters.get("count2").copied().unwrap_or(0);
        match item.item_id {
            8110021 => {
                let neighbors = items
                    .iter()
                    .filter(|other| other.item_id == 8110021 && within_range(item, other))
                    .count() as i32;
                add_rate(&mut result, 201, 50 + count2 * 10 + neighbors * 50);
            }
            8130315 => {
                // The description explicitly says this effect is unstackable.
                let slot = result.slot_attribute_rates.entry(1).or_default();
                for id in [103, 104] {
                    slot.insert(id, 100);
                }
                result.unsupported.push(unsupported(
                    item,
                    "Taunt on entry/rank-2-or-3 cast needs a combat trigger",
                ));
            }
            8210113 => {
                add_rate(&mut result, 213, 100);
                result.unsupported.push(unsupported(
                    item,
                    "conditional Aid penetration and below-layout bonus need a cast trigger",
                ));
            }
            8310212 => add_rate(&mut result, 211, disks * 40),
            8310405 => {
                add_rate(&mut result, 205, 200 + count2 * 10);
                result.unsupported.push(unsupported(
                    item,
                    "permanent Syntony growth needs per-run combat counters",
                ));
            }
            8310609 => {
                // The Chinese Wiki explicitly describes an additional 10%
                // per 50 coins, with that additional part capped at 60%.
                let coin_bonus = (coin.max(0) / 50).min(6) * 100;
                add_rate(&mut result, 213, 200 + count2 * 10 + coin_bonus);
            }
            8320049 => add_four_stats(&mut result, count_category(102) * 20),
            8320109 => {
                result.power_limit_bonus += 50;
                result.unsupported.push(unsupported(
                    item,
                    "end-of-round shield needs Aid-cast tracking",
                ));
            }
            8220612 => add_rate(
                &mut result,
                206,
                (coin.max(0) / 50 * 100).min(1000) + count2 * 10,
            ),
            8320618 => result.coin_drop_bonus_per_mille += 200,
            8330001 => result.shop_price_reduction_per_mille += 100,
            8330003 => {
                // The client explicitly excludes inserted disks from this count.
                // Ancient Chalice is rare/showRare 4 in the supplied client,
                // but is displayed as Epic (3) in the saved Wiki. Preserve the
                // client classification and report this unresolved combination.
                if items.iter().any(|other| other.item_id == 8320025) {
                    result.unsupported.push(unsupported(
                        item,
                        "Ancient Chalice rarity differs between client and Wiki; excluded from rarity count pending evidence",
                    ));
                }
                let rate = items
                    .iter()
                    .filter_map(|other| geometry::get(other.item_id))
                    .map(|meta| match meta.rarity {
                        1 => 5,
                        2 => 10,
                        3 => 15,
                        _ => 0,
                    })
                    .sum();
                add_four_stats(&mut result, rate);
            }
            8330020 => {
                add_rate(&mut result, 217, 1000 + count2 * 10);
                // Keep the known value for an eventual engine implementation,
                // but do not describe it as active: the current HeroEx/Sp
                // protocol path has no Genesis-DMG field and combat::apply
                // explicitly rejects AttrId 217.
                result.unsupported.push(unsupported(
                    item,
                    "Genesis damage bonus cannot be forwarded by the current battle protocol",
                ));
                result.unsupported.push(unsupported(
                    item,
                    "random negative statuses at round start need a combat trigger",
                ));
            }
            8330218 => add_rate(&mut result, 214, disks * 20),
            8330418 => {
                add_rate(&mut result, 206, 200 + count2 * 10);
                result.unsupported.push(unsupported(
                    item,
                    "permanent Syntony growth needs per-run combat counters",
                ));
            }
            8330607 => {
                result.shop_price_reduction_per_mille += 150;
                result.unsupported.push(unsupported(
                    item,
                    "five random permanent buffs on entry need combat support",
                ));
            }
            8230024 | 8130605 | 8140101 | 8240102 => {}
            8220613 => result.unsupported.push(unsupported(
                item,
                "safe-zone coins supported; entry spending/Blessing needs combat support",
            )),
            8210045 => result.unsupported.push(unsupported(
                item,
                "safe-zone healing supported; on-kill statuses need combat support",
            )),
            _ => result.unsupported.push(unsupported(
                item,
                "scripted artefact effect not yet implemented",
            )),
        }
    }
    // Stacking discounts use additive percentage points, capped at a free item.
    // This is an explicit local policy until a multi-discount sample is recorded.
    result.shop_price_reduction_per_mille = result.shop_price_reduction_per_mille.clamp(0, 1000);
    result
}

/// Computes event deltas without mutating the run. Callers commit the counter,
/// resource deltas, item consumption, and node transition in one transaction.
pub fn evaluate_map(
    _tables: &config::GameDB,
    bag: &RunBag,
    trigger: MapTrigger,
    state: &MapState,
) -> MapOutcome {
    let mut result = MapOutcome {
        state: state.clone(),
        ..Default::default()
    };
    for item in bag.items.iter().filter(|item| active(item)) {
        match (trigger, item.item_id) {
            (MapTrigger::Step, 8230024) => {
                result.power += 5;
                result.heal_per_mille += 200;
            }
            (MapTrigger::Step, 8130605) => {
                let counter = result.state.counters.entry(item.uid).or_default();
                *counter = (*counter).max(0).saturating_add(10).min(100);
                if *counter >= 100 {
                    result.coin += 100;
                    result.consume_uids.push(item.uid);
                    result.state.counters.remove(&item.uid);
                }
            }
            (MapTrigger::EnterSafeZone, 8220613) => result.coin += 50,
            (MapTrigger::EnterSafeZone, 8210045) => result.heal_per_mille += 200,
            (MapTrigger::EnterBattle, 8140101) => result.power += 2,
            (MapTrigger::EnterBattle, 8240102) => result.power += 4,
            _ => {}
        }
    }
    result.heal_per_mille = result.heal_per_mille.clamp(0, 1000);
    result
}

#[cfg(test)]
mod tests {
    use super::super::collection_shapes;
    use super::*;

    fn tables() -> &'static config::GameDB {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
        config::configs::get()
    }

    fn item(uid: i64, item_id: i32, row: i32, col: i32) -> BagItem {
        BagItem {
            uid,
            item_id,
            row,
            col,
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

    #[test]
    fn editor_shape_coordinates_match_existing_placement_for_every_base_item() {
        assert_eq!(geometry::ITEM_IDS.len(), 198);
        for id in geometry::ITEM_IDS {
            let meta = geometry::get(*id).unwrap();
            for rotation in 0..=3 {
                let mut actual: Vec<_> = meta
                    .shape
                    .iter()
                    .map(|cell| rotate(*cell, meta, rotation))
                    .collect();
                actual.sort_unstable();
                let mut expected = collection_shapes::cells(*id, rotation)
                    .unwrap_or_else(|| panic!("missing base shape {id}"));
                expected.sort_unstable();
                assert_eq!(actual, expected, "item {id}, rotation {rotation}");
            }
        }
        assert!(geometry::get(38010101).is_none());
        assert!(
            geometry::get(8320062).is_none(),
            "DLC 101 must not enter the base catalog"
        );
    }

    #[test]
    fn range_uses_rotated_editor_cells_and_target_occupied_shape() {
        let mut source = item(1, 8110021, 3, 3);
        assert!(within_range(&source, &item(2, 8140401, 2, 3)));
        assert!(
            !within_range(&source, &item(3, 8140401, 2, 2)),
            "diagonal corners are outside range"
        );
        assert!(
            within_range(&source, &item(4, 8130804, 1, 3)),
            "the target's second cell intersects range"
        );
        assert!(within_range(&source, &item(5, 8140401, 3, 5)));
        source.rotation = 1;
        assert!(!within_range(&source, &item(5, 8140401, 3, 5)));
        assert!(within_range(&source, &item(6, 8140401, 5, 3)));
        assert!(!within_range(&source, &source));
        assert!(!within_range(&source, &item(7, 8140401, -1, -1)));
    }

    #[test]
    fn crude_staffs_stack_only_for_active_staffs_in_actual_range() {
        let layout = bag(vec![
            item(1, 8110021, 3, 1),
            item(2, 8110021, 3, 3),
            item(3, 8110021, -1, -1),
        ]);
        let result = evaluate_battle(tables(), &layout, 0);
        assert_eq!(result.attribute_rates.get(&201), Some(&200));
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn inserted_disks_add_only_mechanic_tags_and_explicit_static_counters() {
        let mut host = item(1, 8310405, 2, 2);
        host.holes = vec![8240402];
        host.hole_ids = vec![2];
        assert_eq!(inlay_counters(tables(), &host).get("count2"), Some(&10));
        assert_eq!(
            evaluate_battle(tables(), &bag(vec![host.clone()]), 0)
                .attribute_rates
                .get(&205),
            Some(&300)
        );
        host.holes = vec![8140101];
        let tags = effective_tags(&host);
        assert!(tags.contains(&1));
        assert!(tags.contains(&103));
        assert!(
            !tags.contains(&105),
            "a holder must not become a Disk by insertion"
        );
        assert!(
            inlay_counters(tables(), &host).is_empty(),
            "a mismatching disk only adds its tag"
        );
        assert_eq!(
            evaluate_battle(tables(), &bag(vec![host]), 0)
                .attribute_rates
                .get(&205),
            Some(&200)
        );
    }

    #[test]
    fn layout_counts_exclude_warehouse_and_inserted_disks() {
        let mut alicorn = item(3, 8310405, 3, 0);
        alicorn.holes = vec![8140401];
        alicorn.hole_ids = vec![5];
        let layout = bag(vec![
            item(1, 8330003, 0, 0),
            item(2, 8320049, 0, 3),
            alicorn,
            item(4, 8320618, -1, -1),
        ]);
        let result = evaluate_battle(tables(), &layout, 0);
        // Maze: three Epics = 45 per-mille. Laurel: one Curio = 20.
        for stat in [101, 102, 103, 104] {
            assert_eq!(result.attribute_rates.get(&stat), Some(&65));
        }
        assert_eq!(result.coin_drop_bonus_per_mille, 0);
        assert_eq!(result.attribute_rates.get(&205), Some(&250));
    }

    #[test]
    fn disk_count_might_includes_inlays_without_triggering_them_as_placed_items() {
        let mut sword = item(1, 8310212, 0, 0);
        // Injecting a disk into a holder here isolates counting semantics;
        // public inlay requests separately validate each holder's hole count.
        sword.holes = vec![8140101];
        sword.hole_ids = vec![5];
        let layout = bag(vec![
            sword,
            item(2, 8330218, 0, 3),
            item(3, 8140401, 3, 0),
            item(4, 8240402, 3, 2),
        ]);
        let result = evaluate_battle(tables(), &layout, 0);
        assert_eq!(result.attribute_rates.get(&211), Some(&120));
        assert_eq!(result.attribute_rates.get(&214), Some(&60));
    }

    #[test]
    fn maze_keeps_client_special_rarity_and_reports_the_chalice_discrepancy() {
        assert_eq!(geometry::get(8320025).unwrap().rarity, 4);
        let layout = bag(vec![item(1, 8330003, 0, 0), item(2, 8320025, 0, 3)]);
        let result = evaluate_battle(tables(), &layout, 0);
        for stat in [101, 102, 103, 104] {
            assert_eq!(result.attribute_rates.get(&stat), Some(&15));
        }
        assert!(result.unsupported.iter().any(|effect| {
            effect.item_id == 8330003 && effect.reason.contains("Ancient Chalice rarity differs")
        }));
    }

    #[test]
    fn percentage_effects_use_engine_attribute_ids_and_unstackable_slot_defense() {
        let layout = bag(vec![
            item(1, 8220612, 0, 0),
            item(2, 8130315, 4, 0),
            item(3, 8130315, 4, 3),
        ]);
        let result = evaluate_battle(tables(), &layout, 250);
        assert_eq!(result.attribute_rates.get(&206), Some(&500));
        assert!(!result.attribute_rates.contains_key(&208));
        assert_eq!(
            result.slot_attribute_rates.get(&1),
            Some(&BTreeMap::from([(103, 100), (104, 100)]))
        );
        assert!(!result.slot_attribute_rates.contains_key(&2));
        assert_eq!(
            evaluate_battle(tables(), &layout, 2000)
                .attribute_rates
                .get(&206),
            Some(&1000)
        );
    }

    #[test]
    fn scepter_coin_threshold_and_cap_do_not_swallow_the_base_or_inlay_bonus() {
        let layout = bag(vec![item(1, 8310609, 0, 0)]);
        for (coin, expected) in [
            (-1, 200),
            (0, 200),
            (49, 200),
            (50, 300),
            (299, 700),
            (300, 800),
            (1000, 800),
        ] {
            assert_eq!(
                evaluate_battle(tables(), &layout, coin)
                    .attribute_rates
                    .get(&213),
                Some(&expected),
                "coin {coin}"
            );
        }
        let mut enhanced = layout;
        enhanced.items[0].holes = vec![8240602];
        enhanced.items[0].hole_ids = vec![7];
        assert_eq!(
            evaluate_battle(tables(), &enhanced, 1000)
                .attribute_rates
                .get(&213),
            Some(&1000),
            "the 60% cap only limits the additional coin-dependent part"
        );
    }

    #[test]
    fn known_genesis_value_stays_explicitly_unsupported_by_current_protocol() {
        let result = evaluate_battle(tables(), &bag(vec![item(1, 8330020, 0, 0)]), 0);
        assert_eq!(result.attribute_rates.get(&217), Some(&1000));
        assert!(result.unsupported.iter().any(|effect| {
            effect.item_id == 8330020
                && effect.reason
                    == "Genesis damage bonus cannot be forwarded by the current battle protocol"
        }));
    }

    #[test]
    fn reward_and_shop_modifiers_are_separate_and_only_placed_items_apply() {
        let layout = bag(vec![
            item(1, 8320618, 0, 0),
            item(2, 8330001, 0, 3),
            item(3, 8330607, 3, 3),
            item(4, 8330001, -1, -1),
        ]);
        let result = evaluate_battle(tables(), &layout, 0);
        assert_eq!(result.coin_drop_bonus_per_mille, 200);
        assert_eq!(result.shop_price_reduction_per_mille, 250);
        assert!(
            result
                .unsupported
                .iter()
                .any(|effect| effect.item_id == 8330607)
        );
    }

    #[test]
    fn step_and_safe_zone_grants_use_their_own_trigger_and_do_not_revive() {
        let layout = bag(vec![
            item(1, 8230024, 0, 0),
            item(2, 8220613, 0, 3),
            item(3, 8210045, 4, 0),
            item(4, 8230024, -1, -1),
        ]);
        let state = MapState::default();
        let step = evaluate_map(tables(), &layout, MapTrigger::Step, &state);
        assert_eq!((step.coin, step.power, step.heal_per_mille), (0, 5, 200));
        let safe = evaluate_map(tables(), &layout, MapTrigger::EnterSafeZone, &state);
        assert_eq!((safe.coin, safe.power, safe.heal_per_mille), (50, 0, 200));
        assert_eq!(
            evaluate_map(tables(), &layout, MapTrigger::BattleWin, &state).heal_per_mille,
            0
        );
    }

    #[test]
    fn pouch_progress_persists_and_stored_pouches_pause_instead_of_resetting() {
        let mut layout = bag(vec![item(1, 8130605, 0, 0)]);
        let mut state = MapState::default();
        for _ in 0..9 {
            let result = evaluate_map(tables(), &layout, MapTrigger::Step, &state);
            assert_eq!(result.coin, 0);
            assert!(result.consume_uids.is_empty());
            state = result.state;
        }
        assert_eq!(state.counters.get(&1), Some(&90));
        let encoded = serde_json::to_string(&state).unwrap();
        state = serde_json::from_str(&encoded).unwrap();
        layout.items[0].row = -1;
        layout.items[0].col = -1;
        assert_eq!(
            evaluate_map(tables(), &layout, MapTrigger::Step, &state).state,
            state
        );
        layout.items[0].row = 0;
        layout.items[0].col = 0;
        let result = evaluate_map(tables(), &layout, MapTrigger::Step, &state);
        assert_eq!(result.coin, 100);
        assert_eq!(result.consume_uids, vec![1]);
        assert!(result.state.counters.is_empty());
        layout.items.clear();
        assert_eq!(
            evaluate_map(tables(), &layout, MapTrigger::Step, &result.state).coin,
            0
        );
    }

    #[test]
    fn only_placed_support_disks_recover_stimulus_on_entry() {
        let mut host = item(3, 8310405, 3, 0);
        host.holes = vec![8140101];
        host.hole_ids = vec![9];
        let layout = bag(vec![
            item(1, 8140101, 0, 0),
            item(2, 8240102, 0, 2),
            host,
            item(4, 8140101, -1, -1),
        ]);
        let result = evaluate_map(
            tables(),
            &layout,
            MapTrigger::EnterBattle,
            &MapState::default(),
        );
        assert_eq!(result.power, 6);
    }

    #[test]
    fn dlc_and_unknown_effects_are_explicitly_unsupported() {
        let result = evaluate_battle(
            tables(),
            &bag(vec![item(1, 8320062, 0, 0), item(2, 8120034, 3, 0)]),
            0,
        );
        assert!(result.attribute_rates.is_empty());
        assert_eq!(result.unsupported.len(), 2);
        assert_eq!(
            result.unsupported[0].reason,
            "missing base-season editor metadata"
        );
        assert_eq!(
            result.unsupported[1].reason,
            "scripted artefact effect not yet implemented"
        );
    }
}
