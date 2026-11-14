//! Resource artefacts tied to actual round boundaries.
//!
//! Each draw is derived from the frozen fight seed, source UID, round and
//! phase. Replaying the same fight therefore reconstructs the same balance.
//! Missing or insufficient currency is locally interpreted as losing at most
//! the current balance. The separate 1% bonus draw is independent of the
//! regular end-of-round gain; neither choice is claimed as official evidence.
use super::*;

fn draw(seed: u64, uid: i64, round: i32, phase: u64) -> i32 {
    let mut v = seed ^ (uid as u64).rotate_left(13) ^ (round as u64).rotate_left(37) ^ phase;
    v = v.wrapping_add(0x9e3779b97f4a7c15);
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((v ^ (v >> 31)) % 1000) as i32
}

pub(super) fn round(managers: &mut BattleManagers, round: i32, start: bool) -> Vec<RuleOp> {
    if !(1..=5).contains(&round) {
        return vec![];
    }
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let seed = context.entry.as_ref().map_or(0, |entry| entry.seed);
    let mut items: Vec<_> = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8210611)
        .cloned()
        .collect();
    items.sort_unstable_by_key(|item| item.uid);
    let mut ops = Vec::new();
    for item in items {
        let key = if start { 82106111 } else { 82106112 };
        if !managers
            .rouge
            .active
            .as_mut()
            .unwrap()
            .growth_triggers
            .insert((item.uid, i64::from(round), key))
        {
            continue;
        }
        if start {
            let chance = item.proc_chance(500_i32.saturating_sub(item.count2.saturating_mul(10)));
            if draw(seed, item.uid, round, 0x13_8210611) < chance {
                let amount = managers.rouge.resources().map_or(0, |r| r.coin.min(10));
                ops.extend(super::catalyst::coin_change(managers, -amount));
            }
        } else {
            let chance = item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            if draw(seed, item.uid, round, 0x27_8210611) < chance {
                ops.extend(super::catalyst::coin_change(managers, 10));
            }
            if draw(seed, item.uid, round, 0x31_8210611) < item.proc_chance(10) {
                ops.extend(super::catalyst::coin_change(managers, 666));
            }
        }
    }
    ops
}

#[cfg(test)]
#[path = "artefact_balance_tests.rs"]
mod tests;
