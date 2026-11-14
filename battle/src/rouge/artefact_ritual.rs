//! Light's Dazzling Gaze responds to committed attacks and Ceremonious loss.
//!
//! The client configuration counts ritual use through its own marker buff.
//! Here any real loss of Ceremonious is counted, including forced removal,
//! as a documented local interpretation of the Wiki's broader wording. The
//! battle-local progress is rebuilt from committed events on replay.
use super::*;
use crate::engine::manager::hp::HurtDamageFromType;

fn give(uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid: uid,
        target_uid: uid,
        buff_id,
        amount: Some(amount),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

pub(super) fn observe(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let items = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8320207)
        .cloned()
        .collect::<Vec<_>>();
    if items.is_empty() {
        return vec![];
    }
    match event {
        BattleEvent::Hit(hit)
            if pool.team_type(hit.source_uid) == Some(2)
                && pool
                    .attacker_main
                    .iter()
                    .any(|actor| actor.uid == hit.target_uid)
                && managers.hp.current(hit.target_uid) > 0
                && hit.damage_from == HurtDamageFromType::Skill =>
        {
            // Opcode 22209 is routed through TargetAttacked: primary Skill
            // damage, including hits absorbed by a shield, not DOT/share damage.
            items
                .iter()
                .map(|_| give(hit.target_uid, 90091, 5))
                .collect()
        }
        BattleEvent::BuffAdded(change)
        | BattleEvent::BuffChanged(change)
        | BattleEvent::BuffRemoved(change) => {
            if change.buff_uid <= 0
                || change.target_uid == 0
                || !pool.source_is_attacker(change.target_uid)
                || managers.catalog().buff_type_id(change.buff_id) != 90091
            {
                return vec![];
            }
            let added = matches!(event, BattleEvent::BuffAdded(_));
            let Some(state) = managers.rouge.active.as_mut() else {
                return vec![];
            };
            let previous = state.ritual_buff_amounts.get(&change.buff_uid).copied();
            if added {
                if previous.is_some() {
                    return vec![];
                }
            } else if previous.is_some_and(|amount| amount != change.before_amount) {
                return vec![];
            }
            state
                .ritual_buff_amounts
                .insert(change.buff_uid, change.after_amount);
            let lost = i64::from(
                change
                    .before_amount
                    .saturating_sub(change.after_amount)
                    .max(0),
            );
            if added || lost == 0 {
                return vec![];
            }
            let mut awards = 0_i64;
            for item in &items {
                let reduction = if item.ritual_in_range_count >= 2 {
                    50_i64
                } else {
                    0
                };
                // Clamping impossible zero/negative thresholds to one is an
                // explicit local safety policy, not a claimed official rule.
                let threshold = (100_i64 - i64::from(item.count2) - reduction).max(1);
                let progress = state.ritual_loss_progress.entry(item.uid).or_default();
                let total = progress.saturating_add(lost);
                awards = awards.saturating_add(total / threshold);
                *progress = total % threshold;
            }
            let mut ops = Vec::new();
            for _ in 0..awards {
                for actor in &pool.attacker_main {
                    if managers.hp.current(actor.uid) > 0 {
                        ops.push(give(actor.uid, 90031, 5));
                        ops.push(give(actor.uid, 90171, 1));
                    }
                }
            }
            ops
        }
        _ => vec![],
    }
}

#[cfg(test)]
#[path = "artefact_ritual_tests.rs"]
mod tests;
