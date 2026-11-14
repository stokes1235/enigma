//! Verified original-Rouge turn and completed-action artefacts.
//!
//! Values are taken from the saved 3.9 Collection of Artefacts Wiki and the
//! current rouge_desc, rather than the older same-name 9xxxxxx skills. Buff
//! definitions 90001/90011/90091/90301 encode the actual status, stack cap,
//! attributes and consumption rules in the shipped game data.
use super::*;
use crate::engine::{
    manager::{
        buff::{BuffConsume, BuffDispel, BuffSelector, BuffStatus, DepletedBuff},
        card::{CardCommand, CardMarkTemporary},
        eureka::{EUREKA_RESOURCE_ID, EurekaChange, EurekaCommand},
        ex_point::{ExPointChange, ExPointCommand},
        hp::{HpCommand, HpHeal, HpHealKind},
    },
    runtime::determinism::RoundDeterminism,
    skill::action::{ActionEvent, SkillInvocation, SkillRequest},
};
use sonettobuf::effect_type_enum::EffectType;

#[derive(Debug, Clone, Default)]
pub(super) struct TurnLedger {
    last_start_round: Option<i32>,
    last_end_round: Option<i32>,
    /// Per artefact instance and actor; rebuilt by replay after reconnect.
    bandage_used: std::collections::BTreeSet<(i64, i64)>,
    /// Actual completed normal incantations in this round, never queued cards.
    cast_actors: std::collections::BTreeSet<i64>,
    /// All actual completed actions; independent of selected-card tracking.
    acted_actors: std::collections::BTreeSet<i64>,
    /// Actual completed support incantations per active character.
    support_casts: std::collections::BTreeMap<i64, i32>,
}

fn give(uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    give_from(uid, uid, buff_id, amount)
}

fn give_from(source_uid: i64, target_uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid,
        target_uid,
        buff_id,
        amount: Some(amount),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn moxie(uid: i64, delta: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::ExPoint(ExPointCommand::Change(
        ExPointChange {
            origin: ORIGIN,
            source_uid: uid,
            target_uid: uid,
            delta,
            config_effect: 0,
            effect_type: EffectType::Expointchange as i32,
        },
    )))
}

fn eureka(uid: i64, delta: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Eureka(EurekaCommand::Change(EurekaChange {
        origin: ORIGIN,
        source_uid: uid,
        target_uid: uid,
        power_id: EUREKA_RESOURCE_ID,
        delta,
        effect_type: EffectType::Powerchange as i32,
    })))
}

/// Random choices use the replayed battle RNG and favor absent statuses.
pub(super) fn permanent_buffs(
    managers: &BattleManagers,
    uid: i64,
    amount: usize,
    rng: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    permanent_buffs_with_pending(managers, uid, amount, rng, &[])
}

/// Include grants already queued in this event when deciding which states are
/// absent. After all six are owned, each further draw may repeat any state.
pub(super) fn permanent_buffs_with_pending(
    managers: &BattleManagers,
    uid: i64,
    amount: usize,
    rng: &mut RoundDeterminism,
    pending: &[RuleOp],
) -> Vec<RuleOp> {
    let all = [90001, 90011, 90041, 90021, 90031, 90281];
    let mut owned: std::collections::BTreeSet<i32> = all
        .iter()
        .copied()
        .filter(|id| managers.buff.buff_id_amount(uid, *id) > 0)
        .collect();
    for op in pending {
        if let RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(grant))) = op {
            if grant.target_uid == uid && all.contains(&grant.buff_id) {
                owned.insert(grant.buff_id);
            }
        }
    }
    let mut out = Vec::new();
    for _ in 0..amount {
        let missing: Vec<_> = all
            .iter()
            .copied()
            .filter(|id| !owned.contains(id))
            .collect();
        let candidates: &[i32] = if missing.is_empty() { &all } else { &missing };
        let Some(index) = rng.lua_random_index(candidates.len()) else {
            break;
        };
        let buff = candidates[index];
        owned.insert(buff);
        out.push(give(uid, buff, 1));
    }
    out
}

/// The client showTag classifies buff (4), counter (5), healing (6) and
/// channel (9) spells independently of Reality/Mental effectTag.
fn spell_show_tag(managers: &BattleManagers, skill_id: i32) -> Option<i32> {
    let catalog = managers.catalog();
    catalog
        .game_data()
        .skill_effect
        .get(catalog.skill_effect_id(skill_id))
        .map(|effect| effect.show_tag)
}

fn live_main(managers: &BattleManagers, pool: &TargetPool) -> Vec<i64> {
    pool.attacker_main
        .iter()
        .filter_map(|entity| (managers.hp.current(entity.uid) > 0).then_some(entity.uid))
        .collect()
}

/// Resolve fading only for the exact timed buff instances whose duration is
/// about to advance. The engine removes the one-round fading status itself.
/// Keeping this on the normal command path preserves events and replay.
pub(super) fn fade_before_duration(
    managers: &BattleManagers,
    stage: i32,
    snapshot: &[(i64, i64)],
) -> Vec<RuleOp> {
    if managers.rouge.context().is_none() || snapshot.is_empty() || !matches!(stage, 103 | 303) {
        return Vec::new();
    }
    let owners: Vec<i64> = snapshot
        .iter()
        .map(|(owner, _)| *owner)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let scheduled: std::collections::BTreeSet<_> = snapshot.iter().copied().collect();
    let mut ops = Vec::new();
    for target in managers.buff.duration_targets(stage, &owners) {
        if !scheduled.contains(&(target.owner_uid, target.buff_uid))
            || managers.hp.current(target.owner_uid) <= 0
        {
            continue;
        }
        let Some(active) = managers
            .buff
            .active_for(target.owner_uid)
            .find(|buff| buff.uid == Some(target.buff_uid))
        else {
            continue;
        };
        let base = match (stage, active.buff_id.unwrap_or_default()) {
            (103, 90022) => 90021,
            (103, 90032) => 90031,
            (303, 90002) => 90001,
            (303, 90012) => 90011,
            (303, 90042) => 90041,
            (303, 90052) => 90051,
            _ => continue,
        };
        let amount = active
            .layer
            .filter(|v| *v > 0)
            .or(active.count.filter(|v| *v > 0))
            .unwrap_or(1);
        ops.push(RuleOp::Command(BattleCommand::Buff(
            BuffCommand::ConsumeCoalesced(BuffConsume {
                origin: ORIGIN,
                target_uid: target.owner_uid,
                selector: BuffSelector::ExactId(base),
                amount,
                depleted: DepletedBuff::Remove,
            }),
        )));
    }
    ops
}

/// A slingshot grants Lucky before the selected rank-2/3 card resolves, so
/// its crit rate applies to this same cast rather than only the next one.
pub(super) fn before_card(
    managers: &BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    skill_id: i32,
    card_index: i32,
    is_ultimate: bool,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    if card_index <= 0
        || !pool
            .attacker_main
            .iter()
            .any(|entity| entity.uid == source_uid)
        || managers.hp.current(source_uid) <= 0
    {
        return vec![];
    }
    if is_ultimate {
        // Snapshot Ceremonious before its normal ultimate consumption using
        // this artefact's own hidden, one-round marker. Completion consumes
        // the marker and returns exactly the captured number of stacks.
        let ritual = managers.buff.buff_id_amount(source_uid, 90091).max(0);
        let refund = context
            .artefacts
            .iter()
            .filter(|item| item.item_id == 8210205)
            .map(|item| ritual / 5_i32.saturating_sub(item.count2).max(1))
            .fold(0_i32, i32::saturating_add);
        let mut ops = Vec::new();
        let old = managers.buff.buff_id_amount(source_uid, 821020511);
        if old > 0 && context.artefacts.iter().any(|item| item.item_id == 8210205) {
            ops.push(RuleOp::Command(BattleCommand::Buff(
                BuffCommand::ConsumeCoalesced(BuffConsume {
                    origin: ORIGIN,
                    target_uid: source_uid,
                    selector: BuffSelector::ExactId(821020511),
                    amount: old,
                    depleted: DepletedBuff::Remove,
                }),
            )));
        }
        if refund > 0 {
            ops.push(give(source_uid, 821020511, refund));
        }
        return ops;
    }
    if !matches!(managers.catalog().skill_rank(skill_id), 2 | 3) {
        return vec![];
    }
    // The ointment resolves on the selected high-rank cast before the skill.
    // Pick a single lowest HP ratio from the pre-effect snapshot; equal ratios
    // retain formation order, and the caster may also be the lowest ally.
    let lowest = live_main(managers, pool).into_iter().min_by(|a, b| {
        let a_hp = managers.hp.get(*a);
        let b_hp = managers.hp.get(*b);
        (i64::from(a_hp.current) * i64::from(b_hp.max.max(1)))
            .cmp(&(i64::from(b_hp.current) * i64::from(a_hp.max.max(1))))
    });
    let mut ops = Vec::new();
    for item in &context.artefacts {
        match item.item_id {
            8210314 => ops.push(give(source_uid, 90011, 2)),
            8310315 => ops.push(give(source_uid, 90011, 4)),
            8230316 => {
                for (uid, rate) in [(Some(source_uid), 100), (lowest, 50)] {
                    let Some(uid) = uid else { continue };
                    ops.push(give(uid, 302, 1));
                    let amount = ((i64::from(managers.hp.get(uid).max.max(0)) * rate) / 1000)
                        .min(i64::from(i32::MAX)) as i32;
                    if amount > 0 {
                        ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::Heal(
                            HpHeal {
                                origin: ORIGIN,
                                source_uid,
                                target_uid: uid,
                                amount,
                                config_effect: 0,
                                kind: HpHealKind::Normal,
                            },
                        ))));
                    }
                }
            }
            _ => {}
        }
    }
    ops
}

