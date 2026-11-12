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

#[path = "artifact_attack.rs"]
mod attack;
#[path = "artifact_entry.rs"]
mod entry;
#[path = "artifact_metadata.rs"]
mod geometry;
pub use attack::{
    AttackContext, AttackPredicate, ConditionalModifier, attack_modifiers, evaluate_attack,
};
pub use entry::{LayoutCounts, entry_context, layout_counts};

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
    /// Additional reduction on goods already marked as discounted by the shop.
    pub sale_discount_bonus_per_mille: i32,
    /// Extra Disk choices and guaranteed Disk grants are separate effects.
    pub extra_disk_choices: i32,
    pub extra_disk_drops: i32,
    pub power_limit_bonus: i32,
    pub unsupported: Vec<UnsupportedEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapTrigger {
    Step,
    EnterSafeZone,
    BattleWin,
    EnterBattle,
    /// Pass the bag after acquiring this UID, including warehouse/inlay items.
    Acquire(i64),
    /// Pass the bag before losing this UID. Moving/inlaying is not a loss.
    Lose(i64),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapContext {
    /// Coin balance before this event, especially before acquisition grants.
    pub coin: i32,
    /// Stable per-action seed; repeated evaluation must use the same value.
    pub seed: u64,
    /// Availability/drop-pool snapshot from outside Genius unlock rules.
    /// Production callers pass Some, even for an empty pool. None preserves
    /// standalone rule evaluation and legacy tests; it is not a full unlock.
    pub eligible_items: Option<BTreeSet<i32>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapState {
    /// Persistent per-instance counters; moving an item to the warehouse does
    /// not reset its progress, and stored items do not advance their counters.
    pub counters: BTreeMap<i64, i32>,
    /// These tombstones make acquisition/loss grants idempotent per real UID.
    /// Existing saves deserialize missing fields as empty sets.
    pub acquired_uids: BTreeSet<i64>,
    pub lost_uids: BTreeSet<i64>,
    /// Per-instance permanent stat rates, applied only while that item is active.
    /// A combat adapter may persist verified growth here after battle settlement.
    pub permanent_rates: BTreeMap<i64, BTreeMap<i32, i32>>,
    /// Per-fight cumulative NEW growth already merged, in percentage points.
    /// Fight identity and item UID prevent replay/reconnect double credit.
    pub battle_growth: BTreeMap<i64, BTreeMap<i64, i32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemReplacement {
    pub uid: i64,
    pub item_id: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapOutcome {
    pub coin: i32,
    pub power: i32,
    /// Fraction of maximum life, per-mille. This does not revive Lost heroes.
    pub heal_per_mille: i32,
    pub consume_uids: Vec<i64>,
    /// Newly granted item config IDs. The caller allocates fresh UIDs and
    /// processes Acquire events for them in a bounded, stable order.
    pub grant_items: Vec<i32>,
    /// Forging keeps the instance UID. Preserve valid holes; return excess
    /// inlays to the warehouse and revalidate placement after replacing it.
    pub replace_items: Vec<ItemReplacement>,
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

pub fn is_active(item: &BagItem) -> bool {
    RunBag::is_placed(item) && (0..=3).contains(&item.rotation)
}

fn active(item: &BagItem) -> bool {
    is_active(item)
}

pub fn has_tag(item: &BagItem, tag: i32) -> bool {
    effective_tags(item).contains(&tag)
}

pub fn base_item_ids() -> &'static [i32] {
    geometry::ITEM_IDS
}

pub fn collection_rarity(item_id: i32) -> Option<i32> {
    geometry::get(item_id).map(|meta| meta.rarity)
}

/// Original editor tags for collection/ending criteria, without inlay tags.
pub fn collection_tags(item_id: i32) -> &'static [i32] {
    geometry::get(item_id).map(|meta| meta.tags).unwrap_or(&[])
}

pub fn count_tag_in_range(source: &BagItem, bag: &RunBag, tag: i32) -> i32 {
    bag.items
        .iter()
        .filter(|other| has_tag(other, tag) && within_range(source, other))
        .count() as i32
}

fn occupied_cells(item: &BagItem) -> BTreeSet<(i32, i32)> {
    geometry::get(item.item_id)
        .map(|meta| {
            meta.shape
                .iter()
                .map(|cell| {
                    let (row, col) = rotate(*cell, meta, item.rotation);
                    (row + item.row, col + item.col)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Local interpretation of "below": cells immediately below the rotated
/// occupied shape on screen, rather than every lower row or all adjacent sides.
/// Category tags do not count as a mechanic other than Support. Empty and
/// matching-only areas qualify. This directional convention needs a live
/// comparison for rotated Support artefacts.
pub fn only_support_below(source: &BagItem, bag: &RunBag) -> bool {
    if !active(source) {
        return false;
    }
    let occupied = occupied_cells(source);
    let below: BTreeSet<_> = occupied
        .iter()
        .map(|(row, col)| (row + 1, *col))
        .filter(|cell| !occupied.contains(cell))
        .collect();
    bag.items
        .iter()
        .filter(|other| active(other) && other.uid != source.uid)
        .filter(|other| {
            occupied_cells(other)
                .iter()
                .any(|cell| below.contains(cell))
        })
        .all(|other| {
            effective_tags(other)
                .iter()
                .all(|tag| *tag >= 100 || *tag == 1)
        })
}

/// Layout energy is supplied by surrounding winders; inserted Disks do not
/// independently provide energy. Mechanic 8 is the client's Electric tag.
pub fn layout_energy(target: &BagItem, bag: &RunBag) -> i32 {
    if !active(target) || !has_tag(target, 8) {
        return 0;
    }
    bag.items
        .iter()
        .filter(|source| within_range(source, target))
        .map(|source| match source.item_id {
            8130803 => 1,
            8130804 => 2,
            8320811 => 4,
            _ => 0,
        })
        .sum()
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
    evaluate_battle_with_state(tables, bag, coin, &MapState::default())
}

pub fn evaluate_battle_with_state(
    tables: &config::GameDB,
    bag: &RunBag,
    coin: i32,
    state: &MapState,
) -> BattleModifiers {
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
        if let Some(rates) = state.permanent_rates.get(&item.uid) {
            for (id, rate) in rates {
                add_rate(&mut result, *id, *rate);
            }
        }
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
                // Entry/rank-2-or-3 Taunt uses the Rouge battle runtime.
            }
            8210113 => {
                add_rate(&mut result, 213, 100);
            }
            // Conditional rates are exposed separately, for one skill only.
            8110312 | 8210313 | 8230305 | 8330306 | 8310706 | 8210712 => {}
            8310108 => result.unsupported.push(unsupported(
                item,
                "Aid-cast damage supported; kill-triggered power still needs combat support",
            )),
            8230019 => {
                result.sale_discount_bonus_per_mille += 200;
                // The entry runtime grants two current permanent-set buffs,
                // preferring missing effects. Stable seeded selection is a
                // documented local interpretation of the random draw.
            }
            8310022 => add_rate(
                &mut result,
                205,
                200 + count2 * 10 + count_tag_in_range(item, bag, 103) * 200,
            ),
            8310114 => {
                add_rate(
                    &mut result,
                    213,
                    300 + count2 * 10
                        + if only_support_below(item, bag) {
                            300
                        } else {
                            0
                        },
                );
                result.unsupported.push(unsupported(
                    item,
                    "round-start Reflect and conditional wound/bleed leech need combat support",
                ));
            }
            8310206 => add_rate(
                &mut result,
                102,
                100 + count2 * 10
                    + state
                        .counters
                        .get(&item.uid)
                        .copied()
                        .unwrap_or(0)
                        .max(0)
                        .saturating_mul(30),
            ),
            8310212 => add_rate(&mut result, 211, disks * 40),
            8310405 => {
                add_rate(&mut result, 205, 200 + count2 * 10);
                result.unsupported.push(unsupported(
                    item,
                    "Syntony growth persists; catalyst-specific triggering and level rewrites remain incomplete",
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
                // The runtime tracks actual Support casts and grants the
                // round-end shield. Its neutral carrier 30740121 is a local
                // display inference; no official carrier ID is claimed.
            }
            8220612 => add_rate(
                &mut result,
                206,
                (coin.max(0) / 50 * 100).min(1000) + count2 * 10,
            ),
            8320618 => result.coin_drop_bonus_per_mille += 200,
            8320310 => {
                add_rate(&mut result, 214, count2 * 10);
                // The battle hook applies shared permanent growth, with
                // independent once-per-actor rank-2/rank-3 limits. Saved rates
                // are applied above; count2 here is static inlay data only.
            }
            8330001 => result.shop_price_reduction_per_mille += 100,
            8330002 => {
                result.extra_disk_choices += 1;
                result.extra_disk_drops += 1;
            }
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
                // HeroEx/Sp has no field for this attribute; the server-only
                // Rouge battle context now applies it to the Genesis formula.
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
                    "Syntony growth persists; catalyst-specific triggering and level rewrites remain incomplete",
                ));
            }
            8330607 => {
                result.shop_price_reduction_per_mille += 150;
                // Five current permanent-set buffs use the same entry hook.
            }
            8230024 | 8130605 | 8140101 | 8240102 | 8140601 | 8240602 | 8230606 | 8330708
            | 8330817 | 8320506 | 8310309 | 8220613 | 8110213 | 8110307 | 8210308 | 8120034
            | 8120104 | 8130041 | 8130103 | 8130203 | 8130217 | 8130303 | 8230304 | 8130803
            | 8130804 | 8230027 | 8230028 | 8330026 | 8120406 | 8130403 | 8130404 | 8120821
            | 8210314 | 8310315 => {}
            8330615 => result.unsupported.push(unsupported(
                item,
                "acquisition coins supported; storm region substitution needs combat support",
            )),
            8210711 => result.unsupported.push(unsupported(
                item,
                "Bullet damage supported; the extra-action chance still needs combat support",
            )),
            8330710 => result.unsupported.push(unsupported(
                item,
                "round-start Schemed supported; storm region replacement is pending",
            )),
            // The combat runtime grants the verified anticipation states and
            // generates own/support cards at their next real trigger.
            8210045 => {}
            _ if battle::rouge::artefact_entry::has_entry_effect(item.item_id)
                || battle::rouge::artefact_entry::medication_skill_id(item.item_id).is_some() => {}
            _ => result.unsupported.push(unsupported(
                item,
                "scripted artefact effect not yet implemented",
            )),
        }
    }
    // Stacking discounts use additive percentage points, capped at a free item.
    // This is an explicit local policy until a multi-discount sample is recorded.
    result.shop_price_reduction_per_mille = result.shop_price_reduction_per_mille.clamp(0, 1000);
    result.sale_discount_bonus_per_mille = result.sale_discount_bonus_per_mille.clamp(0, 1000);
    result
}

/// Only six base forging pairs use different config IDs. DLC 101's two
/// same-image state variants are deliberately outside this map.
pub fn forged_item_id(item_id: i32) -> Option<i32> {
    Some(match item_id {
        8110307 => 8210308,
        8110312 => 8210313,
        8130303 => 8230304,
        8210314 => 8310315,
        8220318 => 8320320,
        8230305 => 8330306,
        _ => return None,
    })
}

/// Reconstruct an owned inlay by its preserved instance UID for lifecycle
/// events. Inlay transfers must never be mistaken for Acquire/Lose.
fn owned_item(bag: &RunBag, uid: i64) -> Option<BagItem> {
    if let Some(item) = bag.item(uid) {
        return Some(item.clone());
    }
    for host in &bag.items {
        for (index, id) in host.holes.iter().enumerate() {
            if *id > 0 && host.hole_ids.get(index).copied() == Some(uid) {
                return Some(BagItem {
                    uid,
                    item_id: *id,
                    row: -1,
                    col: -1,
                    rotation: 0,
                    holes: Vec::new(),
                    hole_ids: Vec::new(),
                });
            }
        }
    }
    None
}

fn next_random(seed: &mut u64) -> u64 {
    // SplitMix64 gives stable action-local choices, without global RNG state.
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut value = *seed;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn random_reward(
    tables: &config::GameDB,
    bag: &RunBag,
    grants: &[i32],
    tag: i32,
    eligible: Option<&BTreeSet<i32>>,
    seed: &mut u64,
) -> Option<i32> {
    let owned: BTreeSet<_> = bag
        .items
        .iter()
        .flat_map(|item| std::iter::once(item.item_id).chain(item.holes.iter().copied()))
        .chain(grants.iter().copied())
        .collect();
    let candidates: BTreeSet<_> = tables
        .rouge_collection_unlock
        .iter()
        .map(|row| row.id)
        .filter(|id| eligible.is_none_or(|pool| pool.contains(id)))
        .filter(|id| {
            geometry::get(*id)
                .is_some_and(|meta| (1..=3).contains(&meta.rarity) && meta.tags.contains(&tag))
        })
        // Acquisition factories cannot recursively produce another factory.
        // Uniform selection within this base-only pool is a local inference;
        // the source describes preference, not the server's exact weights.
        .filter(|id| !matches!(*id, 8330708 | 8330817))
        .filter(|id| {
            !owned.contains(id)
                || !matches!(
                    *id,
                    8210209 | 8310206 | 8310405 | 8310608 | 8320207 | 8320310 | 8320617 | 8330418
                )
        })
        .collect();
    let preferred: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|id| !owned.contains(id))
        .collect();
    let pool: Vec<_> = if preferred.is_empty() {
        candidates.into_iter().collect()
    } else {
        preferred
    };
    if pool.is_empty() {
        None
    } else {
        Some(pool[(next_random(seed) % pool.len() as u64) as usize])
    }
}

/// Computes event deltas without mutating the run. Callers commit the counter,
/// resource deltas, item consumption, and node transition in one transaction.
pub fn evaluate_map(
    tables: &config::GameDB,
    bag: &RunBag,
    trigger: MapTrigger,
    state: &MapState,
) -> MapOutcome {
    evaluate_map_with_context(tables, bag, trigger, state, &MapContext::default())
}

pub fn evaluate_map_with_context(
    tables: &config::GameDB,
    bag: &RunBag,
    trigger: MapTrigger,
    state: &MapState,
    context: &MapContext,
) -> MapOutcome {
    let mut result = MapOutcome {
        state: state.clone(),
        ..Default::default()
    };
    if let MapTrigger::Acquire(uid) | MapTrigger::Lose(uid) = trigger {
        let Some(item) = owned_item(bag, uid) else {
            return result;
        };
        let acquire = matches!(trigger, MapTrigger::Acquire(_));
        let seen = if acquire {
            &mut result.state.acquired_uids
        } else {
            &mut result.state.lost_uids
        };
        if !seen.insert(uid) || geometry::get(item.item_id).is_none() {
            return result;
        }
        let mut seed = context.seed ^ (uid as u64).rotate_left(17);
        match (acquire, item.item_id) {
            (_, 8230606) => result.coin += 50,
            (true, 8330615) => result.coin += if context.coin < 200 { 200 } else { 100 },
            (true, 8330708) => {
                for _ in 0..2 {
                    if let Some(id) = random_reward(
                        tables,
                        bag,
                        &result.grant_items,
                        7,
                        context.eligible_items.as_ref(),
                        &mut seed,
                    ) {
                        result.grant_items.push(id);
                    }
                }
            }
            (true, 8330817) => {
                result.grant_items.push(8130804);
                if let Some(id) = random_reward(
                    tables,
                    bag,
                    &result.grant_items,
                    8,
                    context.eligible_items.as_ref(),
                    &mut seed,
                ) {
                    result.grant_items.push(id);
                }
            }
            _ => {}
        }
        return result;
    }
    // Stable UID order is a local policy where several consumptions or
    // transformations overlap. Each target can be consumed/forged only once.
    // A swallowed target cannot activate later during the same safe-zone event.
    let mut items: Vec<_> = bag.items.iter().filter(|item| active(item)).collect();
    items.sort_unstable_by_key(|item| item.uid);
    let mut replaced = BTreeSet::new();
    for item in &items {
        if result.consume_uids.contains(&item.uid) {
            continue;
        }
        let mut seed = context.seed ^ (item.uid as u64).rotate_left(17);
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
            (MapTrigger::EnterSafeZone, 8310206) => {
                let mut eaten = 0_i32;
                for target in &items {
                    if !result.consume_uids.contains(&target.uid)
                        && !replaced.contains(&target.uid)
                        && geometry::get(target.item_id).is_some_and(|meta| meta.rarity == 1)
                        && within_range(item, target)
                    {
                        result.consume_uids.push(target.uid);
                        eaten += 1;
                    }
                }
                let counter = result.state.counters.entry(item.uid).or_default();
                *counter = (*counter).max(0).saturating_add(eaten);
            }
            (MapTrigger::EnterSafeZone, 8310309) => {
                for target in &items {
                    if !result.consume_uids.contains(&target.uid)
                        && !replaced.contains(&target.uid)
                        && within_range(item, target)
                        && let Some(item_id) = forged_item_id(target.item_id)
                    {
                        result.replace_items.push(ItemReplacement {
                            uid: target.uid,
                            item_id,
                        });
                        replaced.insert(target.uid);
                    }
                }
            }
            (MapTrigger::EnterBattle, 8140101) => result.power += 2,
            (MapTrigger::EnterBattle, 8240102) => result.power += 4,
            (MapTrigger::EnterBattle, 8140601) => {
                result.coin += 2 + (next_random(&mut seed) % 5) as i32
            }
            (MapTrigger::EnterBattle, 8240602) => {
                result.coin += 4 + (next_random(&mut seed) % 9) as i32
            }
            (MapTrigger::EnterBattle, 8320506) => {
                let enhancement = inlay_counters(tables, item)
                    .get("count2")
                    .copied()
                    .unwrap_or(0);
                let resonance = items.iter().filter(|other| has_tag(other, 5)).count() as i32;
                result.power += 5 + enhancement + resonance;
            }
            (MapTrigger::BattleWin, 8330002) => {
                if let Some(id) = random_reward(
                    tables,
                    bag,
                    &result.grant_items,
                    105,
                    context.eligible_items.as_ref(),
                    &mut seed,
                ) {
                    result.grant_items.push(id);
                }
            }
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
    fn genesis_bonus_is_computed_while_random_round_statuses_remain_pending() {
        let result = evaluate_battle(tables(), &bag(vec![item(1, 8330020, 0, 0)]), 0);
        assert_eq!(result.attribute_rates.get(&217), Some(&1000));
        assert_eq!(
            result.unsupported,
            vec![UnsupportedEffect {
                uid: 1,
                item_id: 8330020,
                reason: "random negative statuses at round start need a combat trigger",
            }]
        );
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
        assert!(result.unsupported.is_empty());
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
            &bag(vec![item(1, 8320062, 0, 0), item(2, 8120035, 3, 0)]),
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

    #[test]
    fn lifecycle_coin_grants_use_real_ownership_and_are_idempotent_after_reload() {
        let layout = bag(vec![item(10, 8230606, -1, -1), item(20, 8330615, -1, -1)]);
        let context = MapContext {
            coin: 199,
            seed: 7,
            ..Default::default()
        };
        let initial = MapState::default();
        let cup = evaluate_map_with_context(
            tables(),
            &layout,
            MapTrigger::Acquire(10),
            &initial,
            &context,
        );
        assert_eq!(cup.coin, 50, "acquisition is active even in the warehouse");
        let encoded = serde_json::to_string(&cup.state).unwrap();
        let state: MapState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            evaluate_map_with_context(tables(), &layout, MapTrigger::Acquire(10), &state, &context)
                .coin,
            0
        );
        let lost =
            evaluate_map_with_context(tables(), &layout, MapTrigger::Lose(10), &state, &context);
        assert_eq!(lost.coin, 50);
        assert_eq!(
            evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::Lose(10),
                &lost.state,
                &context
            )
            .coin,
            0
        );
        assert_eq!(
            evaluate_map_with_context(tables(), &layout, MapTrigger::Acquire(20), &state, &context)
                .coin,
            200
        );
        let threshold = MapContext {
            coin: 200,
            ..context.clone()
        };
        assert_eq!(
            evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::Acquire(20),
                &initial,
                &threshold
            )
            .coin,
            100
        );
        assert_eq!(
            evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::Acquire(999),
                &initial,
                &context
            )
            .coin,
            0
        );
        let old: MapState = serde_json::from_str(r#"{"counters":{"1":20}}"#).unwrap();
        assert_eq!(old.counters.get(&1), Some(&20));
        assert!(old.acquired_uids.is_empty() && old.permanent_rates.is_empty());
    }

    #[test]
    fn acquisition_products_are_base_only_prefer_unowned_and_do_not_recurse() {
        let layout = bag(vec![
            item(1, 8330708, -1, -1),
            item(2, 8330817, -1, -1),
            item(3, 8130804, -1, -1),
        ]);
        let context = MapContext {
            coin: 0,
            seed: 91,
            ..Default::default()
        };
        let rosetta = evaluate_map_with_context(
            tables(),
            &layout,
            MapTrigger::Acquire(1),
            &MapState::default(),
            &context,
        );
        assert_eq!(rosetta.grant_items.len(), 2);
        assert_ne!(
            rosetta.grant_items[0], rosetta.grant_items[1],
            "prefer a different not-yet-owned result"
        );
        for id in &rosetta.grant_items {
            assert!(geometry::get(*id).unwrap().tags.contains(&7));
            assert!(!matches!(*id, 8330708 | 8330817));
        }
        assert_eq!(
            rosetta,
            evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::Acquire(1),
                &MapState::default(),
                &context
            )
        );
        let bow = evaluate_map_with_context(
            tables(),
            &layout,
            MapTrigger::Acquire(2),
            &rosetta.state,
            &context,
        );
        assert_eq!(bow.grant_items.len(), 2);
        assert_eq!(bow.grant_items[0], 8130804);
        assert_ne!(
            bow.grant_items[1], 8130804,
            "the random reward prefers not-yet-owned items"
        );
        assert!(geometry::get(bow.grant_items[1]).unwrap().tags.contains(&8));
        assert!(
            evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::Acquire(2),
                &bow.state,
                &context
            )
            .grant_items
            .is_empty()
        );
    }

    #[test]
    fn safe_zone_cane_eats_common_items_once_and_growth_requires_the_cane_active() {
        let layout = bag(vec![
            item(1, 8310206, 2, 2),
            item(2, 8140101, 2, 1),
            item(3, 8140201, 2, 3),
            item(4, 8140301, 2, 4),
            item(5, 8240102, 3, 1),
            item(6, 8140401, -1, -1),
        ]);
        let effect = evaluate_map(
            tables(),
            &layout,
            MapTrigger::EnterSafeZone,
            &MapState::default(),
        );
        assert_eq!(effect.consume_uids, vec![2, 3]);
        assert_eq!(effect.state.counters.get(&1), Some(&2));
        assert_eq!(
            evaluate_battle_with_state(tables(), &layout, 0, &effect.state)
                .attribute_rates
                .get(&102),
            Some(&160)
        );
        let mut committed = layout.clone();
        committed
            .items
            .retain(|item| !effect.consume_uids.contains(&item.uid));
        let second = evaluate_map(
            tables(),
            &committed,
            MapTrigger::EnterSafeZone,
            &effect.state,
        );
        assert!(second.consume_uids.is_empty());
        assert_eq!(second.state.counters.get(&1), Some(&2));
        committed.items[0].row = -1;
        committed.items[0].col = -1;
        assert!(
            !evaluate_battle_with_state(tables(), &committed, 0, &second.state)
                .attribute_rates
                .contains_key(&102)
        );
    }

    #[test]
    fn safe_zone_forges_explicit_pairs_once_and_preserves_uid() {
        let layout = bag(vec![
            item(1, 8310309, 2, 2),
            item(2, 8110312, 1, 2),
            item(3, 8110321, 1, 3),
        ]);
        let effect = evaluate_map(
            tables(),
            &layout,
            MapTrigger::EnterSafeZone,
            &MapState::default(),
        );
        assert_eq!(
            effect.replace_items,
            vec![ItemReplacement {
                uid: 2,
                item_id: 8210313
            }]
        );
        assert!(effect.consume_uids.is_empty());
        assert_eq!(
            forged_item_id(8110321),
            None,
            "DLC state variants are not base forging pairs"
        );
        let mut committed = layout;
        committed.items[1].item_id = 8210313;
        assert!(
            evaluate_map(
                tables(),
                &committed,
                MapTrigger::EnterSafeZone,
                &effect.state
            )
            .replace_items
            .is_empty()
        );
    }

    #[test]
    fn competing_canes_cannot_consume_the_same_item_twice() {
        let layout = bag(vec![
            item(20, 8310206, 2, 3),
            item(10, 8310206, 2, 1),
            item(5, 8140101, 3, 2),
        ]);
        let effect = evaluate_map(
            tables(),
            &layout,
            MapTrigger::EnterSafeZone,
            &MapState::default(),
        );
        assert_eq!(effect.consume_uids, vec![5]);
        assert_eq!(effect.state.counters.get(&10), Some(&1));
        assert_eq!(effect.state.counters.get(&20), Some(&0));
    }

    #[test]
    fn crown_counts_effective_resonance_and_placed_coin_disks_roll_in_bounds() {
        let mut host = item(2, 8310405, 4, 4);
        host.holes = vec![8140501];
        host.hole_ids = vec![12];
        let layout = bag(vec![
            item(1, 8320506, 0, 0),
            host,
            item(3, 8140601, 0, 4),
            item(4, 8240602, 0, 6),
            item(5, 8320506, -1, -1),
        ]);
        for seed in 0..32 {
            let effect = evaluate_map_with_context(
                tables(),
                &layout,
                MapTrigger::EnterBattle,
                &MapState::default(),
                &MapContext {
                    coin: 0,
                    seed,
                    ..Default::default()
                },
            );
            assert_eq!(
                effect.power, 7,
                "Crown itself plus one holder have Resonance"
            );
            assert!((6..=18).contains(&effect.coin));
        }
        assert_eq!(
            evaluate_map(tables(), &layout, MapTrigger::Step, &MapState::default()).coin,
            0
        );
    }

    #[test]
    fn notebook_separates_an_extra_disk_choice_from_a_guaranteed_disk_drop() {
        let layout = bag(vec![item(1, 8330002, 0, 0), item(2, 8330002, -1, -1)]);
        let modifiers = evaluate_battle(tables(), &layout, 0);
        assert_eq!(
            (modifiers.extra_disk_choices, modifiers.extra_disk_drops),
            (1, 1)
        );
        let win = evaluate_map(
            tables(),
            &layout,
            MapTrigger::BattleWin,
            &MapState::default(),
        );
        assert_eq!(win.grant_items.len(), 1);
        assert_eq!(geometry::get(win.grant_items[0]).unwrap().category, 105);
        assert!(
            evaluate_map(
                tables(),
                &layout,
                MapTrigger::EnterBattle,
                &MapState::default()
            )
            .grant_items
            .is_empty()
        );
    }

    #[test]
    fn real_random_grants_obey_genius_401_pool_and_unlocked_items_become_drawable() {
        let tables = tables();
        let locked: BTreeSet<_> = crate::rouge::favorites::unlock_collection_ids(tables, &[])
            .into_iter()
            .collect();
        let unlocked: BTreeSet<_> = crate::rouge::favorites::unlock_collection_ids(tables, &[401])
            .into_iter()
            .collect();
        let gated_disks = BTreeSet::from([8140401, 8140501, 8240402, 8240502]);
        let notebook = bag(vec![item(1, 8330002, 0, 0)]);
        let rosetta = bag(vec![item(1, 8330708, -1, -1)]);
        let mut drawn_after_unlock = BTreeSet::new();
        let mut saw_locked_extra_action_after_unlock = false;
        for seed in 0..256 {
            for (is_unlocked, pool) in [(false, &locked), (true, &unlocked)] {
                let context = MapContext {
                    coin: 0,
                    seed,
                    eligible_items: Some(pool.clone()),
                };
                let disk = evaluate_map_with_context(
                    tables,
                    &notebook,
                    MapTrigger::BattleWin,
                    &MapState::default(),
                    &context,
                );
                assert_eq!(disk.grant_items.len(), 1);
                assert!(disk.grant_items.iter().all(|id| pool.contains(id)));
                let gifts = evaluate_map_with_context(
                    tables,
                    &rosetta,
                    MapTrigger::Acquire(1),
                    &MapState::default(),
                    &context,
                );
                assert_eq!(gifts.grant_items.len(), 2);
                assert!(gifts.grant_items.iter().all(|id| pool.contains(id)));
                if is_unlocked {
                    drawn_after_unlock.extend(disk.grant_items);
                    saw_locked_extra_action_after_unlock |= gifts.grant_items.contains(&8320709);
                } else {
                    assert!(disk.grant_items.iter().all(|id| !gated_disks.contains(id)));
                    assert!(!gifts.grant_items.contains(&8320709));
                }
            }
        }
        assert!(
            gated_disks.is_subset(&drawn_after_unlock),
            "all four gated Disks enter the actual reward pool after 401"
        );
        assert!(
            saw_locked_extra_action_after_unlock,
            "the gated Rosetta result enters the actual gift pool after 401"
        );
    }

    #[test]
    fn empty_random_pool_does_not_block_fixed_seven_day_winder_gift() {
        let context = MapContext {
            eligible_items: Some(BTreeSet::new()),
            ..Default::default()
        };
        let layout = bag(vec![item(1, 8330817, -1, -1)]);
        let gift = evaluate_map_with_context(
            tables(),
            &layout,
            MapTrigger::Acquire(1),
            &MapState::default(),
            &context,
        );
        assert_eq!(gift.grant_items, vec![8130804]);
        let notebook = bag(vec![item(2, 8330002, 0, 0)]);
        assert!(
            evaluate_map_with_context(
                tables(),
                &notebook,
                MapTrigger::BattleWin,
                &MapState::default(),
                &context
            )
            .grant_items
            .is_empty()
        );
    }

    #[test]
    fn support_below_uses_adjacent_lower_cells_and_ignores_category_tags() {
        let source = item(1, 8210113, 3, 3);
        assert!(
            only_support_below(&source, &bag(vec![source.clone(), item(2, 8140401, 2, 3)])),
            "a nonsupport above does not block below"
        );
        assert!(!only_support_below(
            &source,
            &bag(vec![source.clone(), item(2, 8140401, 4, 3)])
        ));
        assert!(only_support_below(
            &source,
            &bag(vec![source.clone(), item(2, 8140101, 4, 3)])
        ));
        assert!(
            only_support_below(&source, &bag(vec![source.clone(), item(2, 8140401, 4, 2)])),
            "diagonal cells are not directly below"
        );
        let mut rotated = source;
        rotated.rotation = 1;
        assert!(only_support_below(
            &rotated,
            &bag(vec![rotated.clone(), item(2, 8140401, 4, 4)])
        ));
        assert!(!only_support_below(
            &rotated,
            &bag(vec![rotated.clone(), item(2, 8140401, 5, 3)])
        ));
    }

    #[test]
    fn conditional_rates_never_leak_into_permanent_stats_or_wrong_skill_types() {
        let mut tooth = item(1, 8110312, 0, 0);
        tooth.holes = vec![8240302];
        tooth.hole_ids = vec![21];
        let layout = bag(vec![
            tooth,
            item(2, 8330306, 0, 4),
            item(3, 8310108, 2, 4),
            item(4, 8210113, 5, 0),
        ]);
        let static_rates = evaluate_battle(tables(), &layout, 0).attribute_rates;
        assert_eq!(static_rates.get(&213), Some(&100));
        assert!(!static_rates.contains_key(&203) && !static_rates.contains_key(&205));
        assert!(
            evaluate_attack(
                tables(),
                &layout,
                &AttackContext {
                    rank: 1,
                    ..Default::default()
                }
            )
            .is_empty()
        );
        let high = evaluate_attack(
            tables(),
            &layout,
            &AttackContext {
                rank: 2,
                ..Default::default()
            },
        );
        assert_eq!(high, BTreeMap::from([(203, 600), (205, 300)]));
        assert!(
            evaluate_attack(
                tables(),
                &layout,
                &AttackContext {
                    rank: 4,
                    is_ultimate: true,
                    ..Default::default()
                }
            )
            .is_empty()
        );
        let aid = evaluate_attack(
            tables(),
            &layout,
            &AttackContext {
                rank: 1,
                is_support: true,
                ..Default::default()
            },
        );
        assert_eq!(aid, BTreeMap::from([(205, 300), (213, 200)]));
        assert!(
            attack_modifiers(tables(), &layout)
                .iter()
                .all(|m| m.source_uid > 0 && m.source_uid < 10)
        );
    }

    #[test]
    fn permanent_growth_and_sale_only_discounts_keep_their_scopes() {
        let layout = bag(vec![
            item(1, 8230019, 0, 0),
            item(2, 8310022, 3, 0),
            item(3, 8110021, 2, 0),
        ]);
        let mut state = MapState::default();
        state.permanent_rates.insert(2, BTreeMap::from([(205, 40)]));
        let effect = evaluate_battle_with_state(tables(), &layout, 0, &state);
        assert_eq!(effect.attribute_rates.get(&205), Some(&440));
        assert_eq!(effect.sale_discount_bonus_per_mille, 200);
        assert_eq!(
            effect.shop_price_reduction_per_mille, 0,
            "sale enhancement is not a shop-wide discount"
        );
    }

    #[test]
    fn winders_provide_range_energy_only_to_active_electric_holders() {
        let source = item(1, 8130803, 3, 3);
        let target = item(2, 8140801, 3, 4);
        let mut stored = item(3, 8130804, -1, -1);
        stored.holes = vec![8140801];
        let layout = bag(vec![source, target.clone(), stored]);
        assert_eq!(layout_energy(&target, &layout), 1);
        assert_eq!(layout_energy(&item(4, 8140101, 3, 4), &layout), 0);
        assert_eq!(layout_energy(&item(5, 8140801, -1, -1), &layout), 0);
    }
}
