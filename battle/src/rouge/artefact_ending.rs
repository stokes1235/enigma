//! The three special ending artefacts which use the normal battle managers.
//!
//! The client definitions specify the two one-round shield carriers and their
//! 200% attack / 20% max-HP formulas. In the absence of a trace containing
//! these items, the shield recipient and event timing are local policies:
//! living deployed allies each use their own attributes; the final-battle
//! shield is granted once per committed enemy death attributed to our side.

use super::*;

fn shield(managers: &BattleManagers, uid: i64, buff_id: i32, attr: AttrId, rate: i32) -> RuleOp {
    // The descriptions specify no stacking cap. Select the larger positive
    // self attribute as a high cap so an older, larger shield is not reduced
    // and multiple grants in the same event can add independently. The
    // configured carrier still controls the actual one-round lifetime.
    let hp = managers.origin_attribute(uid, AttrId::Hp);
    let attack = managers.origin_attribute(uid, AttrId::Attack);
    let max_attr = if attack > hp { AttrId::Attack } else { AttrId::Hp };
    RuleOp::Command(BattleCommand::Shield(ShieldCommand {
        origin: ORIGIN,
        source_uid: uid,
        target_uid: uid,
        buff_id,
        amount_attr: attr,
        amount_rate: rate,
        multiplier_bonus: None,
        max_attr,
        max_rate: i32::MAX,
        scope: ShieldScope::Entity,
        carrier_uid: ShieldCarrierUid::Definition,
    }))
}

fn shields(
    managers: &BattleManagers,
    pool: &TargetPool,
    copies: usize,
    buff_id: i32,
    attr: AttrId,
    rate: i32,
) -> Vec<RuleOp> {
    let mut result = Vec::new();
    for entity in &pool.attacker_main {
        if managers.hp.current(entity.uid) <= 0 {
            continue;
        }
        for _ in 0..copies {
            result.push(shield(managers, entity.uid, buff_id, attr, rate));
        }
    }
    result
}

pub(super) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
) -> Vec<RuleOp> {
    let Some(active) = managers.rouge.active.as_mut() else {
        return Vec::new();
    };
    if round <= 0 || active.ending_last_round.is_some_and(|seen| seen >= round) {
        return Vec::new();
    }
    active.ending_last_round = Some(round);
    let copies = active
        .context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8180501)
        .count();
    shields(managers, pool, copies, 81805010, AttrId::Attack, 2000)
}

pub(super) fn observe(
    managers: &BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return Vec::new();
    };
    let BattleEvent::EntityDied(death) = event else {
        return Vec::new();
    };
    if !context.final_battle
        || !pool.source_is_attacker(death.source_uid)
        || pool.team_type(death.target_uid) != Some(2)
    {
        return Vec::new();
    }
    let copies = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8180503)
        .count();
    shields(managers, pool, copies, 81805030, AttrId::Hp, 200)
}

#[cfg(test)]
#[path = "artefact_ending_tests.rs"]
mod tests;
