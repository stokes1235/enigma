//! Dynamo-loss artefacts use committed per-instance changes, not attack
//! counts. Per-item, per-actor remainders persist across rounds and replay.
//!
//! The client does not document overlapping-layout ordering or thresholds
//! below one; UID ordering and a minimum threshold of one are local policies.
use super::*;
use crate::engine::{
    runtime::determinism::RoundDeterminism,
    skill::action::{SkillInvocation, SkillRequest},
};

fn grant(uid: i64, id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid: uid,
        target_uid: uid,
        buff_id: id,
        amount: Some(amount),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn crossing(
    state: &mut ActiveRouge,
    item: i64,
    owner: i64,
    key: i32,
    loss: i32,
    threshold: i32,
) -> i32 {
    let remainder = state
        .electric_remainders
        .entry((item, owner, key))
        .or_default();
    let total = remainder.saturating_add(loss.max(0));
    let threshold = threshold.max(1);
    *remainder = total % threshold;
    total / threshold
}

pub(super) fn observe(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
    round: i32,
    rng: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let (change, added) = match event {
        BattleEvent::BuffAdded(change) => (change, true),
        BattleEvent::BuffChanged(change) | BattleEvent::BuffRemoved(change) => (change, false),
        _ => return vec![],
    };
    if change.buff_uid == 0
        || !pool.source_is_attacker(change.target_uid)
        || managers.catalog().buff_type_id(change.buff_id) != 90071
    {
        return vec![];
    }
    // This event has already passed the buff manager's stack cap. Count actual
    // positive deltas (including recovery after prior consumption), never the
    // requested amount, and only for a deployed, living actor.
    let deployed_and_alive = pool
        .attacker_main
        .iter()
        .any(|actor| actor.uid == change.target_uid)
        && managers.hp.current(change.target_uid) > 0;
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    let observation_key = (change.target_uid, change.buff_uid);
    if added {
        // A newly allocated UID starts at zero; later positive refreshes are
        // BuffChanged. Ignore a repeated add notification without rewinding.
        if state
            .electric_observed_amounts
            .contains_key(&observation_key)
        {
            return vec![];
        }
    } else if state
        .electric_observed_amounts
        .get(&observation_key)
        .is_some_and(|n| *n != change.before_amount)
    {
        return vec![];
    }
    state
        .electric_observed_amounts
        .insert(observation_key, change.after_amount);
    let gained = change
        .after_amount
        .saturating_sub(change.before_amount)
        .max(0);
    if gained > 0
        && deployed_and_alive
        && state
            .context
            .artefacts
            .iter()
            .any(|item| item.item_id == 8310815)
    {
        // Attacker settlement precedes the enemy phase. Gains after it belong
        // to the next attacker settlement; opening gains belong to round one.
        let current = round.max(1);
        let target_round = if state.electric_ended_rounds.contains(&current) {
            current.saturating_add(1)
        } else {
            current
        };
        let total = state
            .electric_round_gains
            .entry((target_round, change.target_uid))
            .or_default();
        *total = total.saturating_add(gained);
    }
    if added {
        return vec![];
    }
    // Dynamo's client-defined stack cap is 30.
    let lost = change
        .before_amount
        .saturating_sub(change.after_amount)
        .clamp(0, 30);
    if lost == 0 || managers.hp.current(change.target_uid) <= 0 {
        return vec![];
    }
    let mut items: Vec<_> = managers
        .rouge
        .context()
        .map(|c| {
            c.artefacts
                .iter()
                .filter(|item| matches!(item.item_id, 8120808 | 8220813 | 8320812))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    items.sort_unstable_by_key(|item| item.uid);
    let mut ops = Vec::new();
    for item in items {
        let threshold = 10_i32.saturating_sub(item.count2).max(1);
        let state = managers.rouge.active.as_mut().unwrap();
        match item.item_id {
            8120808 => {
                let n = crossing(state, item.uid, change.target_uid, 1, lost, threshold);
                if n > 0 {
                    ops.push(grant(change.target_uid, 90281, n));
                }
            }
            8220813 => {
                let restore = crossing(state, item.uid, change.target_uid, 2, lost, threshold);
                let permanent = crossing(state, item.uid, change.target_uid, 3, lost, 5);
                if restore > 0 {
                    ops.push(grant(change.target_uid, 90071, restore.saturating_mul(2)));
                }
                if permanent > 0 {
                    let draws = super::artefact_turns::permanent_buffs_with_pending(
                        managers,
                        change.target_uid,
                        permanent as usize,
                        rng,
                        &ops,
                    );
                    ops.extend(draws);
                }
            }
            8320812 => {
                let dodge = crossing(state, item.uid, change.target_uid, 4, lost, threshold);
                ops.push(grant(change.target_uid, 90041, lost));
                if dodge > 0 {
                    ops.push(grant(change.target_uid, 90201, dodge));
                }
            }
            _ => unreachable!(),
        }
    }
    ops
}

/// Omega's marker is determined by the actual team-wide gains in this round,
/// matching configured AccTeamAddBuffCountByBuffId(90071, 5). Only the gains
/// of characters still alive on the deployed team at settlement are included;
/// this death boundary and stable tie order are explicit local choices.
pub(super) fn round_end(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
) -> Vec<RuleOp> {
    if round <= 0 {
        return vec![];
    }
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    let copies = state
        .context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8310815)
        .count();
    if copies == 0 || !state.electric_ended_rounds.insert(round) {
        return vec![];
    }
    let gains = state
        .electric_round_gains
        .iter()
        .filter_map(|(&(r, uid), &count)| (r == round).then_some((uid, count)))
        .collect::<BTreeMap<_, _>>();
    // Earlier round observations no longer contribute to a new settlement.
    state.electric_round_gains.retain(|(r, _), _| *r > round);
    let living = pool
        .attacker_main
        .iter()
        .enumerate()
        .filter(|(_, actor)| managers.hp.current(actor.uid) > 0)
        .collect::<Vec<_>>();
    let total = living
        .iter()
        .map(|(_, actor)| gains.get(&actor.uid).copied().unwrap_or(0))
        .fold(0_i32, i32::saturating_add);
    if total < 5
        || !pool
            .defender_main
            .iter()
            .any(|enemy| managers.hp.current(enemy.uid) > 0)
    {
        return vec![];
    }
    let Some(source) = living
        .into_iter()
        .max_by_key(|(position, actor)| {
            (
                managers.origin_attribute(actor.uid, AttrId::Attack),
                std::cmp::Reverse(*position),
            )
        })
        .map(|(_, actor)| actor.uid)
    else {
        return vec![];
    };
    (0..copies)
        .map(|_| {
            let cast: SkillInvocation = SkillRequest {
                source_uid: source,
                skill_id: 811080592,
            }
            .into();
            RuleOp::Skill(cast)
        })
        .collect()
}

#[cfg(test)]
#[path = "artefact_electric_tests.rs"]
mod tests;