/// Called after the opening/next-round hand has been dealt. Take one snapshot
/// of the hand and existing buffs before generating any round-start rewards.
/// Count real incantation cards, including Support and special incantations,
/// but exclude Ultimate cards. This counting/timing is a local policy until a
/// trace containing the relevant artefacts establishes exact ordering.
pub(super) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    determinism: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    if round <= 0
        || state
            .turn_effects
            .last_start_round
            .is_some_and(|seen| seen >= round)
    {
        return vec![];
    }
    state.turn_effects.last_start_round = Some(round);
    state.turn_effects.cast_actors.clear();
    state.turn_effects.acted_actors.clear();
    state.turn_effects.support_casts.clear();
    let items = state.context.artefacts.clone();
    let formation_slots = state
        .context
        .catalyst
        .as_ref()
        .map(|c| c.slots.clone())
        .unwrap_or_default();
    let first_slot_uid = state.context.first_slot_uid;
    let disks = state
        .context
        .catalyst
        .as_ref()
        .map_or(0, |c| c.disk_count.max(0));
    let card_count = managers
        .card
        .hand()
        .iter()
        .filter(|card| {
            !matches!(card.card_type, Some(1 | 3))
                && card
                    .skill_id
                    .is_some_and(|id| id > 0 && !managers.catalog().skill_is_big(id))
        })
        .count()
        .min(i32::MAX as usize) as i32;
    let mut ops = Vec::new();
    // Planned indices are reserved across item copies because effects in this
    // group are committed after this snapshot is inspected.
    let mut reserved_precast_indices = std::collections::BTreeSet::new();
    // Reserve only stacks present at this snapshot. Several grenades cannot
    // spend the same energy before their queued commands execute.
    let mut shock_energy: std::collections::BTreeMap<i64, i32> = live_main(managers, pool)
        .into_iter()
        .map(|uid| (uid, managers.buff.buff_id_amount(uid, 90071)))
        .collect();

    for item in items {
        if item.item_id == 8230039 {
            let mut choices: Vec<usize> = managers
                .card
                .hand()
                .iter()
                .enumerate()
                .filter_map(|(index, card)| {
                    let owner = card.uid.unwrap_or_default();
                    let eligible = !card.temp_card.unwrap_or_default()
                        && matches!(card.card_type, None | Some(0 | 2))
                        && card.skill_id.is_some_and(|skill| {
                            skill > 0 && !managers.catalog().skill_is_big(skill)
                        })
                        && pool
                            .attacker_main
                            .iter()
                            .any(|actor| actor.uid == owner && managers.hp.current(owner) > 0)
                        && !reserved_precast_indices.contains(&index);
                    eligible.then_some(index)
                })
                .collect();
            let mut chosen = Vec::new();
            for _ in 0..3 {
                let Some(index) = determinism.lua_random_index(choices.len()) else {
                    break;
                };
                let chosen_index = choices.swap_remove(index);
                reserved_precast_indices.insert(chosen_index);
                chosen.push(chosen_index);
            }
            if !chosen.is_empty() {
                ops.push(RuleOp::Command(BattleCommand::Card(
                    CardCommand::MarkTemporary(CardMarkTemporary {
                        origin: ORIGIN,
                        indices: chosen,
                        team_type: 1,
                        config_effect: 50023,
                    }),
                )));
            }
            continue;
        }
        if item.item_id == 8110805 {
            for (uid, remaining) in &mut shock_energy {
                if *remaining <= 0 {
                    continue;
                }
                *remaining -= 1;
                ops.push(RuleOp::Command(BattleCommand::Buff(
                    BuffCommand::ConsumeCoalesced(BuffConsume {
                        origin: ORIGIN,
                        target_uid: *uid,
                        selector: BuffSelector::ExactId(90071),
                        amount: 1,
                        depleted: DepletedBuff::Remove,
                    }),
                )));
                let cast: SkillInvocation = SkillRequest {
                    source_uid: *uid,
                    skill_id: 811080592,
                }
                .into();
                ops.push(RuleOp::Skill(cast));
            }
            continue;
        }
        if item.item_id == 8120821 && round % 2 == 0 {
            for enemy in &pool.defender_main {
                if managers.hp.current(enemy.uid) > 0 {
                    ops.push(RuleOp::Command(BattleCommand::Buff(
                        BuffCommand::ConsumeCoalesced(BuffConsume {
                            origin: ORIGIN,
                            target_uid: enemy.uid,
                            selector: BuffSelector::ExactId(90071),
                            amount: 2,
                            depleted: DepletedBuff::Remove,
                        }),
                    )));
                }
            }
            continue;
        }
        // Random recipients use the battle's replayed RNG and are restricted
        // to living characters actually on the field.
        if matches!(item.item_id, 8120034 | 8120104 | 8130203) {
            let allies = live_main(managers, pool);
            if let Some(uid) = determinism
                .lua_random_index(allies.len())
                .map(|index| allies[index])
            {
                match item.item_id {
                    8120034 => ops.push(give(uid, 90011, 1)),
                    8130203 => ops.push(give(uid, 90091, 10)),
                    8120104 => ops.push(RuleOp::Command(BattleCommand::ExPoint(
                        ExPointCommand::Change(ExPointChange {
                            origin: ORIGIN,
                            source_uid: uid,
                            target_uid: uid,
                            delta: 1,
                            config_effect: 0,
                            effect_type: EffectType::Expointchange as i32,
                        }),
                    ))),
                    _ => unreachable!(),
                }
            }
            continue;
        }
        if item.item_id == 8220044 && (round - 1) % 3 == 0 {
            // This carrier is a verified +1 AP buff with one-round expiry at
            // stage 303. A single living main carries each glove instance.
            if let Some(uid) = live_main(managers, pool).first().copied() {
                ops.push(give(uid, 832071801, 1));
            }
            continue;
        }
        if item.item_id == 8230004 {
            // Range-based probability bonus needs the frozen layout geometry.
            let chance = item.proc_chance(100_i32.saturating_add(item.count2.saturating_mul(10)));
            if determinism.roll_permille(chance) {
                ops.extend(catalyst::power_change(managers, 2));
            }
            continue;
        }
        if item.item_id == 8220814 {
            for uid in live_main(managers, pool) {
                ops.push(give(uid, 90071, 5));
            }
            let chance = item.proc_chance(1000_i32.saturating_sub(item.count2.saturating_mul(10)));
            if determinism.roll_permille(chance) {
                ops.extend(catalyst::power_change(managers, -1));
            }
            continue;
        }
        if item.item_id == 8320210 {
            let chance = item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            // Range-dependent bonus is not guessed without a layout snapshot.
            for uid in live_main(managers, pool) {
                if determinism.roll_permille(chance) {
                    ops.push(moxie(uid, 1));
                }
            }
            continue;
        }
        if item.item_id == 8330020 {
            if let Some(source) = live_main(managers, pool).first().copied() {
                for enemy in &pool.defender_main {
                    if managers.hp.current(enemy.uid) <= 0 {
                        continue;
                    }
                    let mut all = vec![90062, 90232, 90152, 90222, 90162];
                    for _ in 0..2 {
                        let missing: Vec<_> = all
                            .iter()
                            .copied()
                            .filter(|id| managers.buff.buff_id_amount(enemy.uid, *id) <= 0)
                            .collect();
                        let candidates = if missing.is_empty() { &all } else { &missing };
                        let Some(index) = determinism.lua_random_index(candidates.len()) else {
                            break;
                        };
                        let buff = candidates[index];
                        all.retain(|id| *id != buff);
                        ops.push(give_from(source, enemy.uid, buff, 1));
                    }
                }
            }
            continue;
        }
        if item.item_id == 8220806 {
            for (slot, delta) in [(1_i32, 1_i32), (4, -1)] {
                let uid = if formation_slots.is_empty() {
                    (slot == 1).then_some(first_slot_uid).flatten().or_else(|| {
                        pool.attacker_main
                            .iter()
                            .find(|e| e.position == slot)
                            .map(|e| e.uid)
                    })
                } else {
                    formation_slots
                        .iter()
                        .find_map(|(index, uid)| (*index == slot).then_some(*uid))
                };
                if let Some(entity) = pool
                    .attacker_main
                    .iter()
                    .find(|e| Some(e.uid) == uid && managers.hp.current(e.uid) > 0)
                {
                    ops.push(moxie(entity.uid, delta));
                    if delta > 0 {
                        ops.push(give(entity.uid, 90071, 5));
                    } else {
                        ops.push(RuleOp::Command(BattleCommand::Buff(
                            BuffCommand::ConsumeCoalesced(BuffConsume {
                                origin: ORIGIN,
                                target_uid: entity.uid,
                                selector: BuffSelector::ExactId(90071),
                                amount: 5,
                                depleted: DepletedBuff::Remove,
                            }),
                        )));
                    }
                }
            }
            continue;
        }
        let bleeding_enemy = pool.defender_main.iter().any(|e| {
            managers.hp.current(e.uid) > 0
                && [90061, 90062, 90063, 90161, 90162, 90163, 5051, 5052, 5053]
                    .iter()
                    .any(|id| managers.buff.buff_id_amount(e.uid, *id) > 0)
        });
        for uid in live_main(managers, pool) {
            match item.item_id {
                8210818 if (round - 1) % 3 == 0 && disks > 0 => ops.push(give(uid, 90071, disks)),
                8230050 => {
                    let hp = managers.hp.get(uid);
                    if hp.max > 0
                        && i64::from(hp.current) * 100 < i64::from(hp.max) * 30
                        && managers.rouge.active.as_mut().is_some_and(|state| {
                            state.turn_effects.bandage_used.insert((item.uid, uid))
                        })
                    {
                        let amount =
                            ((i64::from(hp.max) * 150) / 1000).min(i64::from(i32::MAX)) as i32;
                        if amount > 0 {
                            ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::Heal(
                                HpHeal {
                                    origin: ORIGIN,
                                    source_uid: uid,
                                    target_uid: uid,
                                    amount,
                                    config_effect: 0,
                                    kind: HpHealKind::Normal,
                                },
                            ))));
                        }
                    }
                    // A stable first dispellable negative is a local policy.
                    ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Dispel(
                        BuffDispel {
                            origin: ORIGIN,
                            target_uid: uid,
                            statuses: vec![BuffStatus::NegativeStatus],
                            excluded_ids_or_types: Vec::new(),
                            count: 1,
                        },
                    ))));
                }
                8220820 => ops.push(give(uid, 30740172, 1)),
                // Current Reflect carrier reflects 100% of own ATK; the wiki
                // does not specify its exact coefficient or consumption.
                8310114 => ops.push(give(uid, 2130005, 1)),
                8330715 => {
                    let stacks = managers.buff.buff_id_amount(uid, 90051).clamp(0, 50);
                    for _ in 0..stacks {
                        ops.push(give(uid, 30750131, 1));
                        if let Some(index) = determinism.lua_random_index(9) {
                            ops.push(give(uid, 91130031 + index as i32, 1));
                        }
                    }
                }
                8220036 => {
                    ops.push(give(uid, 90241, if bleeding_enemy { 2 } else { 1 }));
                    ops.push(give(uid, 90021, 1));
                }
                8220051
                    if managers.eureka.get(uid, EUREKA_RESOURCE_ID).max > 0
                        && managers.eureka.get(uid, EUREKA_RESOURCE_ID).current <= 2 =>
                {
                    ops.push(eureka(uid, 1))
                }
                8220052 if managers.ex_point.get(uid) >= 3 => {
                    ops.push(give(uid, 302, 1));
                    ops.push(give(uid, 301, 1));
                }
                8130217 => ops.push(give(uid, 90091, 3)),
                8130041 if managers.buff.buff_id_amount(uid, 90011) >= 4 => {
                    ops.push(give(uid, 90011, 2))
                }
                8230028 => ops.push(give(uid, 90011, 2)),
                8330026 => ops.push(give(uid, 90011, 4)),
                8330710 => ops.push(give(uid, 90051, 5)),
                8130803 => ops.push(give(uid, 90071, 1)),
                8130804 => ops.push(give(uid, 90071, 2)),
                8120821 if round % 2 == 1 => ops.push(give(uid, 90071, 2)),
                8110307 | 8210308 => {
                    let divisor = if item.item_id == 8110307 { 3 } else { 2 };
                    let stacks = (card_count / divisor)
                        .saturating_mul(2_i32.saturating_add(item.count2.max(0)))
                        .min(100);
                    if stacks > 0 {
                        ops.push(give(uid, 90301, stacks));
                    }
                }
                _ => {}
            }
        }
    }
    ops
}

