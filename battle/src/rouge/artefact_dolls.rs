//! The two money dolls respond to committed increases in Might or Fortune.
//! "Small chance" has no published rate; 10% per gain event is a local policy.
use super::*;
use crate::engine::runtime::determinism::RoundDeterminism;

pub(super) fn observe(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
    random: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let (change, added) = match event {
        BattleEvent::BuffAdded(change) => (change, true),
        BattleEvent::BuffChanged(change) | BattleEvent::BuffRemoved(change) => (change, false),
        _ => return vec![],
    };
    if change.buff_uid == 0 || !pool.source_is_attacker(change.target_uid) {
        return vec![];
    }
    let id = managers.catalog().buff_type_id(change.buff_id);
    if !matches!(id, 90001 | 90011) {
        return vec![];
    }
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    let previous = state.doll_buff_amounts.get(&change.buff_uid).copied();
    if added {
        if previous.is_some() {
            return vec![];
        }
    } else if previous.is_some_and(|amount| amount != change.before_amount) {
        return vec![];
    }
    state
        .doll_buff_amounts
        .insert(change.buff_uid, change.after_amount);
    let delta = change.after_amount.saturating_sub(change.before_amount);
    if delta <= 0 || managers.hp.current(change.target_uid) <= 0 {
        return vec![];
    }
    let matching = managers
        .rouge
        .context()
        .map(|c| {
            c.artefacts
                .iter()
                .filter(|item| {
                    (item.item_id == 8130603 && id == 90011)
                        || (item.item_id == 8130604 && id == 90001)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut ops = Vec::new();
    for item in matching {
        if random.roll_permille(item.proc_chance(100)) {
            ops.extend(super::catalyst::coin_change(
                managers,
                if item.item_id == 8130603 { 1 } else { 2 },
            ));
        }
    }
    ops
}

#[cfg(test)]
#[path = "artefact_dolls_tests.rs"]
mod tests;
