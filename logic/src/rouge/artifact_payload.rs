//! Display values used by RougeCollectionExpressionHelper, separate from combat.
//!
//! `rouge_attr` maps count1..count9 to 1001..1009. The client uses server
//! attributes for placed items, but adds static Disk attributes itself for
//! stored items. SlotMO also replaces item.attr with layout.attr on creation,
//! so both fields must carry the same placed-item values.
//!
//! Only static Disk enhancements, implemented persistent counters/growth, and
//! formulas whose variables are explicit in rouge_desc are supplied here.
//! An unrelated map counter must never become count2; permanent battle growth
//! comes from its separately typed per-instance attribute rate.

use super::bag::{BagItem, RunBag};
use database::models::game::rouge::RougeRun;
use sonettobuf::{RougeBag, RougeItem, RougeItemAttr, RougeItemLayout};
use std::collections::BTreeMap;

// Share the extracted client metadata without changing the rules module's
// visibility. This module only reads category/rarity; shape fields stay unused.
#[allow(dead_code)]
#[path = "artifact_metadata.rs"]
mod metadata;

/// The current placed bag, including the attributes consumed by SlotMO.
pub fn bag_payload(run: &RougeRun, bag: &RunBag) -> RougeBag {
    let mut result = bag.to_client();
    decorate_layouts(run, bag, &mut result.layouts);
    result
}

/// One item for warehouse/item-update messages. Use the persisted final run:
/// its bag supplies the current layout for context-dependent expressions.
/// Stored holders omit Disk enhancements because the client adds those itself.
pub fn item_payload(run: &RougeRun, item: &BagItem) -> RougeItem {
    let bag = serde_json::from_str::<RunBag>(&run.bag).ok();
    let mut result = RunBag::to_client_item(item);
    merge_attributes(&mut result.attr, values(run, bag.as_ref(), item));
    result
}

/// Decorate layout replies built by placement/inlay operations without losing
/// their positions, relations, effects, or attributes from other producers.
pub fn decorate_layouts(run: &RougeRun, bag: &RunBag, layouts: &mut [RougeItemLayout]) {
    for layout in layouts {
        let Some(uid) = layout.item.as_ref().and_then(|item| item.id) else {
            continue;
        };
        let Some(item) = bag.items.iter().find(|item| item.uid == uid) else {
            continue;
        };
        decorate_effects(bag, item, &mut layout.base_effects);
        let attributes = values(run, Some(bag), item);
        // Merge both copies, then mirror the complete union: SlotMO reads the
        // layout field last, while ordinary item updates read the item field.
        let mut combined = layout.item.as_ref().and_then(|item| item.attr.clone());
        if let Some(old) = layout.attr.take() {
            merge_attributes(&mut combined, attr_map(&old));
        }
        if let Some(client_item) = &mut layout.item {
            merge_attributes(&mut combined, attributes);
            client_item.attr = combined.clone();
        }
        layout.attr = combined;
    }
}

fn active(item: &BagItem) -> bool {
    RunBag::is_placed(item) && (0..=3).contains(&item.rotation)
}

fn decorate_effects(bag: &RunBag, item: &BagItem, effects: &mut Vec<i32>) {
    use battle::rouge::artefact_entry::{has_entry_effect, medication_skill_id};
    let medication = medication_skill_id(item.item_id).is_some();
    if !medication && !has_entry_effect(item.item_id) {
        return;
    }
    // These effect indices come directly from current rouge_desc. Preserve
    // unrelated server effects, including hidden flags observed in captures.
    effects.retain(|effect| *effect != 0 && (!medication || *effect != 1));
    if active(item) {
        effects.push(0);
        if medication && super::artifact_rules::count_tag_in_range(item, bag, 104) > 0 {
            effects.push(1);
        }
    }
    effects.sort_unstable();
    effects.dedup();
}