/// At an allied action's actual completion, never at queue construction or
/// damage preview. Each successful hit's crit count contributes separately.
pub(super) fn action(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    action: &ActionEvent,
    aid: bool,
    determinism: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let special_card = action.card_index > 0
        && managers.card.played().iter().any(|played| {
            played.card_index == action.card_index
                && played.caster_uid == action.source_uid
                && played.skill_id == action.skill_id
                && played.card.card_type == Some(1)
        });
    let mut special_ops = Vec::new();
    if special_card {
        if let Some(context) = managers.rouge.context() {
            for item in &context.artefacts {
                if item.item_id == 8350043 {
                    for uid in live_main(managers, pool) {
                        let chosen = permanent_buffs_with_pending(
                            managers,
                            uid,
                            3,
                            determinism,
                            &special_ops,
                        );
                        special_ops.extend(chosen);
                    }
                }
            }
        }
    }
    if !pool
        .attacker_main
        .iter()
        .any(|entity| entity.uid == action.source_uid)
        || managers.hp.current(action.source_uid) <= 0
    {
        // Actual Rouge special cards belong to the attacker-side scene UID 0,
        // not a character. Their result must still reach all living allies.
        return special_ops;
    }
    let selected = action.card_index > 0
        && !managers.catalog().skill_is_big(action.skill_id)
        && managers.card.played().iter().any(|played| {
            played.card_index == action.card_index
                && played.caster_uid == action.source_uid
                && played.skill_id == action.skill_id
                && (matches!(played.card.card_type, None | Some(0 | 2))
                    || (played.card.card_type == Some(6)
                        && played.card.temp_card == Some(true)
                        && managers.card.is_hero_precast_play(action.card_index)))
        });
    // Commit the round ledger first, then release its mutable borrow before
    // resolving effects against the rest of the battle managers.
    let support_incantation =
        aid && action.card_index > 0 && !managers.catalog().skill_is_big(action.skill_id);
    let (first_action, support_before, support_after, disks, support_owners, items) = {
        let Some(state) = managers.rouge.active.as_mut() else {
            return vec![];
        };
        if selected {
            state.turn_effects.cast_actors.insert(action.source_uid);
        }
        let first = state.turn_effects.acted_actors.insert(action.source_uid);
        let support_before = state
            .turn_effects
            .support_casts
            .get(&action.source_uid)
            .copied()
            .unwrap_or(0);
        let support_after = if support_incantation {
            support_before.saturating_add(1)
        } else {
            support_before
        };
        if support_incantation {
            state
                .turn_effects
                .support_casts
                .insert(action.source_uid, support_after);
        }
        let disks = state
            .context
            .catalyst
            .as_ref()
            .map_or(0, |c| c.disk_count.max(0));
        let support_owners: std::collections::BTreeSet<i64> = state
            .context
            .support_skills
            .iter()
            .filter(|support| support.hero_id > 0)
            .map(|support| support.owner_uid)
            .collect();
        (
            first,
            support_before,
            support_after,
            disks,
            support_owners,
            state.context.artefacts.clone(),
        )
    };
    let high_rank = action.card_index > 0
        && matches!(action.rank, 2 | 3)
        && !managers.catalog().skill_is_big(action.skill_id);
    let extra = crate::engine::skill::action::performs_extra_action(
        action.card_index,
        action.mode,
        action.extra_skill_kind,
    );
    let mut ops = special_ops;
    if action.card_index > 0
        && managers.catalog().skill_is_big(action.skill_id)
        && items.iter().any(|item| item.item_id == 8210205)
    {
        let refund = managers.buff.buff_id_amount(action.source_uid, 821020511);
        if refund > 0 {
            ops.push(RuleOp::Command(BattleCommand::Buff(
                BuffCommand::ConsumeCoalesced(BuffConsume {
                    origin: ORIGIN,
                    target_uid: action.source_uid,
                    selector: BuffSelector::ExactId(821020511),
                    amount: refund,
                    depleted: DepletedBuff::Remove,
                }),
            )));
            ops.push(give(action.source_uid, 90091, refund));
        }
    }
    let active_support_count = support_owners
        .into_iter()
        .filter(|uid| {
            pool.attacker_main.iter().any(|actor| actor.uid == *uid)
                && managers.hp.current(*uid) > 0
        })
        .count();
    let mut power_gifts = Vec::new();
    for item in &items {
        match item.item_id {
            8320709 => {
                // Current completed-action hook is after the triggering cast;
                // first/extra procs cannot protect or boost that same action.
                if first_action
                    && determinism.roll_permille(
                        item.proc_chance(600_i32.saturating_add(item.count2.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 90021, 2));
                    ops.push(give(action.source_uid, 91210030, 2));
                }
                if extra
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count4.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 90021, 1));
                    ops.push(give(action.source_uid, 91210030, 1));
                }
            }
            8220215 if support_incantation => {
                // Trigger once on crossing the threshold this round. The
                // below-layout flag is frozen by the server at battle entry.
                let threshold = 4_i32
                    .saturating_sub(item.count2.max(0))
                    .saturating_sub(i32::from(item.only_support_below))
                    .max(1);
                if support_before < threshold && support_after >= threshold {
                    for uid in live_main(managers, pool) {
                        ops.push(moxie(uid, 1));
                    }
                }
            }
            8120035
                if action.card_index > 0
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && matches!(
                        spell_show_tag(managers, action.skill_id),
                        Some(4 | 5 | 6 | 9)
                    ) =>
            {
                ops.push(give(action.source_uid, 30750131, 1));
                ops.push(give(action.source_uid, 30750132, 1));
            }
            8210032
                if action.card_index > 0
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && matches!(spell_show_tag(managers, action.skill_id), Some(4 | 5 | 9)) =>
            {
                for enemy in pool
                    .defender_main
                    .iter()
                    .filter(|e| managers.hp.current(e.uid) > 0)
                {
                    ops.push(give_from(action.source_uid, enemy.uid, 4150001, 10));
                    ops.push(give_from(action.source_uid, enemy.uid, 30740171, 1));
                }
            }
            8210819
                if action.card_index > 0
                    && action.is_attack
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && managers.buff.buff_id_amount(action.source_uid, 90071) > 0 =>
            {
                // The target list is all the committed action exposes here.
                // Gash is applied after this cast, a documented local timing
                // approximation; the current cast cannot benefit from it.
                let mut targets = action.target_uids.clone();
                if targets.is_empty() && action.target_uid != 0 {
                    targets.push(action.target_uid);
                }
                targets.sort_unstable();
                targets.dedup();
                for target in targets {
                    if pool.team_type(target) == Some(2) && managers.hp.current(target) > 0 {
                        ops.push(give_from(action.source_uid, target, 90062, 1));
                    }
                }
                if determinism.roll_permille(
                    item.proc_chance(1000_i32.saturating_sub(item.count2.saturating_mul(10))),
                ) {
                    ops.push(RuleOp::Command(BattleCommand::Buff(
                        BuffCommand::ConsumeCoalesced(BuffConsume {
                            origin: ORIGIN,
                            target_uid: action.source_uid,
                            selector: BuffSelector::ExactId(90071),
                            amount: 1,
                            depleted: DepletedBuff::Remove,
                        }),
                    )));
                }
            }
            8120703 => {
                // First and extra effects are currently applied after the
                // completed action; an extra action cannot use its own proc.
                if first_action {
                    ops.push(give(action.source_uid, 90051, 1));
                }
                if extra
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 90051, 1));
                }
            }
            8120704 => {
                if first_action {
                    ops.push(give(action.source_uid, 30750132, 1));
                    ops.push(give(action.source_uid, 30750131, 1));
                }
                if extra
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 30750131, 1));
                }
            }
            8120705 => {
                if first_action
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 91210030, 2));
                }
                if extra
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count4.saturating_mul(10))),
                    )
                {
                    ops.push(give(action.source_uid, 91210030, 2));
                }
            }
            8330005 => {
                // The layout is frozen on entering this fight; inlaid disks are
                // not independently placed objects and therefore do not count.
                let chance = item.proc_chance(
                    50_i32
                        .saturating_add(item.count2.saturating_mul(50))
                        .saturating_add(item.artefacts_in_range_count.max(0).saturating_mul(50)),
                );
                if determinism.roll_permille(chance) {
                    power_gifts.push(2);
                }
            }
            8130115
                if action.card_index > 0
                    && action.is_attack
                    && managers.catalog().skill_is_big(action.skill_id)
                    && action.damage_amount > 0 =>
            {
                // The action event exposes aggregate damage and target UIDs,
                // not a per-target hit list. Applying to its living listed
                // targets after a damaging action is a local approximation.
                let mut targets = action.target_uids.clone();
                if targets.is_empty() && action.target_uid != 0 {
                    targets.push(action.target_uid);
                }
                targets.sort_unstable();
                targets.dedup();
                for target in targets {
                    if pool.team_type(target) != Some(2) || managers.hp.current(target) <= 0 {
                        continue;
                    }
                    ops.push(give_from(action.source_uid, target, 90222, 1));
                    if aid {
                        ops.push(give_from(action.source_uid, target, 90232, 1));
                    }
                }
            }
            8130107 if support_incantation => {
                // Extra attempts count valid frozen support links on living
                // active owners; reserves are an explicit local interpretation.
                for _ in 0..=active_support_count {
                    if determinism.roll_permille(item.proc_chance(100)) {
                        power_gifts.push(10);
                    }
                }
            }
            8120110
                if aid
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && determinism.roll_permille(
                        item.proc_chance(600_i32.saturating_add(item.count2.saturating_mul(10))),
                    ) =>
            {
                // The common dispel operation uses a stable eligible order.
                for uid in live_main(managers, pool) {
                    ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Dispel(
                        BuffDispel {
                            origin: ORIGIN,
                            target_uid: uid,
                            statuses: vec![BuffStatus::NegativeStatus],
                            excluded_ids_or_types: Vec::new(),
                            count: 2,
                        },
                    ))));
                }
            }
            8320809 => ops.push(give(action.source_uid, 90071, 1)),
            8330112 if aid && !managers.catalog().skill_is_big(action.skill_id) => {
                for uid in live_main(managers, pool) {
                    // Stable first dispellable negative status is a local policy;
                    // the generic dispel command cannot select a random UID.
                    ops.push(RuleOp::Command(BattleCommand::Buff(BuffCommand::Dispel(
                        BuffDispel {
                            origin: ORIGIN,
                            target_uid: uid,
                            statuses: vec![BuffStatus::NegativeStatus],
                            excluded_ids_or_types: Vec::new(),
                            count: 1,
                        },
                    ))));
                    let selected = permanent_buffs_with_pending(
                        managers,
                        uid,
                        1 + item.count2.max(0) as usize,
                        determinism,
                        &ops,
                    );
                    ops.extend(selected);
                }
            }
            8230714 => {
                if action.card_index > 0
                    && action.is_attack
                    && !managers.catalog().skill_is_big(action.skill_id)
                {
                    ops.push(give(action.source_uid, 90001, 2));
                    ops.push(give(action.source_uid, 90002, 2));
                }
                if extra {
                    let chance =
                        item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
                    if determinism.roll_permille(chance) {
                        ops.push(give(action.source_uid, 90001, 2));
                    }
                    if determinism.roll_permille(chance) {
                        ops.push(give(action.source_uid, 90002, 2));
                    }
                }
            }
            8210818 if extra && disks > 0 => {
                ops.push(give(
                    action.source_uid,
                    90071,
                    disks.saturating_mul(1 + item.count2.max(0)),
                ));
            }
            8310815 if disks > 0 => {
                let chance =
                    item.proc_chance(600_i32.saturating_add(item.count2.saturating_mul(10)));
                let successes = (0..disks.min(100))
                    .filter(|_| determinism.roll_permille(chance))
                    .count() as i32;
                if successes > 0 {
                    ops.push(give(action.source_uid, 90071, successes.saturating_mul(2)));
                }
            }
            8220106
                if action.card_index > 0 && !managers.catalog().skill_is_big(action.skill_id) =>
            {
                let enemies: Vec<_> = pool
                    .defender_main
                    .iter()
                    .filter(|e| managers.hp.current(e.uid) > 0)
                    .map(|e| e.uid)
                    .collect();
                if let Some(index) = determinism.lua_random_index(enemies.len()) {
                    ops.push(give_from(
                        action.source_uid,
                        enemies[index],
                        4150001,
                        5_i32.saturating_add(item.count2.max(0)),
                    ));
                }
                if aid {
                    ops.push(give(action.source_uid, 4150002, 5));
                }
            }
            8330211
                if action.card_index > 0 && managers.catalog().skill_is_big(action.skill_id) =>
            {
                for uid in live_main(managers, pool) {
                    ops.push(give(uid, 4150002, 15));
                }
            }
            8120047 if high_rank => {
                ops.push(give(action.source_uid, 90031, 2));
                ops.push(give(action.source_uid, 90032, 2));
            }
            8220807
                if high_rank
                    && determinism.roll_permille(
                        item.proc_chance(600_i32.saturating_add(item.count2.saturating_mul(10))),
                    ) =>
            {
                ops.push(give(action.source_uid, 90071, 5));
                let selected =
                    permanent_buffs_with_pending(managers, action.source_uid, 2, determinism, &ops);
                ops.extend(selected);
            }
            8320811 if high_rank => {
                ops.push(give(action.source_uid, 90071, 2));
                let selected =
                    permanent_buffs_with_pending(managers, action.source_uid, 2, determinism, &ops);
                ops.extend(selected);
            }
            8110213 => ops.push(give(action.source_uid, 90091, 5)),
            8130303 if high_rank => ops.push(give(action.source_uid, 90301, 4)),
            8230304 if high_rank => ops.push(give(action.source_uid, 90301, 6)),
            8130103
                if aid
                    && action.card_index > 0
                    && !managers.catalog().skill_is_big(action.skill_id) =>
            {
                for uid in live_main(managers, pool) {
                    ops.push(give(uid, 90091, 2));
                }
            }
            8310309
                if high_rank
                    && determinism.roll_permille(
                        item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10))),
                    ) =>
            {
                for uid in live_main(managers, pool) {
                    ops.push(give(uid, 90301, 4));
                }
            }
            8210314 | 8310315 if action.crit_count > 0 => {
                let per_crit = if item.item_id == 8210314 {
                    1_i32
                } else {
                    2_i32
                };
                ops.push(give(
                    action.source_uid,
                    90011,
                    action.crit_count.saturating_mul(per_crit).min(50),
                ));
            }
            _ => {}
        }
    }
    for amount in power_gifts {
        ops.extend(catalyst::power_change(managers, amount));
    }
    ops
}

