//! Frozen layout facts for entry scripts and catalyst passives.
//!
//! Medication uses the current 8xxxxxx skills, never the similarly named
//! legacy 9xxxxxx artefact skills. Inserted Disks enhance their holder; they
//! do not independently produce entry cards or entry states.

use super::super::bag::RunBag;
use std::collections::BTreeMap;

use super::{collection_rarity, count_tag_in_range, effective_tags, inlay_counters, is_active};
use battle::rouge::artefact_entry::{EntryContext, MedicationCard, medication_skill_id};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutCounts {
    /// Physically placed base objects, including physically placed Disks.
    pub physical_count: i32,
    /// Physical Disks and Disks inside active holders, each counted once.
    /// This shares the existing Sword/Decanter policy; a live layout sample
    /// is still needed to establish every catalyst's treatment of inlays.
    pub disk_count: i32,
    /// Client rarity exactly 3 (妙想). The rarity-4 Chalice is not guessed 3.
    pub epic_count: i32,
    pub rare_or_better_count: i32,
    /// Effective mechanic tags, once per physical holder. An inserted Disk
    /// can add a tag but cannot make the same holder count twice for it.
    pub tag_counts: BTreeMap<i32, i32>,
}

pub fn layout_counts(bag: &RunBag) -> LayoutCounts {
    let mut result = LayoutCounts::default();
    for item in bag.items.iter().filter(|item| is_active(item)) {
        let Some(rarity) = collection_rarity(item.item_id) else {
            continue;
        };
        result.physical_count += 1;
        result.epic_count += i32::from(rarity == 3);
        result.rare_or_better_count += i32::from(rarity >= 2);
        result.disk_count += std::iter::once(item.item_id)
            .chain(item.holes.iter().copied())
            .filter(|id| super::super::collection_shapes::is_enchant_item(*id))
            .count() as i32;
        for tag in effective_tags(item).into_iter().filter(|tag| *tag < 100) {
            *result.tag_counts.entry(tag).or_default() += 1;
        }
    }
    result
}

/// Capture this once with the Fight. Reconnect must restore this snapshot,
/// rather than regenerate cards from a bag changed after battle started.
pub fn entry_context(tables: &config::GameDB, bag: &RunBag, coin: i32, seed: u64) -> EntryContext {
    let mut medication_cards = Vec::new();
    for item in bag.items.iter().filter(|item| is_active(item)) {
        let Some(skill_id) = medication_skill_id(item.item_id) else {
            continue;
        };
        let count2 = inlay_counters(tables, item)
            .get("count2")
            .copied()
            .unwrap_or_default();
        // The Wiki and current rouge_desc explicitly grant one extra card
        // when any Medication is within the rotated effect area (not one
        // card for every nearby Medication).
        // within_range excludes self, stored objects and invalid rotations.
        let count = 1_i32
            .saturating_add(count2)
            .saturating_add(i32::from(count_tag_in_range(item, bag, 104) > 0))
            .max(0);
        medication_cards.push(MedicationCard {
            source_uid: item.uid,
            item_id: item.item_id,
            skill_id,
            count,
        });
    }
    medication_cards.sort_by_key(|card| card.source_uid);
    EntryContext {
        coin: coin.max(0),
        seed,
        medication_cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rouge::bag::BagItem;

    fn item(uid: i64, item_id: i32, row: i32, col: i32) -> BagItem {
        BagItem {
            uid,
            item_id,
            row,
            col,
            rotation: 0,
            holes: Vec::new(),
            hole_ids: Vec::new(),
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
    fn catalyst_counts_inlays_once_and_preserves_client_chalice_rarity() {
        let mut horn = item(1, 8310405, 0, 0);
        horn.holes = vec![8140401, 8140401, 8240202];
        let mut stored = item(2, 8310405, -1, -1);
        stored.holes = vec![8240402];
        let counts = layout_counts(&bag(vec![
            horn,
            stored,
            item(3, 8140501, 3, 0),
            item(4, 8320025, 4, 0),
            item(5, 8230019, 5, 0),
        ]));
        assert_eq!(counts.physical_count, 4);
        assert_eq!(counts.disk_count, 4);
        assert_eq!(counts.epic_count, 1);
        assert_eq!(counts.tag_counts.get(&4), Some(&1));
        assert_eq!(counts.tag_counts.get(&2), Some(&1));
    }

    #[test]
    fn medications_use_exact_current_skills_and_only_active_physical_sources() {
        super::super::super::original_tests::init_config();
        let tables = config::configs::get();
        let mut holder = item(20, 8310405, 0, 0);
        holder.holes = vec![8150006]; // malformed inlay cannot become a source.
        let frozen = entry_context(
            tables,
            &bag(vec![
                item(10, 8150006, 1, 1),
                item(11, 8250017, -1, -1),
                holder,
                item(12, 8150008, 6, 6),
            ]),
            70,
            42,
        );
        assert_eq!(frozen.medication_cards.len(), 2);
        assert_eq!(frozen.medication_cards[0].skill_id, 815000611);
        assert_eq!(frozen.medication_cards[0].count, 1);
        assert_eq!(frozen.medication_cards[1].skill_id, 815000811);
        assert_eq!(frozen.coin, 70);
    }

    #[test]
    fn medication_range_gives_an_extra_card_only_when_the_other_shape_intersects() {
        super::super::super::original_tests::init_config();
        let tables = config::configs::get();
        let source = item(1, 8150006, 3, 3);
        // Find an actual editor range cell, rather than assuming a rectangle.
        let neighbor = (0..8)
            .flat_map(|row| (0..8).map(move |col| (row, col)))
            .map(|(row, col)| item(2, 8150008, row, col))
            .find(|target| super::super::within_range(&source, target))
            .expect("Medication has a configured effect area");
        let mut layout = bag(vec![source, neighbor]);
        let frozen = entry_context(tables, &layout, 0, 1);
        assert_eq!(frozen.medication_cards[0].count, 2);
        // The text says "when a Medication is within range", not "per
        // Medication". A second qualifying neighbor cannot add another card.
        let mut second = layout.items[1].clone();
        second.uid = 3;
        layout.items.push(second);
        assert_eq!(
            entry_context(tables, &layout, 0, 1).medication_cards[0].count,
            2
        );
        layout.items.pop();
        layout.items[1].row = -1;
        layout.items[1].col = -1;
        let frozen = entry_context(tables, &layout, 0, 1);
        assert_eq!(frozen.medication_cards.len(), 1);
        assert_eq!(frozen.medication_cards[0].count, 1);
    }
}