fn values(run: &RougeRun, bag: Option<&RunBag>, item: &BagItem) -> BTreeMap<i32, i64> {
    let tables = config::configs::get();
    let mut result = BTreeMap::new();
    if active(item) {
        // Placement/inlay replies may contain attributes from the old layout.
        // Reset only flags explicitly owned by this item's inlay config, so
        // removing the final Disk cannot leave the old enhancement visible.
        // Unknown server attributes remain intact during the merge below.
        if let Some(row) = tables.rouge_item_static_desc.get(item.item_id) {
            for spec in [&row.item1_attr, &row.item2_attr, &row.item3_attr] {
                for attribute in spec.split('|') {
                    if let Some((flag, _)) = attribute.split_once('#')
                        && let Some(attr) = tables.rouge_attr.iter().find(|row| row.flag == flag)
                    {
                        result.insert(attr.id, 0);
                    }
                }
            }
        }
        for (flag, amount) in super::artifact_rules::inlay_counters(tables, item) {
            if let Some(attr) = tables.rouge_attr.iter().find(|row| row.flag == flag) {
                result.insert(attr.id, i64::from(amount));
            }
        }
    }

    // The pouch description explicitly reads count2 as accumulated coins.
    // Its progress survives storage, so it is also safe to send in warehouse.
    if item.item_id == 8130605 {
        let progress = super::flow::details(run)
            .artifact_state
            .counters
            .get(&item.uid)
            .copied()
            .unwrap_or(0)
            .clamp(0, 100);
        result.insert(1002, i64::from(progress));
    }
    if item.item_id == 8310206 {
        // The persistent counter stores swallowed objects; count2 in the
        // description is the resulting percentage-point attack enhancement.
        let eaten = super::flow::details(run)
            .artifact_state
            .counters
            .get(&item.uid)
            .copied()
            .unwrap_or_default()
            .max(0);
        *result.entry(1002).or_default() += i64::from(eaten).saturating_mul(3);
    }
    if matches!(item.item_id, 8310405 | 8330418 | 8320310) {
        let permanent = super::artifact_growth::permanent_count2(
            item,
            &super::flow::details(run).artifact_state,
        );
        if permanent > 0 {
            *result.entry(1002).or_default() += i64::from(permanent);
        }
    }
    if !active(item) {
        return result;
    }

    // Only this expression displays the held-coin reduction as count2.
    // Other items mentioning coins may use count2 for a Disk enhancement;
    // their flags are not reassigned by matching prose or effect names.
    if item.item_id == 8220612 {
        let rate_percent = (run.coin.max(0) / 50).min(10) * 10;
        *result.entry(1002).or_default() += i64::from(rate_percent);
    }

    let Some(bag) = bag else { return result };
    match item.item_id {
        8310212 | 8330218 => {
            // rouge_desc uses {count2*1}% for the total, so count2 is the
            // percentage bonus (4/2 per Disk), not the raw number of Disks.
            // Keep the evaluator's explicit policy: inserted Disks in active
            // holders also count, while stored holders/Disks do not.
            let disks = bag
                .items
                .iter()
                .filter(|other| active(other))
                .flat_map(|other| std::iter::once(other.item_id).chain(other.holes.iter().copied()))
                .filter(|id| metadata::get(*id).is_some_and(|row| row.category == 105))
                .count() as i64;
            let per_disk = if item.item_id == 8310212 { 4 } else { 2 };
            *result.entry(1002).or_default() += disks.saturating_mul(per_disk);
        }
        8320049 => {
            // {count2*0.1}% means 20 display units per +2% Curio bonus.
            let curios = bag
                .items
                .iter()
                .filter(|other| active(other))
                .filter(|other| metadata::get(other.item_id).is_some_and(|row| row.category == 102))
                .count() as i64;
            *result.entry(1002).or_default() += curios.saturating_mul(20);
        }
        8330003 => {
            // Exact expression: count2*0.5 + count4*1 + count5*1.5.
            // Each variable is a physical item count. Inserted Disks are
            // explicitly excluded by the description. Rarity 4 is omitted,
            // matching the battle evaluator's unresolved Chalice boundary.
            for flag in [1002, 1004, 1005] {
                result.entry(flag).or_default();
            }
            for other in bag.items.iter().filter(|other| active(other)) {
                let flag = match metadata::get(other.item_id).map(|row| row.rarity) {
                    Some(1) => 1002,
                    Some(2) => 1004,
                    Some(3) => 1005,
                    _ => continue,
                };
                *result.entry(flag).or_default() += 1;
            }
        }
        _ => {}
    }
    result
}

