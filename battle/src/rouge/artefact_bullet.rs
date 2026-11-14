//! Flintlock reacts to committed Bullet losses, never to previews or grants.
//!
//! The local rule treats one decrease event for one Bullet type as one loss,
//! even when that event removes several stacks. Its replacement is drawn from
//! the nine current Bullet types in the configured 91130030 collection.
use super::*;
use crate::engine::runtime::determinism::RoundDeterminism;

const BULLETS: [i32; 9] = [
    91130031, 91130032, 91130033, 91130034, 91130035, 91130036, 91130037, 91130038, 91130039,
];

fn is_bullet(managers: &BattleManagers, buff_id: i32) -> bool {
    managers
        .catalog()
        .game_data()
        .skill_buff
        .get(buff_id)
        .is_some_and(|row| {
            row.features.split('|').any(|feature| {
                feature
                    .split('#')
                    .next()
                    .and_then(|id| id.parse::<i32>().ok())
                    == Some(827)
            })
        })
}

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
    if change.buff_uid <= 0
        || !pool
            .attacker_main
            .iter()
            .any(|actor| actor.uid == change.target_uid)
        || !is_bullet(managers, change.buff_id)
    {
        return vec![];
    }
    // This ledger is separate from growth and other artefact subscriptions.
    // An Added/Changed/Removed sequence can describe the same committed
    // transition more than once. Buff instance UIDs are battle-unique.
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    let previous = state.bullet_buff_amounts.get(&change.buff_uid).copied();
    if added {
        if previous.is_some() {
            return vec![];
        }
    } else if previous.is_some_and(|amount| amount != change.before_amount) {
        return vec![];
    }
    state
        .bullet_buff_amounts
        .insert(change.buff_uid, change.after_amount);
    if change.after_amount >= change.before_amount || managers.hp.current(change.target_uid) <= 0 {
        return vec![];
    }
    let copies = managers
        .rouge
        .context()
        .map(|context| {
            context
                .artefacts
                .iter()
                .filter(|item| item.item_id == 8210713)
                .count()
        })
        .unwrap_or_default();
    let mut ops = Vec::with_capacity(copies.saturating_mul(2));
    for _ in 0..copies {
        if let Some(index) = random.lua_random_index(BULLETS.len()) {
            ops.push(grant(change.target_uid, change.target_uid, BULLETS[index]));
            ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(
                BuffGrant {
                    origin: ORIGIN,
                    source_uid: change.target_uid,
                    target_uid: change.target_uid,
                    buff_id: 4150002,
                    amount: Some(10),
                    occurrences: 1,
                    child_uid_reservations: 0,
                },
            ))));
        }
    }
    ops
}

#[cfg(test)]
#[path = "artefact_bullet_tests.rs"]
mod tests;