/// Apply the boxing set after the attacker-side round settlement. Multiple
/// copies add normally; the same round cannot be committed twice.
pub(super) fn round_end(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    if round <= 0
        || state
            .turn_effects
            .last_end_round
            .is_some_and(|seen| seen >= round)
    {
        return vec![];
    }
    state.turn_effects.last_end_round = Some(round);
    let items = state.context.artefacts.clone();
    let cast_actors = state.turn_effects.cast_actors.clone();
    let seed = state.context.entry.as_ref().map_or(0, |entry| entry.seed);
    let power = state.catalyst.as_ref().map_or(0, |c| c.resources.power);
    let mut ops = Vec::new();

    for item in items {
        if item.item_id == 8220515 {
            for uid in live_main(managers, pool) {
                if !cast_actors.contains(&uid) {
                    ops.push(give(
                        uid,
                        90082,
                        1_i32
                            .saturating_add(item.count2.max(0))
                            .saturating_add(i32::from(item.non_resonance_in_range)),
                    ));
                }
            }
        }
        if item.item_id == 8330111 {
            // Stable local seed, as the round-end hook has no shared RNG.
            let amount = 1_usize
                .saturating_add(item.support_below_count.max(0) as usize)
                .saturating_add(if power >= 50 {
                    1 + item.count2.max(0) as usize
                } else {
                    0
                });
            for uid in live_main(managers, pool) {
                let mut rng = RoundDeterminism::with_seed(
                    seed ^ (item.uid as u64).rotate_left(17)
                        ^ (uid as u64).rotate_left(31)
                        ^ (round as u64),
                );
                let selected = permanent_buffs_with_pending(managers, uid, amount, &mut rng, &ops);
                ops.extend(selected);
            }
        }
        if item.item_id == 8220051 {
            for uid in live_main(managers, pool) {
                if managers.eureka.get(uid, EUREKA_RESOURCE_ID).current >= 4 {
                    ops.push(give(uid, 301, 1));
                }
            }
        }
        if matches!(item.item_id, 8230038 | 8230040 | 8330023) {
            for uid in pool
                .attacker_main
                .iter()
                .chain(pool.defender_main.iter())
                .filter_map(|entity| (managers.hp.current(entity.uid) > 0).then_some(entity.uid))
            {
                if matches!(item.item_id, 8230038 | 8330023) {
                    ops.push(moxie(uid, 1));
                }
                if matches!(item.item_id, 8230040 | 8330023) {
                    ops.push(eureka(uid, 1));
                }
            }
        }
        let stacks = match item.item_id {
            8230027 => 1,
            8330026 => 2,
            _ => 0,
        };
        if stacks > 0 {
            for uid in live_main(managers, pool) {
                ops.push(give(uid, 90001, stacks));
            }
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        manager::card::CardManager,
        runtime::{determinism::RoundDeterminism, drain},
        skill::{effect::SkillEffectCatalog, target::TargetContext},
    };
    use sonettobuf::{CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

    fn setup(items: &[(i32, i32)]) -> (BattleManagers, TargetPool) {
        crate::test_support::init_config();
        let entity = |uid, team| FightEntityInfo {
            uid: Some(uid),
            team_type: Some(team),
            position: Some(match uid {
                10 => 1,
                11 => 2,
                12 => -1,
                _ => 1,
            }),
            current_hp: Some(1000),
            attr: Some(HeroAttribute {
                hp: Some(1000),
                attack: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![entity(10, 1), entity(11, 1)],
                sub_entitys: vec![entity(12, 1)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![entity(-1, 2)],
                ..Default::default()
            }),
            version: Some(7),
            ..Default::default()
        };
        let mut managers = BattleManagers::seeded(&fight);
        configure(
            &mut managers,
            RougeContext {
                artefacts: items
                    .iter()
                    .enumerate()
                    .map(|(index, (item_id, count2))| Artefact {
                        uid: index as i64 + 100,
                        item_id: *item_id,
                        count2: *count2,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
        (managers, TargetPool::from_fight(&fight))
    }

    fn execute(managers: &mut BattleManagers, pool: &TargetPool, ops: Vec<RuleOp>) {
        let mut rng = RoundDeterminism::with_seed(42);
        drain::run_command_group(
            managers,
            pool,
            &SkillEffectCatalog::default(),
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            ops,
        )
        .unwrap();
    }

    #[test]
    fn fading_statuses_apply_through_real_round_lifecycle() {
        let (mut managers, pool) = setup(&[]);
        let catalog = SkillEffectCatalog::default();
        let mut rng = RoundDeterminism::with_seed(42);
        execute(
            &mut managers,
            &pool,
            vec![
                give(10, 90001, 2),
                give(10, 90002, 2),
                give(10, 90011, 2),
                give(10, 90012, 2),
                give(10, 90041, 2),
                give(10, 90042, 2),
                give(10, 90051, 2),
                give(10, 90052, 2),
            ],
        );
        crate::engine::runtime::schedule::run_attacker_round_end(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            managers.buff.buff_id_amount(10, 90001),
            0,
            "the official round end must consume the fading amount before expiry"
        );

        for id in [90011, 90041, 90051, 90002, 90012, 90042, 90052] {
            assert_eq!(
                managers.buff.buff_id_amount(10, id),
                0,
                "expired end-stage status {id}"
            );
        }
        execute(
            &mut managers,
            &pool,
            vec![
                give(10, 90031, 2),
                give(10, 90032, 2),
                give(10, 90021, 2),
                give(10, 90022, 2),
            ],
        );
        crate::engine::runtime::schedule::run_round_start_split(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            TargetContext {
                current_round: 2,
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert_eq!(
            managers.buff.buff_id_amount(10, 90031),
            0,
            "the official round start must consume the fading amount before expiry"
        );
        for id in [90021, 90022, 90032] {
            assert_eq!(
                managers.buff.buff_id_amount(10, id),
                0,
                "expired start-stage status {id}"
            );
        }
        assert!(fade_before_duration(&managers, 103, &[(10, 0)]).is_empty());
    }

    #[test]
    fn round_effects_use_real_buffs_hand_and_alive_active_allies_once_per_round() {
        let (mut managers, pool) = setup(&[
            (8110307, 0),
            (8210308, 1),
            (8130041, 0),
            (8230028, 0),
            (8230027, 0),
            (8330026, 0),
        ]);
        let cards = (0..5)
            .map(|_| CardInfo {
                uid: Some(10),
                skill_id: Some(30030111),
                ..Default::default()
            })
            .chain(std::iter::once(CardInfo {
                uid: Some(10),
                skill_id: Some(30030131),
                card_type: Some(1),
                ..Default::default()
            }))
            .collect();
        managers.card = CardManager::new(cards);
        execute(&mut managers, &pool, vec![give(10, 90011, 4)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 12);
        assert_eq!(managers.buff.buff_id_amount(11, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 8);
        assert_eq!(
            managers.buff.buff_id_amount(12, 90011),
            0,
            "reserve heroes are not on the field"
        );
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90001), 3);
        assert_eq!(managers.buff.buff_id_amount(11, 90001), 3);
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }

    #[test]
    fn actual_action_results_award_only_the_actor_and_stack_by_rank_and_crits() {
        let (mut managers, pool) = setup(&[
            (8110213, 0),
            (8130303, 0),
            (8230304, 0),
            (8210314, 0),
            (8310315, 0),
        ]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            crit_count: 2,
            is_attack: true,
            ..Default::default()
        };
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90091), 5);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 10);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(11, 90091), 0);
        let weak = ActionEvent {
            rank: 1,
            crit_count: 0,
            ..event
        };
        let ops = action(&mut managers, &pool, &weak, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90091), 10);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 10);
    }

    #[test]
    fn support_scarf_and_forging_hammer_use_actual_casts_and_replayed_probability() {
        let (mut managers, pool) = setup(&[(8130103, 0), (8310309, 0)]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            ..Default::default()
        };
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 999]);
        let ops = action(&mut managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90091), 2);
            assert_eq!(managers.buff.buff_id_amount(uid, 90301), 4);
        }
        let ops = action(&mut managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90091), 4);
            assert_eq!(managers.buff.buff_id_amount(uid, 90301), 4);
        }
        let no_card = ActionEvent {
            card_index: 0,
            ..event
        };
        assert!(action(&mut managers, &pool, &no_card, true, &mut rng).is_empty());
    }

    #[test]
    fn slingshot_lucky_is_applied_before_only_the_selected_high_rank_card() {
        let (mut managers, pool) = setup(&[(8210314, 0), (8310315, 0)]);
        assert!(before_card(&managers, &pool, 10, 30030112, 0, false).is_empty());
        assert!(before_card(&managers, &pool, 10, 30030111, 1, false).is_empty());
        assert!(before_card(&managers, &pool, 10, 30030112, 1, true).is_empty());
        let ops = before_card(&managers, &pool, 10, 30030112, 1, false);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(11, 90011), 0);
    }

    #[test]
    fn clockwork_and_random_round_gifts_stay_on_living_main_characters() {
        let (mut managers, pool) = setup(&[
            (8120034, 0),
            (8120104, 0),
            (8130203, 0),
            (8130803, 0),
            (8130804, 0),
        ]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 3);
        assert_eq!(managers.buff.buff_id_amount(11, 90071), 3);
        assert_eq!(managers.buff.buff_id_amount(12, 90071), 0);
        assert_eq!(
            managers.buff.buff_id_amount(10, 90011) + managers.buff.buff_id_amount(11, 90011),
            1
        );
        assert_eq!(
            managers.buff.buff_id_amount(10, 90091) + managers.buff.buff_id_amount(11, 90091),
            10
        );
        assert_eq!(managers.ex_point.get(10) + managers.ex_point.get(11), 1);
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
    }

    #[test]
    fn catalyst_hand_limit_stacks_with_actual_placed_powder_boxes() {
        let (mut managers, _) = setup(&[]);
        for (style_id, expected) in [(1, 3), (4, 5), (5, 6), (6, 6)] {
            configure(
                &mut managers,
                RougeContext {
                    catalyst: Some(Catalyst {
                        style_id,
                        ..Default::default()
                    }),
                    artefacts: vec![
                        Artefact {
                            uid: 1,
                            item_id: 8230305,
                            ..Default::default()
                        },
                        Artefact {
                            uid: 2,
                            item_id: 8330306,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            );
            assert_eq!(hand_limit_bonus(&managers), expected);
        }
    }

    #[test]
    fn cardboard_hat_grants_odd_round_dynamo_and_consumes_only_enemy_dynamo_on_even_rounds() {
        let (mut managers, pool) = setup(&[(8120821, 0), (8120821, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 4);
        execute(&mut managers, &pool, vec![give(-1, 90071, 3)]);
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(-1, 90071), 0);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 4);
        assert!(round_start(&mut managers, &pool, 2, &mut rng).is_empty());
    }
    #[test]
    fn new_round_artefacts_check_actor_status_and_living_enemies() {
        let (mut managers, pool) = setup(&[(8220036, 0), (8220052, 0), (8220806, 0), (8230038, 0)]);
        managers.ex_point.set(10, 10, 3, 0);
        execute(&mut managers, &pool, vec![give(-1, 90061, 1)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90241), 2);
        assert_eq!(managers.buff.buff_id_amount(11, 90021), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 302), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 301), 1);
        assert_eq!(managers.buff.buff_id_amount(11, 301), 0);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 5);
        assert_eq!(managers.ex_point.get(10), 4);
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.ex_point.get(10), 5);
        assert_eq!(managers.ex_point.get(-1), 1);
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }

    #[test]
    fn high_rank_artefacts_apply_fading_regeneration_and_prioritize_new_permanent_buffs() {
        let (mut managers, pool) = setup(&[(8120047, 0), (8320811, 0)]);
        execute(
            &mut managers,
            &pool,
            vec![
                give(10, 90001, 1),
                give(10, 90011, 1),
                give(10, 90041, 1),
                give(10, 90021, 1),
            ],
        );
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            ..Default::default()
        };
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        let queued_permanent: Vec<(i32, i32)> = ops
            .iter()
            .filter_map(|op| match op {
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(grant)))
                    if grant.target_uid == 10
                        && [90001, 90011, 90041, 90021, 90031, 90281].contains(&grant.buff_id) =>
                {
                    Some((grant.buff_id, grant.amount.unwrap_or_default()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(queued_permanent.len(), 3);
        assert_eq!(queued_permanent[0], (90031, 2));
        assert_eq!(
            queued_permanent[1],
            (90281, 1),
            "the pending regeneration is already owned, so the other missing state wins first"
        );
        execute(&mut managers, &pool, ops);
        let regeneration = managers.buff.buff_id_amount(10, 90031);
        assert!((2..=3).contains(&regeneration));
        assert_eq!(managers.buff.buff_id_amount(10, 90032), 2);
        assert!(managers.buff.buff_id_amount(10, 90281) >= 1);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 2);
        let total: i32 = [90001, 90011, 90041, 90021, 90031, 90281]
            .iter()
            .map(|id| managers.buff.buff_id_amount(10, *id))
            .sum();
        assert_eq!(
            total, 8,
            "two drawn buffs and two regeneration stacks are all retained"
        );
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        // The separate post-deal artefact hook does not advance buff duration.
        assert_eq!(managers.buff.buff_id_amount(10, 90031), regeneration);
        assert_eq!(managers.buff.buff_id_amount(10, 90032), 2);
        let low = ActionEvent { rank: 1, ..event };
        assert!(action(&mut managers, &pool, &low, false, &mut rng).is_empty());
    }
    #[test]
    fn action_and_random_status_effects_apply_to_actual_recipients() {
        let (mut managers, pool) = setup(&[
            (8230714, 0),
            (8220106, 0),
            (8330211, 0),
            (8330020, 0),
            (8330111, 0),
        ]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        let negatives = [90062, 90232, 90152, 90222, 90162]
            .iter()
            .map(|id| managers.buff.buff_id_amount(-1, *id))
            .sum::<i32>();
        assert_eq!(negatives, 2);
        let action1 = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            is_attack: true,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &action1, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90001), 2);
        assert_eq!(managers.buff.buff_id_amount(10, 90002), 2);
        assert_eq!(managers.buff.buff_id_amount(-1, 4150001), 5);
        assert_eq!(managers.buff.buff_id_amount(10, 4150002), 5);
        let ultimate = ActionEvent {
            skill_id: 30030131,
            rank: 0,
            ..action1
        };
        let ops = action(&mut managers, &pool, &ultimate, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 4150002), 20);
        assert_eq!(managers.buff.buff_id_amount(11, 4150002), 15);
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        // Fading is applied by actual duration settlement, not this late hook.
        assert_eq!(managers.buff.buff_id_amount(10, 90001), 2);
        assert_eq!(managers.buff.buff_id_amount(10, 90002), 2);
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }
    #[test]
    fn additional_round_and_support_artefacts_use_living_allies_and_real_buffs() {
        let (mut managers, pool) = setup(&[
            (8220814, 0),
            (8220820, 0),
            (8320210, 50),
            (8320809, 0),
            (8330112, 0),
        ]);
        let mut rng = RoundDeterminism::with_seed(42);
        execute(&mut managers, &pool, vec![give(10, 90152, 1)]);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90071), 5);
            assert_eq!(managers.buff.buff_id_amount(uid, 30740172), 1);
            assert_eq!(managers.ex_point.get(uid), 1);
        }
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 6);
        assert_eq!(managers.buff.buff_id_amount(10, 90152), 0);
        for uid in [10, 11] {
            let total: i32 = [90001, 90011, 90041, 90021, 90031, 90281]
                .iter()
                .map(|id| managers.buff.buff_id_amount(uid, *id))
                .sum();
            assert_eq!(total, 1);
        }
    }
    #[test]
    fn ointment_resolves_before_high_rank_cards_with_a_stable_lowest_ally() {
        use crate::engine::manager::hp::HpLoss;
        let (mut managers, pool) = setup(&[(8230316, 0)]);
        let loss = |uid, amount| {
            RuleOp::Command(BattleCommand::Hp(HpCommand::Lose(HpLoss {
                origin: ORIGIN,
                source_uid: uid,
                target_uid: uid,
                amount,
                config_effect: 0,
                hurt: None,
            })))
        };
        execute(&mut managers, &pool, vec![loss(10, 300), loss(11, 700)]);
        assert!(before_card(&managers, &pool, 10, 30030111, 1, false).is_empty());
        assert!(before_card(&managers, &pool, 10, 30030112, 1, true).is_empty());
        let ops = before_card(&managers, &pool, 10, 30030112, 1, false);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.hp.current(10), 800);
        assert_eq!(managers.hp.current(11), 350);
        assert_eq!(managers.buff.buff_id_amount(10, 302), 1);
        assert_eq!(managers.buff.buff_id_amount(11, 302), 1);
        assert_eq!(managers.hp.current(12), 1000);
    }
    #[test]
    fn pig_chain_applies_corrosion_and_collaboration_pollution_after_damaging_ultimate() {
        let (mut managers, pool) = setup(&[(8130115, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030131,
            card_index: 1,
            rank: 0,
            is_attack: true,
            damage_amount: 10,
            target_uids: vec![-1],
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(-1, 90222), 1);
        assert_eq!(managers.buff.buff_id_amount(-1, 90232), 1);
        let miss = ActionEvent {
            damage_amount: 0,
            ..event
        };
        assert!(action(&mut managers, &pool, &miss, true, &mut rng).is_empty());
    }
    #[test]
    fn fade_only_uses_scheduled_instances_and_skips_non_rouge_battles() {
        let (mut managers, pool) = setup(&[]);
        execute(
            &mut managers,
            &pool,
            vec![give(10, 90001, 3), give(10, 90002, 2)],
        );
        let fading = managers
            .buff
            .active_for(10)
            .find(|b| b.buff_id == Some(90002))
            .unwrap()
            .uid
            .unwrap();
        assert!(fade_before_duration(&managers, 303, &[(10, fading + 1)]).is_empty());
        assert!(fade_before_duration(&managers, 103, &[(10, fading)]).is_empty());
        managers.rouge = RougeRuntime::default();
        assert!(fade_before_duration(&managers, 303, &[(10, fading)]).is_empty());
        assert_eq!(managers.buff.buff_id_amount(10, 90001), 3);
    }
    #[test]
    fn phonograph_uses_frozen_sparse_formation_slots() {
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                catalyst: Some(Catalyst {
                    slots: vec![(4, 11)],
                    ..Default::default()
                }),
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8220806,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        managers.ex_point.set(11, 11, 3, 0);
        execute(&mut managers, &pool, vec![give(11, 90071, 7)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers.ex_point.get(10),
            0,
            "empty slot 1 must not choose first surviving actor"
        );
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 0);
        assert_eq!(managers.ex_point.get(11), 2);
        assert_eq!(managers.buff.buff_id_amount(11, 90071), 2);
    }

    #[test]
    fn corridor_cleanser_does_not_trigger_for_support_ultimate() {
        let (mut managers, pool) = setup(&[(8330112, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ultimate = ActionEvent {
            source_uid: 10,
            skill_id: 30030131,
            card_index: 1,
            ..Default::default()
        };
        assert!(action(&mut managers, &pool, &ultimate, true, &mut rng).is_empty());
        let normal = ActionEvent {
            skill_id: 30030112,
            rank: 2,
            ..ultimate
        };
        let ops = action(&mut managers, &pool, &normal, true, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            let total: i32 = [90001, 90011, 90041, 90021, 90031, 90281]
                .iter()
                .map(|id| managers.buff.buff_id_amount(uid, *id))
                .sum();
            assert_eq!(total, 1);
        }
    }

    #[test]
    fn bandage_heals_each_low_health_actor_once_and_cleanses_each_round() {
        use crate::engine::manager::hp::HpLoss;
        let (mut managers, pool) = setup(&[(8230050, 0)]);
        let loss = |uid, amount| {
            RuleOp::Command(BattleCommand::Hp(HpCommand::Lose(HpLoss {
                origin: ORIGIN,
                source_uid: uid,
                target_uid: uid,
                amount,
                config_effect: 0,
                hurt: None,
            })))
        };
        execute(
            &mut managers,
            &pool,
            vec![loss(10, 750), loss(11, 700), give(10, 90152, 1)],
        );
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.hp.current(10), 400);
        assert_eq!(managers.hp.current(11), 300, "exactly 30% does not trigger");
        assert_eq!(managers.buff.buff_id_amount(10, 90152), 0);
        execute(
            &mut managers,
            &pool,
            vec![loss(10, 200), give(10, 90152, 1)],
        );
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers.hp.current(10),
            200,
            "same actor/item already used its heal"
        );
        assert_eq!(
            managers.buff.buff_id_amount(10, 90152),
            0,
            "cleanse still runs each round"
        );
    }
    #[test]
    fn permanent_draws_share_pending_and_can_exceed_six() {
        let (managers, _) = setup(&[]);
        let mut rng = RoundDeterminism::with_seed(42);
        let mut pending = permanent_buffs_with_pending(&managers, 10, 2, &mut rng, &[]);
        let next = permanent_buffs_with_pending(&managers, 10, 6, &mut rng, &pending);
        pending.extend(next);
        let ids: Vec<i32> = pending
            .iter()
            .filter_map(|op| match op {
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(g))) => Some(g.buff_id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 8);
        assert_eq!(
            ids[..6]
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            6
        );
    }

    #[test]
    fn gloves_add_actual_action_point_on_round_one_and_every_three_rounds() {
        let (mut managers, pool) = setup(&[(8220044, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            crate::engine::round::modifier::active_buff_action_bonus(&managers, 1),
            1
        );
        crate::engine::runtime::schedule::run_attacker_round_end(
            &mut managers,
            &pool,
            &SkillEffectCatalog::default(),
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            crate::engine::round::modifier::active_buff_action_bonus(&managers, 1),
            0
        );
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            crate::engine::round::modifier::active_buff_action_bonus(&managers, 1),
            0
        );
        let ops = round_start(&mut managers, &pool, 4, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            crate::engine::round::modifier::active_buff_action_bonus(&managers, 1),
            1
        );
    }

    #[test]
    fn hunter_belt_counts_completed_selected_cards_per_actor() {
        let (mut managers, pool) = setup(&[(8220515, 0)]);
        managers.card = CardManager::new(vec![CardInfo {
            uid: Some(10),
            skill_id: Some(30030111),
            ..Default::default()
        }]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        let played = managers.card.play_card(0, Some(-1), None, None).unwrap();
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: played.card_index,
            rank: 1,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        execute(&mut managers, &pool, ops);
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90082), 0);
        assert_eq!(managers.buff.buff_id_amount(11, 90082), 1);
        assert_eq!(managers.buff.buff_id_amount(12, 90082), 0);
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }

    #[test]
    fn fairy_cleanse_is_for_support_spells_and_not_support_ultimates() {
        let (mut managers, pool) = setup(&[(8120110, 40)]);
        execute(
            &mut managers,
            &pool,
            vec![give(10, 90152, 1), give(10, 90222, 1)],
        );
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030131,
            card_index: 1,
            ..Default::default()
        };
        assert!(action(&mut managers, &pool, &event, true, &mut rng).is_empty());
        let normal = ActionEvent {
            skill_id: 30030112,
            rank: 2,
            ..event
        };
        let ops = action(&mut managers, &pool, &normal, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90152), 0);
        assert_eq!(managers.buff.buff_id_amount(10, 90222), 0);
    }
    #[test]
    fn dancing_shoes_track_first_action_per_actor_and_round_with_extra_procs() {
        use crate::engine::skill::action::SkillExecutionMode;
        let (mut managers, pool) = setup(&[(8120703, 50), (8120704, 50), (8120705, 50)]);
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 0, 0, 0, 0, 0, 0]);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        let normal = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            mode: SkillExecutionMode::Active,
            ..Default::default()
        };
        let first = action(&mut managers, &pool, &normal, false, &mut rng);
        execute(&mut managers, &pool, first);
        assert_eq!(managers.buff.buff_id_amount(10, 90051), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 30750131), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 30750132), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 91210030), 2);
        assert!(action(&mut managers, &pool, &normal, false, &mut rng).is_empty());
        let extra = ActionEvent {
            card_index: 0,
            ..normal.clone()
        };
        let extra_ops = action(&mut managers, &pool, &extra, false, &mut rng);
        let granted: Vec<_> = extra_ops
            .iter()
            .filter_map(|op| match op {
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(g))) => Some(g.buff_id),
                _ => None,
            })
            .collect();
        assert_eq!(granted, vec![90051, 30750131, 91210030]);
        execute(&mut managers, &pool, extra_ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90051), 2);
        let other = ActionEvent {
            source_uid: 11,
            ..normal.clone()
        };
        let ops = action(&mut managers, &pool, &other, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(11, 90051), 1);
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        let ops = action(&mut managers, &pool, &normal, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90051), 3);
    }

    #[test]
    fn voyage_medal_applies_frozen_range_before_telescope_probability() {
        let old_item: Artefact = serde_json::from_str("{}").unwrap();
        assert_eq!(old_item.artefacts_in_range_count, 0);
        let frozen_item = Artefact {
            uid: 100,
            item_id: 8330005,
            artefacts_in_range_count: 3,
            ..Default::default()
        };
        let restored: Artefact =
            serde_json::from_str(&serde_json::to_string(&frozen_item).unwrap()).unwrap();
        assert_eq!(restored.artefacts_in_range_count, 3);
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                catalyst: Some(Catalyst {
                    style_id: 1,
                    power: 40,
                    power_limit: 100,
                    ..Default::default()
                }),
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8330005,
                    count2: 1,
                    artefacts_in_range_count: 3,
                    extra_action_chance_doublers: 1,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        // (5% base + 5% enhancement + 15% from neighbours) * 2 = 50%.
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([499, 500]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            ..Default::default()
        };
        let awarded = action(&mut managers, &pool, &event, false, &mut rng);
        assert!(!awarded.is_empty());
        assert_eq!(managers.rouge.resources().unwrap().power, 42);
        execute(&mut managers, &pool, awarded);
        let missed = action(&mut managers, &pool, &event, false, &mut rng);
        assert!(missed.is_empty());
        assert_eq!(managers.rouge.resources().unwrap().power, 42);
    }

    #[test]
    fn voyage_medal_restores_real_rouge_power_after_each_successful_action() {
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                catalyst: Some(Catalyst {
                    style_id: 1,
                    power: 40,
                    power_limit: 100,
                    ..Default::default()
                }),
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8330005,
                    count2: 19,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        assert!(
            !ops.is_empty(),
            "power changes must emit synchronization markers"
        );
        assert_eq!(managers.rouge.resources().unwrap().power, 42);
        execute(&mut managers, &pool, ops);
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        assert_eq!(managers.rouge.resources().unwrap().power, 44);
        execute(&mut managers, &pool, ops);
    }
    #[test]
    fn supportive_spell_tags_distinguish_buff_healing_debuff_and_attack() {
        let (mut managers, pool) = setup(&[(8120035, 0), (8210032, 0)]);
        assert_eq!(spell_show_tag(&managers, 30020121), Some(3));
        assert_eq!(spell_show_tag(&managers, 30050121), Some(4));
        assert_eq!(spell_show_tag(&managers, 30090121), Some(6));
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30020121,
            card_index: 1,
            rank: 1,
            ..Default::default()
        };
        assert!(action(&mut managers, &pool, &event, false, &mut rng).is_empty());
        let healing = ActionEvent {
            skill_id: 30090121,
            ..event.clone()
        };
        let ops = action(&mut managers, &pool, &healing, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 30750131), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 30750132), 1);
        assert_eq!(managers.buff.buff_id_amount(-1, 4150001), 0);
        let buff = ActionEvent {
            skill_id: 30050121,
            ..event
        };
        let ops = action(&mut managers, &pool, &buff, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(-1, 4150001), 10);
        assert_eq!(managers.buff.buff_id_amount(-1, 30740171), 1);
    }

    #[test]
    fn chainsaw_applies_two_round_gash_and_spends_only_actor_dynamo() {
        let (mut managers, pool) = setup(&[(8210819, 0)]);
        execute(
            &mut managers,
            &pool,
            vec![give(10, 90071, 2), give(11, 90071, 2)],
        );
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            is_attack: true,
            target_uids: vec![-1],
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(-1, 90062), 1);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 1);
        assert_eq!(managers.buff.buff_id_amount(11, 90071), 2);
        let gash = managers
            .buff
            .active_for(-1)
            .find(|buff| buff.buff_id == Some(90062))
            .unwrap();
        assert_eq!(gash.from_uid, Some(10));
    }

    #[test]
    fn ritual_dagger_restores_precast_stacks_even_after_ultimate_consumes_them() {
        let (mut managers, pool) = setup(&[(8210205, 0)]);
        execute(&mut managers, &pool, vec![give(10, 90091, 10)]);
        let ops = before_card(&managers, &pool, 10, 30030131, 1, true);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 821020511), 2);
        let spent = RuleOp::Command(BattleCommand::Buff(BuffCommand::ConsumeCoalesced(
            BuffConsume {
                origin: ORIGIN,
                target_uid: 10,
                selector: BuffSelector::ExactId(90091),
                amount: 10,
                depleted: DepletedBuff::Remove,
            },
        )));
        execute(&mut managers, &pool, vec![spent]);
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030131,
            card_index: 1,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 821020511), 0);
        assert_eq!(managers.buff.buff_id_amount(10, 90091), 2);
        assert!(action(&mut managers, &pool, &event, false, &mut rng).is_empty());
    }
    #[test]
    fn forest_whispers_triggers_from_a_real_scene_medication_play() {
        use crate::{Battle, catalog::BattleCatalog, dungeon::BuiltFight};
        use sonettobuf::{BeginRoundOper, BeginRoundRequest};
        crate::test_support::init_config();
        let entity = |uid: i64, position: i32, hp: i32| FightEntityInfo {
            uid: Some(uid),
            model_id: Some(1001),
            position: Some(position),
            current_hp: Some(hp),
            attr: Some(HeroAttribute {
                hp: Some(hp),
                attack: Some(100),
                ..Default::default()
            }),
            skill_group1: if uid > 0 { vec![101] } else { vec![] },
            skill_group2: if uid > 0 { vec![201] } else { vec![] },
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![entity(10, 1, 1000), entity(11, 2, 1000)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![entity(-1, 1, 5000)],
                ..Default::default()
            }),
            version: Some(7),
            cur_round: Some(1),
            max_round: Some(20),
            ..Default::default()
        };
        let skill_id = crate::rouge::artefact_entry::medication_skill_id(8150006).unwrap();
        let context = RougeContext {
            artefacts: vec![Artefact {
                uid: 100,
                item_id: 8350043,
                ..Default::default()
            }],
            entry: Some(EntryContext {
                medication_cards: vec![crate::rouge::artefact_entry::MedicationCard {
                    source_uid: 101,
                    item_id: 8150006,
                    skill_id,
                    count: 1,
                }],
                ..Default::default()
            }),
            catalyst: Some(Catalyst {
                style_id: 0,
                power: 1,
                power_limit: 40,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut battle = Battle::start(
            BattleCatalog::new(crate::test_support::game_data()),
            BuiltFight {
                fight,
                ex_attributes: vec![],
                sp_attributes: vec![],
                rouge: Some(context),
                battle_rule_skills: vec![],
            },
            42,
        )
        .unwrap();
        let initial = battle.reconnect_state().1.unwrap();
        let index = initial
            .team_a_cards1
            .iter()
            .position(|card| {
                card.uid == Some(0) && card.skill_id == Some(skill_id) && card.card_type == Some(1)
            })
            .unwrap();
        crate::dungeon::begin_round(
            &mut battle,
            BeginRoundRequest {
                opers: vec![BeginRoundOper {
                    oper_type: Some(crate::engine::manager::card::CardOpType::PlayCard as i32),
                    param1: Some(index as i32 + 1),
                    to_id: Some(10),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap();
        for uid in [10, 11] {
            let actor = battle.entity_info(uid).unwrap();
            let count = actor
                .buffs
                .iter()
                .filter(|buff| {
                    [90001, 90011, 90041, 90021, 90031, 90281]
                        .contains(&buff.buff_id.unwrap_or_default())
                })
                .count();
            assert_eq!(
                count, 3,
                "each living actor receives three actual permanent buffs"
            );
        }
    }

    #[test]
    fn forest_whispers_ignores_unplayed_or_generic_temporary_cards() {
        let (mut managers, pool) = setup(&[(8350043, 0)]);
        managers.card = CardManager::new(vec![CardInfo {
            uid: Some(10),
            skill_id: Some(30030111),
            temp_card: Some(true),
            card_type: Some(6),
            ..Default::default()
        }]);
        let mut rng = RoundDeterminism::with_seed(42);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            ..Default::default()
        };
        assert!(action(&mut managers, &pool, &event, false, &mut rng).is_empty());
        managers.card.play_card(0, Some(-1), None, None).unwrap();
        assert!(action(&mut managers, &pool, &event, false, &mut rng).is_empty());
    }
    #[test]
    fn tall_hat_uses_per_actor_support_counts_and_frozen_layout_threshold() {
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8220215,
                    count2: 1,
                    only_support_below: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        let spell = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            ..Default::default()
        };
        let other = ActionEvent {
            source_uid: 11,
            ..spell.clone()
        };
        let ops = action(&mut managers, &pool, &spell, true, &mut rng);
        execute(&mut managers, &pool, ops);
        let ops = action(&mut managers, &pool, &other, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers.ex_point.get(10),
            0,
            "different actors do not combine their counts"
        );
        let ops = action(&mut managers, &pool, &spell, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.ex_point.get(10), 1);
        assert_eq!(managers.ex_point.get(11), 1);
        let ops = action(&mut managers, &pool, &spell, true, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers.ex_point.get(10),
            1,
            "crossing is one award per actor per round"
        );
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        for _ in 0..2 {
            let ops = action(&mut managers, &pool, &spell, true, &mut rng);
            execute(&mut managers, &pool, ops);
        }
        assert_eq!(managers.ex_point.get(10), 2);
    }
    #[test]
    fn grenade_reserves_each_energy_stack_at_most_once_in_a_round() {
        let (mut managers, pool) = setup(&[(8110805, 0), (8110805, 0)]);
        execute(
            &mut managers,
            &pool,
            vec![give(10, 90071, 1), give(11, 90071, 2)],
        );
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        let sources: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                RuleOp::Skill(skill) if skill.plan.skill_id == 811080592 => {
                    Some(skill.plan.source_uid)
                }
                _ => None,
            })
            .collect();
        assert_eq!(sources, vec![10, 11, 11]);
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
    }

    #[test]
    fn grenade_uses_loaded_configured_aoe_genesis_damage_at_opening() {
        use crate::{Battle, catalog::BattleCatalog, dungeon::BuiltFight};
        use sonettobuf::BuffInfo;
        crate::test_support::init_config();
        let entity = |uid: i64, position: i32, hp: i32| FightEntityInfo {
            uid: Some(uid),
            model_id: Some(1001),
            position: Some(position),
            current_hp: Some(hp),
            attr: Some(HeroAttribute {
                hp: Some(hp),
                attack: Some(100),
                ..Default::default()
            }),
            skill_group1: if uid > 0 { vec![101] } else { vec![] },
            skill_group2: if uid > 0 { vec![201] } else { vec![] },
            buffs: if uid == 10 {
                vec![BuffInfo {
                    uid: Some(99),
                    buff_id: Some(90071),
                    layer: Some(1),
                    duration: Some(0),
                    from_uid: Some(10),
                    ..Default::default()
                }]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![entity(10, 1, 1000), entity(11, 2, 1000)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![entity(-1, 1, 5000)],
                ..Default::default()
            }),
            version: Some(7),
            cur_round: Some(1),
            max_round: Some(20),
            ..Default::default()
        };
        let context = RougeContext {
            artefacts: vec![Artefact {
                uid: 100,
                item_id: 8110805,
                ..Default::default()
            }],
            ..Default::default()
        };
        let battle = Battle::start(
            BattleCatalog::new(crate::test_support::game_data()),
            BuiltFight {
                fight,
                ex_attributes: vec![],
                sp_attributes: vec![],
                rouge: Some(context),
                battle_rule_skills: vec![],
            },
            42,
        )
        .unwrap();
        assert_eq!(battle.entity_info(-1).unwrap().current_hp, Some(4900));
        assert!(
            !battle
                .entity_info(10)
                .unwrap()
                .buffs
                .iter()
                .any(|b| b.buff_id == Some(90071))
        );
    }

    #[test]
    fn support_net_rolls_once_plus_each_live_deployed_support_and_syncs_resources() {
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8130107,
                    ..Default::default()
                }],
                catalyst: Some(Catalyst {
                    style_id: 1,
                    power: 20,
                    power_limit: 100,
                    ..Default::default()
                }),
                support_skills: vec![
                    SupportSkill {
                        owner_uid: 10,
                        hero_id: 1001,
                        skill_ids: vec![],
                    },
                    SupportSkill {
                        owner_uid: 11,
                        hero_id: 1002,
                        skill_ids: vec![],
                    },
                    SupportSkill {
                        owner_uid: 12,
                        hero_id: 1003,
                        skill_ids: vec![],
                    },
                ],
                ..Default::default()
            },
        );
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 999, 0]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &event, true, &mut rng);
        assert_eq!(managers.rouge.resources().unwrap().power, 40);
        assert!(
            !ops.is_empty(),
            "actual recovery also sends protocol markers"
        );
        execute(&mut managers, &pool, ops);
        let ultimate = ActionEvent {
            skill_id: 30030131,
            rank: 0,
            ..event
        };
        assert!(action(&mut managers, &pool, &ultimate, true, &mut rng).is_empty());
        assert_eq!(managers.rouge.resources().unwrap().power, 40);
    }

    #[test]
    fn telescope_doubles_positive_and_negative_item_probability_only() {
        let run = |item_id, doubled, roll| {
            let (mut managers, pool) = setup(&[]);
            let count2 = if item_id == 8220814 { 50 } else { 0 };
            configure(
                &mut managers,
                RougeContext {
                    artefacts: vec![Artefact {
                        uid: 100,
                        item_id,
                        count2,
                        extra_action_chance_doublers: i32::from(doubled),
                        ..Default::default()
                    }],
                    catalyst: Some(Catalyst {
                        style_id: 1,
                        power: 20,
                        power_limit: 100,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
            let mut rng = RoundDeterminism::with_seed(42);
            rng.enqueue_permille_rolls([roll]);
            let ops = round_start(&mut managers, &pool, 1, &mut rng);
            execute(&mut managers, &pool, ops);
            managers.rouge.resources().unwrap().power
        };
        assert_eq!(run(8230004, false, 150), 20);
        assert_eq!(run(8230004, true, 150), 22);
        assert_eq!(run(8220814, false, 750), 20);
        assert_eq!(run(8220814, true, 750), 19);
    }
    #[test]
    fn old_bag_hand_limit_tracks_living_main_characters() {
        use crate::engine::manager::hp::{HpHeal, HpHealKind, HpLoss};
        let (mut managers, pool) = setup(&[(8230039, 0)]);
        let limit = |managers: &BattleManagers| {
            crate::engine::mechanic::card::CardMechanic.normal_hand_limit(8, managers, &pool)
        };
        assert_eq!(limit(&managers), 9, "two living mains add one slot");
        let loss = RuleOp::Command(BattleCommand::Hp(HpCommand::Lose(HpLoss {
            origin: ORIGIN,
            source_uid: 11,
            target_uid: 11,
            amount: 1000,
            config_effect: 0,
            hurt: None,
        })));
        execute(&mut managers, &pool, vec![loss]);
        assert_eq!(limit(&managers), 8, "one living main has no extra slots");
        let revive = RuleOp::Command(BattleCommand::Hp(HpCommand::Heal(HpHeal {
            origin: ORIGIN,
            source_uid: 10,
            target_uid: 11,
            amount: 100,
            config_effect: 0,
            kind: HpHealKind::Revive,
        })));
        execute(&mut managers, &pool, vec![revive]);
        assert_eq!(limit(&managers), 9, "revival restores the live-count bonus");
        managers.rouge = RougeRuntime::default();
        assert_eq!(
            limit(&managers),
            8,
            "ordinary battles do not receive an old-bag bonus"
        );
    }

    #[test]
    fn old_bags_choose_distinct_normal_cards_and_expire_unplayed_instances() {
        let (mut managers, pool) = setup(&[(8230039, 0), (8230039, 0)]);
        let normals = [30030111, 30030112, 30030113, 30050111, 30050112];
        let mut hand: Vec<CardInfo> = normals
            .iter()
            .map(|id| CardInfo {
                uid: Some(10),
                skill_id: Some(*id),
                ..Default::default()
            })
            .collect();
        hand.push(CardInfo {
            uid: Some(10),
            skill_id: Some(30030131),
            ..Default::default()
        });
        hand.push(CardInfo {
            uid: Some(0),
            skill_id: Some(815000611),
            card_type: Some(1),
            ..Default::default()
        });
        managers.card = CardManager::new(hand);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers
                .card
                .hand()
                .iter()
                .filter(|c| c.temp_card == Some(true))
                .count(),
            5
        );
        assert!(
            managers
                .card
                .hand()
                .iter()
                .filter(|c| c.temp_card == Some(true))
                .all(|c| normals.contains(&c.skill_id.unwrap_or_default()))
        );
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
        // Playing one and moving the hand must not confuse instance tracking.
        assert!(managers.card.move_card(0, 4));
        let played_index = managers
            .card
            .hand()
            .iter()
            .position(|c| c.temp_card == Some(true))
            .unwrap();
        managers
            .card
            .play_card(played_index, Some(-1), None, None)
            .unwrap();
        crate::engine::runtime::schedule::run_temporary_card_expiration(
            &mut managers,
            &pool,
            &SkillEffectCatalog::default(),
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(managers.card.hand().len(), 2);
        assert!(
            managers
                .card
                .hand()
                .iter()
                .all(|c| c.temp_card != Some(true))
        );
        assert!(managers.card.hand().iter().any(|c| c.card_type == Some(1)));
        assert!(
            managers
                .card
                .hand()
                .iter()
                .any(|c| c.skill_id == Some(30030131))
        );
    }

    #[test]
    fn old_bag_real_round_expires_unplayed_precast_cards() {
        use crate::{Battle, catalog::BattleCatalog, dungeon::BuiltFight};
        use sonettobuf::{BeginRoundOper, BeginRoundRequest};
        crate::test_support::init_config();
        let ally = |uid: i64, position: i32| FightEntityInfo {
            uid: Some(uid),
            model_id: Some(3003),
            position: Some(position),
            current_hp: Some(10000),
            attr: Some(HeroAttribute {
                hp: Some(10000),
                attack: Some(100),
                ..Default::default()
            }),
            skill_group1: vec![30030111, 30030112, 30030113],
            skill_group2: vec![30030121, 30030122, 30030123],
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![ally(10, 1), ally(11, 2), ally(12, 3), ally(13, 4)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(-1),
                    position: Some(1),
                    model_id: Some(1001),
                    current_hp: Some(100000),
                    attr: Some(HeroAttribute {
                        hp: Some(100000),
                        attack: Some(1),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            version: Some(7),
            cur_round: Some(1),
            max_round: Some(20),
            ..Default::default()
        };
        let context = RougeContext {
            artefacts: vec![Artefact {
                uid: 100,
                item_id: 8230039,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut battle = Battle::start(
            BattleCatalog::new(crate::test_support::game_data()),
            BuiltFight {
                fight,
                ex_attributes: vec![],
                sp_attributes: vec![],
                rouge: Some(context),
                battle_rule_skills: vec![],
            },
            42,
        )
        .unwrap();
        let initial = battle.reconnect_state().1.unwrap();
        // team_a_cards1 is the original dealt hand. The actual current hand
        // and the opening CHANGETOTEMPCARD operation must agree on indices.
        assert!(
            initial
                .team_a_cards1
                .iter()
                .all(|card| card.temp_card != Some(true))
        );
        let push = battle.card_info_push();
        let temporary: Vec<_> = push
            .card_group
            .iter()
            .enumerate()
            .filter(|(_, card)| card.temp_card == Some(true))
            .collect();
        assert_eq!(
            temporary.len(),
            3,
            "the authoritative hand contains the marked cards"
        );
        let marker = initial
            .fight_step
            .iter()
            .flat_map(|step| &step.act_effect)
            .find(|effect| {
                effect.effect_type
                    == Some(sonettobuf::effect_type_enum::EffectType::Changetotempcard as i32)
                    && effect.config_effect == Some(50023)
            })
            .expect("opening must transmit the actual conversion to the client");
        let mut sent_indices: Vec<usize> = marker
            .reserve_str
            .as_deref()
            .unwrap_or_default()
            .split('#')
            .map(|part| part.parse::<usize>().unwrap())
            .collect();
        let mut actual_indices: Vec<usize> = temporary.iter().map(|(index, _)| index + 1).collect();
        sent_indices.sort_unstable();
        actual_indices.sort_unstable();
        assert_eq!(
            sent_indices, actual_indices,
            "wire indices address the authoritative marked instances"
        );
        let (index, card) = temporary[0];
        let target = if card.skill_id == Some(30030111) {
            -1
        } else {
            card.uid.unwrap_or(10)
        };
        let round = crate::dungeon::begin_round(
            &mut battle,
            BeginRoundRequest {
                opers: vec![BeginRoundOper {
                    oper_type: Some(crate::engine::manager::card::CardOpType::PlayCard as i32),
                    param1: Some(index as i32 + 1),
                    to_id: Some(target),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap()
        .round
        .unwrap();
        let expired = round
            .fight_step
            .iter()
            .flat_map(|step| &step.act_effect)
            .find(|effect| {
                effect.effect_type
                    == Some(sonettobuf::effect_type_enum::EffectType::Cardremove as i32)
                    && effect.json_param.as_deref() == Some(r#"{"buffActActiveIds":""}"#)
            })
            .expect("unplayed temporary cards expire on the real round");
        assert_eq!(
            expired
                .reserve_str
                .as_deref()
                .unwrap_or_default()
                .split('#')
                .count(),
            2
        );
    }
    #[test]
    fn old_bag_uses_all_configured_live_roster_thresholds() {
        crate::test_support::init_config();
        for count in 0..=4 {
            let fight = Fight {
                attacker: Some(FightTeam {
                    entitys: (0..4)
                        .map(|index| FightEntityInfo {
                            uid: Some(10 + index),
                            team_type: Some(1),
                            position: Some(index as i32 + 1),
                            current_hp: Some(if index < count { 1000 } else { 0 }),
                            attr: Some(HeroAttribute {
                                hp: Some(1000),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                }),
                defender: Some(FightTeam::default()),
                ..Default::default()
            };
            let mut managers = BattleManagers::seeded(&fight);
            configure(
                &mut managers,
                RougeContext {
                    artefacts: vec![Artefact {
                        uid: 100,
                        item_id: 8230039,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
            let pool = TargetPool::from_fight(&fight);
            assert_eq!(
                crate::rouge::old_bag_hand_limit_bonus(&managers, &pool),
                (count - 1).clamp(0, 3) as i32
            );
        }
    }
    #[test]
    fn stag_struggle_first_and_extra_actions_use_independent_probabilities() {
        use crate::engine::skill::action::SkillExecutionMode;
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                artefacts: vec![Artefact {
                    uid: 100,
                    item_id: 8320709,
                    count2: 40,
                    count4: 50,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 0]);
        let first = ActionEvent {
            source_uid: 10,
            skill_id: 30030111,
            card_index: 1,
            rank: 1,
            mode: SkillExecutionMode::Active,
            ..Default::default()
        };
        let ops = action(&mut managers, &pool, &first, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90021), 2);
        assert_eq!(managers.buff.buff_id_amount(10, 91210030), 2);
        assert!(action(&mut managers, &pool, &first, false, &mut rng).is_empty());
        let extra = ActionEvent {
            card_index: 0,
            ..first
        };
        let ops = action(&mut managers, &pool, &extra, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90021), 3);
        assert_eq!(managers.buff.buff_id_amount(10, 91210030), 3);
        assert_eq!(managers.buff.buff_id_amount(11, 90021), 0);
    }

    #[test]
    fn hunter_and_bird_egg_apply_frozen_layout_conditions() {
        let (mut managers, pool) = setup(&[]);
        configure(
            &mut managers,
            RougeContext {
                artefacts: vec![
                    Artefact {
                        uid: 100,
                        item_id: 8220515,
                        count2: 1,
                        non_resonance_in_range: true,
                        ..Default::default()
                    },
                    Artefact {
                        uid: 101,
                        item_id: 8330111,
                        support_below_count: 2,
                        ..Default::default()
                    },
                ],
                catalyst: Some(Catalyst {
                    style_id: 1,
                    power: 50,
                    power_limit: 100,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90082), 3);
            let total: i32 = [90001, 90011, 90041, 90021, 90031, 90281]
                .iter()
                .map(|id| managers.buff.buff_id_amount(uid, *id))
                .sum();
            assert_eq!(
                total, 4,
                "base one + power bonus one + two placed Support neighbours"
            );
        }
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }

    #[test]
    fn magazine_round_start_draws_a_bullet_and_burst_for_each_plan_stack() {
        let (mut managers, pool) = setup(&[(8330715, 0)]);
        execute(&mut managers, &pool, vec![give(10, 90051, 2)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        let granted: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(g))) => {
                    Some((g.target_uid, g.buff_id))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            granted
                .iter()
                .filter(|&&(uid, id)| uid == 10 && id == 30750131)
                .count(),
            2
        );
        assert_eq!(
            granted
                .iter()
                .filter(|&&(uid, id)| uid == 10 && (91130031..=91130039).contains(&id))
                .count(),
            2
        );
        assert!(!granted.iter().any(|&(uid, _)| uid == 11));
        execute(&mut managers, &pool, ops);
        assert_eq!(
            managers.buff.buff_id_amount(10, 90051),
            2,
            "Plan is counted, not spent"
        );
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
    }

    #[test]
    fn biting_crossbow_gives_reflect_only_to_living_fielded_allies() {
        let (mut managers, pool) = setup(&[(8310114, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 2130005), 1);
        assert_eq!(managers.buff.buff_id_amount(11, 2130005), 1);
        assert_eq!(managers.buff.buff_id_amount(12, 2130005), 0);
        assert_eq!(managers.buff.buff_id_amount(-1, 2130005), 0);
    }
}