fn attr_map(attr: &RougeItemAttr) -> BTreeMap<i32, i64> {
    attr.attr_ids
        .iter()
        .copied()
        .zip(attr.attr_vals.iter().copied())
        .collect()
}

fn merge_attributes(target: &mut Option<RougeItemAttr>, values: BTreeMap<i32, i64>) {
    if values.is_empty() {
        return;
    }
    let mut all = target.as_ref().map(attr_map).unwrap_or_default();
    all.extend(values);
    let (attr_ids, attr_vals) = all.into_iter().unzip();
    *target = Some(RougeItemAttr {
        attr_ids,
        attr_vals,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(items: Vec<BagItem>) -> (RougeRun, RunBag) {
        super::super::original_tests::init_config();
        let bag = RunBag {
            rows: 8,
            cols: 8,
            items,
            uid_counter: 100,
        };
        let run = RougeRun {
            user_id: 1,
            season: 1,
            layer_index: 0,
            layer_id: 111,
            stage: 0,
            node_id: 1,
            seed: 11,
            graph: String::new(),
            coin: 0,
            power: 0,
            updated_at: 0,
            map_type: 1,
            middle_layer_id: 0,
            position_index: 0,
            choices: "{}".into(),
            finished: "[]".into(),
            bag: serde_json::to_string(&bag).unwrap(),
            shop: String::new(),
            spent_coin: 0,
            retry_num: 0,
            terminal_state: 0,
            minor_count: 0,
            danger_count: 0,
            crisis_count: 0,
            step_count: 0,
            team: "{}".into(),
            power_limit: 40,
            interactive_index: -1,
            interactive_refresh: 0,
            battle_interactive: String::new(),
            team_exp: 0,
            middle_state: "{}".into(),
            details: "{}".into(),
        };
        (run, bag)
    }

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

    fn attrs(item: &RougeItem) -> BTreeMap<i32, i64> {
        item.attr.as_ref().map(attr_map).unwrap_or_default()
    }

    #[test]
    fn placed_disks_fill_both_client_fields_and_sum_repeated_enhancements() {
        let mut holder = item(1, 8310405);
        holder.holes = vec![8140401, 8240402, 8140401];
        holder.hole_ids = vec![2, 3, 4];
        let (run, bag) = fixture(vec![holder]);
        let payload = bag_payload(&run, &bag);
        let layout = &payload.layouts[0];
        assert_eq!(attrs(layout.item.as_ref().unwrap()).get(&1002), Some(&20));
        assert_eq!(layout.attr, layout.item.as_ref().unwrap().attr);
    }

    #[test]
    fn cane_displays_persistent_consumption_growth_after_storage() {
        let (mut run, mut bag) = fixture(vec![item(7, 8310206)]);
        let mut detail = super::super::flow::details(&run);
        detail.artifact_state.counters.insert(7, 2);
        run.details = serde_json::to_string(&detail).unwrap();
        assert_eq!(
            attrs(bag_payload(&run, &bag).layouts[0].item.as_ref().unwrap()).get(&1002),
            Some(&6)
        );
        bag.items[0].row = -1;
        bag.items[0].col = -1;
        run.bag = serde_json::to_string(&bag).unwrap();
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[0])).get(&1002),
            Some(&6)
        );
    }

    #[test]
    fn battle_growth_count2_adds_to_inlays_once_and_survives_warehouse_display() {
        let mut horn = item(1, 8310405);
        horn.holes = vec![8240402];
        horn.hole_ids = vec![21];
        let (mut run, mut bag) = fixture(vec![horn, item(2, 8320310), item(3, 8330418)]);
        let mut detail = super::super::flow::details(&run);
        detail
            .artifact_state
            .permanent_rates
            .insert(1, BTreeMap::from([(205, 120)]));
        detail
            .artifact_state
            .permanent_rates
            .insert(2, BTreeMap::from([(214, 60)]));
        detail
            .artifact_state
            .permanent_rates
            .insert(3, BTreeMap::from([(206, 40)]));
        run.details = serde_json::to_string(&detail).unwrap();
        let payload = bag_payload(&run, &bag);
        assert_eq!(
            attrs(payload.layouts[0].item.as_ref().unwrap()).get(&1002),
            Some(&22)
        );
        assert_eq!(
            payload.layouts[0].attr,
            payload.layouts[0].item.as_ref().unwrap().attr
        );
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[1])).get(&1002),
            Some(&6)
        );
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[2])).get(&1002),
            Some(&4)
        );
        let effects = super::super::artifact_rules::evaluate_battle_with_state(
            config::configs::get(),
            &bag,
            0,
            &detail.artifact_state,
        );
        assert_eq!(effects.attribute_rates.get(&205), Some(&420));
        assert_eq!(effects.attribute_rates.get(&214), Some(&60));
        assert_eq!(effects.attribute_rates.get(&206), Some(&240));
        bag.items[0].row = -1;
        bag.items[0].col = -1;
        run.bag = serde_json::to_string(&bag).unwrap();
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[0])).get(&1002),
            Some(&12),
            "stored client adds the static Disk enhancement itself"
        );
    }

    #[test]
    fn stored_holders_omit_static_enhancements_to_avoid_client_double_counting() {
        let mut holder = item(1, 8310405);
        holder.row = -1;
        holder.col = -1;
        holder.holes = vec![8140401];
        holder.hole_ids = vec![2];
        let (run, bag) = fixture(vec![holder]);
        assert!(item_payload(&run, &bag.items[0]).attr.is_none());
        assert!(bag_payload(&run, &bag).layouts.is_empty());
    }

    #[test]
    fn pouch_reads_only_its_persisted_counter_even_when_stored() {
        let mut pouch = item(7, 8130605);
        pouch.row = -1;
        pouch.col = -1;
        let (mut run, bag) = fixture(vec![pouch, item(8, 8320310)]);
        let mut state = super::super::flow::Details::default();
        state.artifact_state.counters.insert(7, 70);
        state.artifact_state.counters.insert(8, 30);
        run.details = serde_json::to_string(&state).unwrap();
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[0])).get(&1002),
            Some(&70)
        );
        assert!(item_payload(&run, &bag.items[1]).attr.is_none());
        assert_eq!(
            super::super::flow::details(&run).artifact_state.counters[&7],
            70
        );
    }

    #[test]
    fn known_expression_units_match_the_numeric_battle_rules() {
        let mut holder = item(1, 8310212);
        holder.holes = vec![8140401];
        holder.hole_ids = vec![10];
        let mut stored = item(6, 8240402);
        stored.row = -1;
        stored.col = -1;
        let (mut run, bag) = fixture(vec![
            holder,
            item(2, 8330218),
            item(3, 8140601),
            item(4, 8320049),
            item(5, 8220612),
            stored,
        ]);
        run.coin = 549;
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[0])).get(&1002),
            Some(&8)
        );
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[1])).get(&1002),
            Some(&4)
        );
        let curio_count = bag
            .items
            .iter()
            .filter(|item| active(item))
            .filter(|item| metadata::get(item.item_id).is_some_and(|meta| meta.category == 102))
            .count() as i64;
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[3])).get(&1002),
            Some(&(curio_count * 20))
        );
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[4])).get(&1002),
            Some(&100)
        );
        let effects =
            super::super::artifact_rules::evaluate_battle(config::configs::get(), &bag, run.coin);
        assert_eq!(effects.attribute_rates[&211], 80);
        assert_eq!(effects.attribute_rates[&214], 40);
    }

    #[test]
    fn rarity_expression_counts_physical_items_and_keeps_the_chalice_boundary() {
        let mut holder = item(1, 8330003);
        holder.holes = vec![8140401];
        holder.hole_ids = vec![10];
        let (run, bag) = fixture(vec![
            holder,
            item(2, 8140601),
            item(3, 8240402),
            item(4, 8320025),
        ]);
        assert_eq!(
            attrs(&item_payload(&run, &bag.items[0])),
            BTreeMap::from([(1002, 1), (1004, 1), (1005, 1)])
        );
    }

    #[test]
    fn decorating_a_reply_preserves_existing_effects_and_unknown_attributes() {
        let mut holder = item(1, 8310405);
        holder.holes = vec![8140401];
        holder.hole_ids = vec![2];
        let (run, bag) = fixture(vec![holder]);
        let mut layout = RunBag::to_client_layout(&bag.items[0]);
        layout.base_effects = vec![42];
        layout.attr = Some(RougeItemAttr {
            attr_ids: vec![1004],
            attr_vals: vec![3],
        });
        layout.item.as_mut().unwrap().attr = Some(RougeItemAttr {
            attr_ids: vec![1004, 1041],
            attr_vals: vec![1, 2],
        });
        decorate_layouts(&run, &bag, std::slice::from_mut(&mut layout));
        assert_eq!(layout.base_effects, vec![42]);
        assert_eq!(
            attrs(layout.item.as_ref().unwrap()),
            BTreeMap::from([(1002, 5), (1004, 3), (1041, 2)])
        );
        assert_eq!(layout.attr, layout.item.as_ref().unwrap().attr);
    }

    #[test]
    fn removing_the_final_disk_clears_its_counter_without_erasing_other_attributes() {
        let mut holder = item(1, 8310405);
        holder.holes = vec![8140401];
        holder.hole_ids = vec![2];
        let (mut run, mut bag) = fixture(vec![holder]);
        let mut layout = bag_payload(&run, &bag).layouts.remove(0);
        merge_attributes(&mut layout.attr, BTreeMap::from([(1041, 7)]));
        bag.items[0].holes.clear();
        bag.items[0].hole_ids.clear();
        run.bag = serde_json::to_string(&bag).unwrap();
        decorate_layouts(&run, &bag, std::slice::from_mut(&mut layout));
        assert_eq!(
            attrs(layout.item.as_ref().unwrap()),
            BTreeMap::from([(1002, 0), (1041, 7)])
        );
        assert_eq!(layout.attr, layout.item.as_ref().unwrap().attr);
    }

    #[test]
    fn medication_range_activation_is_recomputed_and_other_effects_are_preserved() {
        let source = item(1, 8150006);
        let neighbor = (0..8)
            .flat_map(|row| (0..8).map(move |col| (row, col)))
            .map(|(row, col)| BagItem {
                row,
                col,
                ..item(2, 8150008)
            })
            .find(|other| super::super::artifact_rules::within_range(&source, other))
            .unwrap();
        let (mut run, mut bag) = fixture(vec![source, neighbor]);
        let mut layout = RunBag::to_client_layout(&bag.items[0]);
        layout.base_effects = vec![42];
        decorate_layouts(&run, &bag, std::slice::from_mut(&mut layout));
        assert_eq!(layout.base_effects, vec![0, 1, 42]);
        bag.items[1].row = -1;
        bag.items[1].col = -1;
        run.bag = serde_json::to_string(&bag).unwrap();
        decorate_layouts(&run, &bag, std::slice::from_mut(&mut layout));
        assert_eq!(layout.base_effects, vec![0, 42]);
    }

    #[test]
    fn placed_disks_activate_only_their_placement_description() {
        let (run, bag) = fixture(vec![item(1, 8140401), item(2, 8240702)]);
        let payload = bag_payload(&run, &bag);
        assert_eq!(payload.layouts[0].base_effects, vec![0]);
        assert_eq!(payload.layouts[1].base_effects, vec![0]);
    }
}
