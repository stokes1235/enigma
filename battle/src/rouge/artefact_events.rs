//! Event-based artefacts. Events here are produced after a committed damage
//! command or an actual death, never from targeting or a damage preview.
use super::*;
use crate::engine::damage::{DamageFormulaInput, calculate, modifiers};
use crate::engine::manager::{
    buff::BuffStatus,
    ex_point::{ExPointChange, ExPointCommand, ExPointKind},
    hp::{
        DamageEffectKind, HpCommand, HpDamage, HpHeal, HpHealKind, HurtDamageFromType, HurtInfoData,
    },
};
use crate::engine::runtime::determinism::RoundDeterminism;
use sonettobuf::effect_type_enum::EffectType;

fn wound(source_uid: i64, target_uid: i64, buff_id: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid,
        target_uid,
        buff_id,
        amount: Some(1),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

pub(super) fn observe(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
    round: i32,
    random: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let items = context.artefacts.clone();
    let mut ops = Vec::new();
    let pending_key = i32::MIN.saturating_add(round.max(0));
    if let BattleEvent::AllyAction(action) = event
        && action.mode == crate::engine::skill::action::SkillExecutionMode::DirectBig
    {
        // A completed direct ritual releases the per-actor reservation. If a
        // cast is interrupted, the round-scoped key naturally expires next round.
        managers
            .rouge
            .active
            .as_mut()
            .unwrap()
            .growth_triggers
            .remove(&(0, action.source_uid, pending_key));
    }
    if let BattleEvent::AllyAction(action) = event
        && pool.source_is_attacker(action.source_uid)
        && action.enemy_kill_count > 0
    {
        let aid = super::is_support_card(
            managers,
            action.source_uid,
            action.skill_id,
            action.card_index,
        );
        for item in items.iter().filter(|item| item.item_id == 8310108) {
            // Local interpretation: the below-layout clause applies to any
            // credited enemy kill; the base recovery requires a real Aid card.
            let per_kill = (if aid {
                15_i32.saturating_add(item.count2.max(0))
            } else {
                0
            })
            .saturating_add(if item.only_support_below { 15 } else { 0 });
            if per_kill > 0 {
                ops.extend(super::catalyst::power_change(
                    managers,
                    per_kill.saturating_mul(action.enemy_kill_count),
                ));
            }
        }
    }
    if let BattleEvent::Hit(hit) = event
        && pool.source_is_attacker(hit.source_uid)
        && pool.team_type(hit.target_uid) == Some(2)
        && managers.hp.current(hit.source_uid) > 0
        && hit.damage_from == HurtDamageFromType::Skill
        && hit.origin.domain == RuleDomain::Skill
        && hit.origin.key.type_name == "SkillDamage"
        && [90061, 90251, 90261, 6002]
            .into_iter()
            .any(|id| managers.buff.buff_id_or_type_amount(hit.target_uid, id) > 0)
    {
        let count = items
            .iter()
            .filter(|item| item.item_id == 8310114)
            .count()
            .min(i32::MAX as usize) as i64;
        // Clipped HP loss plus absorbed shields mirrors committed damage;
        // shared damage and indirect reactions do not recursively heal.
        let dealt = i64::from(hit.amount.max(0)) + i64::from(hit.shield_absorbed.max(0));
        let efficacy = 1000_i32
            .saturating_sub(managers.buff.buff_act_scalar(
                hit.source_uid,
                crate::engine::skill::buff_act::registry::BuffActKind::InjuryAbsorb,
            ))
            .max(0);
        let amount = ((i128::from(dealt) * i128::from(count) * 100 / 1000) * i128::from(efficacy)
            / 1000)
            .clamp(0, i128::from(i32::MAX)) as i32;
        if amount > 0 {
            ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::Heal(
                HpHeal {
                    origin: ORIGIN,
                    source_uid: hit.source_uid,
                    target_uid: hit.source_uid,
                    amount,
                    config_effect: 0,
                    kind: HpHealKind::Bloodlust,
                },
            ))));
        }
    }
    match event {
        BattleEvent::Hit(hit)
            if pool.source_is_attacker(hit.source_uid)
                && pool.team_type(hit.target_uid) == Some(2)
                && managers.hp.current(hit.source_uid) > 0
                && managers.hp.current(hit.target_uid) > 0
                && hit.damage_from == HurtDamageFromType::Skill
                && hit.origin.domain == RuleDomain::Skill
                && hit.origin.key.type_name == "SkillDamage" =>
        {
            // Local timing: test the target's committed pre-reaction status;
            // statuses granted by this same hit do not qualify until a later hit.
            let negative = !managers
                .buff
                .buffs_with_status(hit.target_uid, BuffStatus::NegativeStatus)
                .is_empty();
            for item in &items {
                if matches!(item.item_id, 8110616 | 8210508) {
                    let coin = i64::from(managers.rouge.resources().map_or(0, |r| r.coin.max(0)));
                    let inlays = i64::from(item.count2.max(0));
                    let base = if item.item_id == 8210508 {
                        // Cap the configured base before Genesis bonuses. Use
                        // wide arithmetic even for malformed snapshot values.
                        (coin * (2 + inlays)).min(2000 + 500 * inlays)
                    } else {
                        (coin * 2).min(2000)
                    }
                    .min(i64::from(i32::MAX)) as i32;
                    let triggered = base > 0
                        && (item.item_id == 8210508
                            || random.roll_permille(item.proc_chance(
                                500_i32.saturating_add(item.count2.saturating_mul(10)),
                            )));
                    if triggered {
                        let amount = calculate(DamageFormulaInput::genesis(
                            base,
                            1000,
                            modifiers::genesis_multiplier(managers, hit.source_uid, hit.target_uid),
                        ));
                        if amount > 0 {
                            ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(
                                HpDamage {
                                    origin: ORIGIN,
                                    source_uid: hit.source_uid,
                                    target_uid: hit.target_uid,
                                    amount,
                                    config_effect: 0,
                                    effect_kind: DamageEffectKind::Genesis,
                                    assassinate: false,
                                    ignore_riposte: false,
                                    hurt: HurtInfoData {
                                        from_uid: hit.source_uid,
                                        is_crit: false,
                                        career_restraint: false,
                                        reduce_hp: 0,
                                        effect_id: hit.skill_id,
                                        skill_id: hit.skill_id,
                                        damage_from: HurtDamageFromType::SkillEffect,
                                        buff_act_id: 0,
                                        buff_uid: 0,
                                        hurt_effect_type: EffectType::Origindamage as i32,
                                        display_amount: None,
                                    },
                                },
                            ))));
                        }
                    }
                    continue;
                }
                let (buff_id, amount) = match item.item_id {
                    8110030 if negative => (5052, 1), // Bleed, two rounds
                    8130505 if random.roll_permille(item.proc_chance(500)) => (4150001, 5), // Burn
                    _ => continue,
                };
                ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(
                    BuffGrant {
                        origin: ORIGIN,
                        source_uid: hit.source_uid,
                        target_uid: hit.target_uid,
                        buff_id,
                        amount: Some(amount),
                        occurrences: 1,
                        child_uid_reservations: 0,
                    },
                ))));
            }
        }
        BattleEvent::Hit(hit)
            if pool.team_type(hit.source_uid) == Some(2)
                && pool.team_type(hit.target_uid) == Some(1)
                && managers.hp.current(hit.target_uid) > 0
                && matches!(
                    hit.damage_from,
                    HurtDamageFromType::Skill | HurtDamageFromType::SkillEffect
                ) =>
        {
            for item in &items {
                if item.item_id == 8220214 {
                    let actor = hit.target_uid;
                    let is_main_attack = hit.damage_from == HurtDamageFromType::Skill
                        && hit.origin.domain == RuleDomain::Skill
                        && hit.origin.key.type_name == "SkillDamage";
                    let reserved = managers
                        .rouge
                        .active
                        .as_ref()
                        .unwrap()
                        .growth_triggers
                        .contains(&(0, actor, pending_key));
                    let skill = managers
                        .entity
                        .snapshot(actor)
                        .and_then(|entity| entity.ex_skill)
                        .filter(|skill_id| managers.catalog().skill_is_big(*skill_id));
                    if is_main_attack
                        && !reserved
                        && managers.ex_point.get(actor) >= 5
                        && ExPointKind::from_wire(managers.ex_point.kind(actor))
                            == ExPointKind::Common
                        && let Some(skill_id) = skill
                        && random.roll_permille(
                            item.proc_chance(
                                250_i32.saturating_add(item.count2.saturating_mul(10)),
                            ),
                        )
                    {
                        managers
                            .rouge
                            .active
                            .as_mut()
                            .unwrap()
                            .growth_triggers
                            .insert((0, actor, pending_key));
                        // Local policy: pay five moxie and interrupt immediately
                        // after the hit; the generated ritual costs no card AP.
                        ops.push(RuleOp::Command(BattleCommand::ExPoint(
                            ExPointCommand::Spend(ExPointChange {
                                origin: ORIGIN,
                                source_uid: actor,
                                target_uid: actor,
                                delta: -5,
                                config_effect: 0,
                                effect_type: EffectType::Expointchange as i32,
                            }),
                        )));
                        let mut invocation: crate::engine::skill::action::SkillInvocation =
                            crate::engine::skill::action::SkillRequest {
                                source_uid: actor,
                                skill_id,
                            }
                            .into();
                        invocation.mode =
                            crate::engine::skill::action::SkillExecutionMode::DirectBig;
                        invocation.additional_moxie = 5;
                        ops.push(RuleOp::Skill(invocation));
                    }
                    continue;
                }
                let (buff_id, stacks) = match item.item_id {
                    8120208 => (90091, 5_i32.saturating_add(item.count2).max(0)),
                    8120317 => (90301, 3),
                    _ => continue,
                };
                ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(
                    BuffGrant {
                        origin: ORIGIN,
                        source_uid: hit.target_uid,
                        target_uid: hit.target_uid,
                        buff_id,
                        amount: Some(stacks),
                        occurrences: 1,
                        child_uid_reservations: 0,
                    },
                ))));
            }
        }
        BattleEvent::AllyAction(action)
            if pool.source_is_attacker(action.source_uid)
                && action.is_attack
                && action.damage_amount > 0
                && managers.hp.current(action.source_uid) > 0 =>
        {
            // `target_uids` is the committed action's affected-target list.
            // Treat a single living enemy in that list as a single-target
            // attack; a one-target AoE is an explicit local approximation.
            let targets = action
                .target_uids
                .iter()
                .copied()
                .filter(|uid| pool.team_type(*uid) == Some(2))
                .collect::<BTreeSet<_>>();
            let extra = crate::engine::skill::action::performs_extra_action(
                action.card_index,
                action.mode,
                action.extra_skill_kind,
            );
            for item in &items {
                if item.item_id == 8110029 && targets.len() == 1 {
                    // The exact target weights are not exported. Use the
                    // replayable battle RNG uniformly over living enemies.
                    let living = pool
                        .defender_main
                        .iter()
                        .filter(|enemy| managers.hp.current(enemy.uid) > 0)
                        .map(|enemy| enemy.uid)
                        .collect::<Vec<_>>();
                    if let Some(index) = random.lua_random_index(living.len()) {
                        let uid = living[index];
                        let base = managers.origin_attribute(action.source_uid, AttrId::Attack);
                        let amount = calculate(DamageFormulaInput::genesis(
                            base,
                            1000,
                            modifiers::genesis_multiplier(managers, action.source_uid, uid),
                        ));
                        if amount > 0 {
                            ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(
                                HpDamage {
                                    origin: ORIGIN,
                                    source_uid: action.source_uid,
                                    target_uid: uid,
                                    amount,
                                    config_effect: 0,
                                    effect_kind: DamageEffectKind::Genesis,
                                    assassinate: false,
                                    ignore_riposte: false,
                                    hurt: HurtInfoData {
                                        from_uid: action.source_uid,
                                        is_crit: false,
                                        career_restraint: false,
                                        reduce_hp: 0,
                                        effect_id: action.skill_id,
                                        skill_id: action.skill_id,
                                        damage_from: HurtDamageFromType::SkillEffect,
                                        buff_act_id: 0,
                                        buff_uid: 0,
                                        hurt_effect_type: EffectType::Origindamage as i32,
                                        display_amount: None,
                                    },
                                },
                            ))));
                        }
                    }
                } else if item.item_id == 8110507
                    && targets.len() == 1
                    && managers.catalog().skill_is_big(action.skill_id)
                    && managers.hp.current(*targets.iter().next().unwrap()) > 0
                {
                    let uid = *targets.iter().next().unwrap();
                    let chance = item.proc_chance(
                        500_i32
                            .saturating_add(item.count2.saturating_mul(10))
                            .clamp(0, 1000),
                    );
                    for buff_id in [3091, 3101] {
                        // Blind, Mis-aim: one round each
                        if random.roll_permille(chance) {
                            ops.push(wound(action.source_uid, uid, buff_id));
                        }
                    }
                } else if item.item_id == 8110048
                    && targets.len() == 1
                    && managers.hp.current(*targets.iter().next().unwrap()) > 0
                {
                    let uid = *targets.iter().next().unwrap();
                    ops.push(wound(action.source_uid, uid, 90061));
                    if extra
                        && random.roll_permille(
                            item.proc_chance(
                                500_i32.saturating_add(item.count2.saturating_mul(10)),
                            ),
                        )
                    {
                        ops.push(wound(action.source_uid, uid, 90061));
                    }
                } else if item.item_id == 8110716 {
                    // Replay reconstructs this deterministic per-item, per-actor,
                    // per-round marker; negative keys do not collide with growth.
                    let trigger = i32::MIN.saturating_add(round.max(0));
                    let first = managers
                        .rouge
                        .active
                        .as_mut()
                        .unwrap()
                        .growth_triggers
                        .insert((item.uid, action.source_uid, trigger));
                    for &uid in &targets {
                        if managers.hp.current(uid) <= 0 {
                            continue;
                        }
                        if first {
                            ops.push(wound(action.source_uid, uid, 90062));
                        }
                        if extra
                            && random.roll_permille(item.proc_chance(
                                500_i32.saturating_add(item.count2.saturating_mul(10)),
                            ))
                        {
                            ops.push(wound(action.source_uid, uid, 90062));
                        }
                    }
                }
            }
        }
        BattleEvent::EntityDied(death)
            if pool.source_is_attacker(death.source_uid)
                && pool.team_type(death.target_uid) == Some(2)
                && managers.hp.current(death.source_uid) > 0 =>
        {
            for _ in items.iter().filter(|item| item.item_id == 8320617) {
                ops.extend(super::catalyst::coin_change(managers, 3));
            }
            for _ in items.iter().filter(|item| item.item_id == 8230037) {
                ops.push(RuleOp::Command(BattleCommand::ExPoint(
                    ExPointCommand::Change(ExPointChange {
                        origin: ORIGIN,
                        source_uid: death.source_uid,
                        target_uid: death.source_uid,
                        delta: 1,
                        config_effect: 0,
                        effect_type: EffectType::Expointchange as i32,
                    }),
                )));
            }
        }
        _ => {}
    }
    ops
}

#[cfg(test)]
#[path = "artefact_events_tests.rs"]
mod tests;
