//! Ownership transitions shared by drops, events, purchases and compositions.
//! Placement and inlay moves retain ownership and must never retrigger a gift.
use super::{
    bag::{BagItem, RunBag},
    flow, *,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn owned(bag: &RunBag) -> BTreeMap<i64, i32> {
    let mut result = BTreeMap::new();
    for item in &bag.items {
        result.insert(item.uid, item.item_id);
        for (&uid, &id) in item.hole_ids.iter().zip(&item.holes) {
            if uid > 0 && id > 0 {
                result.insert(uid, id);
            }
        }
    }
    result
}

fn event_bag(bag: &RunBag, uid: i64) -> RunBag {
    let mut source = bag.clone();
    if !source.items.iter().any(|item| item.uid == uid) {
        if let Some(id) = owned(bag).get(&uid) {
            source.items.push(BagItem {
                uid,
                item_id: *id,
                row: -1,
                col: -1,
                rotation: 0,
                holes: vec![],
                hole_ids: vec![],
            });
        }
    }
    source
}

/// Apply bounded gift chains before the same optimistic action is committed.
/// Losses precede acquisitions, each sorted by UID. This ordering is a local
/// policy for simultaneous exchanges; moving to storage is never a loss.
pub fn prepare(expected: &RougeRun, run: &mut RougeRun) -> Result<(), AppError> {
    // Early local saves used {} for an empty bag. Do not invent ownership or
    // retroactively issue acquisition gifts when that legacy state is unknown.
    let Ok(mut previous) = serde_json::from_str::<RunBag>(&expected.bag) else {
        return Ok(());
    };
    let mut handled = BTreeSet::new();
    for _ in 0..64 {
        let current: RunBag = serde_json::from_str(&run.bag)?;
        let old = owned(&previous);
        let new = owned(&current);
        let losses: Vec<_> = old
            .keys()
            .filter(|uid| !new.contains_key(uid))
            .copied()
            .collect();
        let gains: Vec<_> = new
            .keys()
            .filter(|uid| !old.contains_key(uid))
            .copied()
            .collect();
        if losses.is_empty() && gains.is_empty() {
            return Ok(());
        }
        for (losing, uid) in losses
            .into_iter()
            .map(|uid| (true, uid))
            .chain(gains.into_iter().map(|uid| (false, uid)))
        {
            if !handled.insert((losing, uid)) {
                continue;
            }
            let before = run.clone();
            let trigger = if losing {
                artifact_rules::MapTrigger::Lose(uid)
            } else {
                artifact_rules::MapTrigger::Acquire(uid)
            };
            let live: RunBag = serde_json::from_str(&run.bag)?;
            let source = event_bag(
                if losing {
                    &previous
                } else if owned(&live).contains_key(&uid) {
                    &live
                } else {
                    &current
                },
                uid,
            );
            flow::apply_map_effects_from(run, trigger, &source)?;
            let before_items = owned(&serde_json::from_str(&before.bag)?).len() as i32;
            let after_items = owned(&serde_json::from_str(&run.bag)?).len() as i32;
            run.middle_state = updated_entrust(
                &before,
                EntrustChange {
                    gained_coin: (run.coin - before.coin).max(0),
                    spent_coin: (before.coin - run.coin).max(0),
                    gained_power: (run.power - before.power).max(0),
                    spent_power: (before.power - run.power).max(0),
                    collections: (after_items - before_items).max(0),
                    ..Default::default()
                },
            )?;
        }
        previous = current;
    }
    Err(AppError::InvalidRequest)
}

pub fn initialize(run: &mut RougeRun) -> Result<(), AppError> {
    let mut before = run.clone();
    let mut bag: RunBag = serde_json::from_str(&run.bag)?;
    bag.items.clear();
    before.bag = serde_json::to_string(&bag)?;
    prepare(&before, run)
}

/// Transform in place when the new shape still fits; otherwise store it.
/// All enchants keep their UIDs, including overflow from a smaller holder.
pub fn replace(bag: &mut RunBag, uid: i64, id: i32) -> Result<(), AppError> {
    let index = bag
        .items
        .iter()
        .position(|item| item.uid == uid)
        .ok_or(AppError::InvalidRequest)?;
    let old = bag.items[index].clone();
    let holes = collection_shapes::hole_num(id).max(0) as usize;
    let mut overflow = Vec::new();
    for (&disk_id, &disk_uid) in old.holes.iter().zip(&old.hole_ids).skip(holes) {
        if disk_id > 0 && disk_uid > 0 {
            overflow.push(BagItem {
                uid: disk_uid,
                item_id: disk_id,
                row: -1,
                col: -1,
                rotation: 0,
                holes: vec![],
                hole_ids: vec![],
            });
        }
    }
    let item = &mut bag.items[index];
    item.item_id = id;
    item.row = -1;
    item.col = -1;
    item.holes.resize(holes, 0);
    item.hole_ids.resize(holes, 0);
    bag.items.extend(overflow);
    if RunBag::is_placed(&old) {
        let _ = bag.place(uid, old.row, old.col, old.rotation);
    }
    Ok(())
}
