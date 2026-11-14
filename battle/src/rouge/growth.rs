use super::*;
use crate::engine::skill::action::ActionEvent;

const GROWTH_KEY: &str = "growth_pct";

fn award(managers: &mut BattleManagers, source: &GrowthSource, actor: i64, trigger: i32, pct: i32) {
    let Some(attr) = (match source.item_id {
        8310405 => Some(AttrId::DmgBonus),
        8330418 => Some(AttrId::DmgTakenReduction),
        8320310 => Some(AttrId::IncantationMight),
        _ => None,
    }) else {
        return;
    };
    let state = managers.rouge.active.as_mut().unwrap();
    if !state.growth_triggers.insert((source.uid, actor, trigger)) {
        return;
    }
    let value = state
        .counters
        .entry(source.uid)
        .or_default()
        .entry(GROWTH_KEY.into())
        .or_default();
    *value = value.saturating_add(pct);
    // Existing permanent_growth was already included in the frozen Fight.
    // Only this battle's new shared bonus is applied here.
    for uid in source.actor_uids.iter().copied().collect::<BTreeSet<_>>() {
        if managers.entity.team_type(uid) == Some(1) {
            managers.attribute.add(uid, attr, pct.saturating_mul(10));
        }
    }
}

pub(super) fn completed_action(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    action: &ActionEvent,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let sources = context
        .growth_sources
        .iter()
        .filter(|source| {
            context
                .artefacts
                .iter()
                .any(|item| item.uid == source.uid && item.item_id == source.item_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let ultimate = managers.catalog().skill_is_big(action.skill_id);
    let mut ops = Vec::new();
    if ultimate && pool.source_is_attacker(action.source_uid) {
        // Lyrical Lance keeps permanent Ceremonious stacks per artefact UID.
        // Growth from confirmed enemy kills is applied before the award for
        // this ultimate; this ordering is a local interpretation. Only the
        // new stacks are reported; the frozen baseline is not granted twice.
        let lance_items = context
            .artefacts
            .iter()
            .filter(|item| {
                item.item_id == 8210209
                    && sources
                        .iter()
                        .any(|source| source.uid == item.uid && source.item_id == item.item_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let frozen = context.permanent_growth.clone();
        for item in lance_items {
            let baseline = frozen
                .get(&item.uid)
                .and_then(|values| values.get("ritual_stacks"))
                .copied()
                .unwrap_or_default()
                .clamp(0, 10);
            let state = managers.rouge.active.as_mut().unwrap();
            let prior = state
                .counters
                .get(&item.uid)
                .and_then(|values| values.get("ritual_stacks"))
                .copied()
                .unwrap_or_default()
                .clamp(0, 10 - baseline);
            let new_stacks = action.enemy_kill_count.max(0).min(10 - baseline - prior);
            let total = prior.saturating_add(new_stacks);
            if new_stacks > 0 {
                state
                    .counters
                    .entry(item.uid)
                    .or_default()
                    .insert("ritual_stacks".to_owned(), total);
            }
            let chance = item.proc_chance(500_i32.saturating_add(item.count4.saturating_mul(10)));
            if determinism.roll_permille(chance) {
                let amount = 2_i32
                    .saturating_add(item.count2)
                    .saturating_add(baseline)
                    .saturating_add(total)
                    .clamp(0, 100);
                if amount > 0 {
                    for entity in &pool.attacker_main {
                        if managers.hp.current(entity.uid) > 0 {
                            ops.push(grant_stacks(action.source_uid, entity.uid, 90091, amount));
                        }
                    }
                }
            }
        }
    }
    if !ultimate && matches!(action.rank, 2 | 3) {
        for source in sources.iter().filter(|source| {
            source.item_id == 8320310 && source.actor_uids.contains(&action.source_uid)
        }) {
            if action.rank == 2 {
                award(managers, source, action.source_uid, 2, 2);
            } else if source.rare_artefact_count >= 10 {
                award(managers, source, action.source_uid, 3, 4);
            }
        }
    }
    // A selected rank-two or rank-three spell triggers the claw even if
    // syntony has not been unlocked. Queued or failed cards do not.
    let selected = action.card_index > 0
        && managers.card.played().iter().any(|played| {
            played.card_index == action.card_index
                && played.caster_uid == action.source_uid
                && played.skill_id == action.skill_id
                && (matches!(played.card.card_type, None | Some(0 | 2))
                    || (played.card.card_type == Some(6)
                        && played.card.temp_card == Some(true)
                        && managers.card.is_hero_precast_play(action.card_index)
                        && pool.entity(action.source_uid).is_some_and(|owner| {
                            owner.skill_group1.contains(&action.skill_id)
                                || owner.skill_group2.contains(&action.skill_id)
                        })))
        });
    let extra = crate::engine::skill::action::performs_extra_action(
        action.card_index,
        action.mode,
        action.extra_skill_kind,
    );
    let items = managers
        .rouge
        .context()
        .map(|c| c.artefacts.clone())
        .unwrap_or_default();
    if selected
        && !ultimate
        && !extra
        && matches!(action.rank, 2 | 3)
        && managers.entity.team_type(action.source_uid) == Some(1)
    {
        for _ in items.iter().filter(|item| item.item_id == 8310510) {
            ops.extend(super::catalyst::coin_change(managers, 5));
            if managers.hp.current(action.source_uid) > 0 {
                ops.push(grant_stacks(action.source_uid, action.source_uid, 90021, 2));
            }
        }
    }
    // Cross-round triggers are recorded on the actual completed card,
    // separately from current-round level calculation. Quotas count successful
    // triggers, not failed probability rolls.
    let played = action.card_index > 0
        && managers.card.played().iter().any(|card| {
            card.card_index == action.card_index
                && card.caster_uid == action.source_uid
                && card.skill_id == action.skill_id
        });
    let aid = super::is_support_card(
        managers,
        action.source_uid,
        action.skill_id,
        action.card_index,
    );
    if played && !extra && managers.entity.team_type(action.source_uid) == Some(1) {
        for item in &items {
            if item.item_id == 8220409 && aid {
                let limit = 2_i32
                    .saturating_add(item.count4)
                    .saturating_add(if item.syntony_in_range_count > 0 {
                        2
                    } else {
                        0
                    })
                    .max(0);
                if quota_available(managers, item.uid, 8220409, limit)
                    && determinism.roll_permille(
                        item.proc_chance(600_i32.saturating_add(item.count2.saturating_mul(10))),
                    )
                {
                    queue_syntony_bonus(managers, item.uid, 8220409, 1);
                }
            } else if item.item_id == 8310407 && aid {
                let limit = if item.support_in_range_count >= 4 {
                    i32::MAX
                } else {
                    5
                };
                if quota_available(managers, item.uid, 83104071, limit) {
                    queue_syntony_bonus(managers, item.uid, 83104071, 1);
                }
            } else if item.item_id == 8320408 && ultimate {
                let amount = 1_i32
                    .saturating_add(item.count2)
                    .saturating_add(i32::from(item.syntony_in_range_count >= 4))
                    .max(0);
                queue_syntony_bonus(managers, item.uid, 8320408, amount);
            }
        }
    }
    // Real player turns settle the complete submitted queue before any card
    // acts. Keep the standalone direct-cast path for callers without a queue.
    if managers
        .rouge
        .active
        .as_ref()
        .is_some_and(|state| state.incantation.queue_settled)
    {
        return ops;
    }
    if !selected || ultimate || extra {
        return ops;
    }
    let kind = normalized_category(managers.catalog().skill_effect_tag(action.skill_id));
    if !matches!(kind, 1 | 3 | 4 | 5 | 6 | 8 | 9) {
        return ops;
    }
    if let Some(state) = managers.rouge.active.as_mut() {
        let count = state.incantation.casts_by_type.entry(kind).or_default();
        *count = count.saturating_add(1);
    }
    ops.extend(settle_kind(managers, pool, action.source_uid, kind, determinism));
    ops
}

fn normalized_category(tag: i32) -> i32 {
    match tag {
        1 | 2 => 1,
        tag => tag,
    }
}

/// Settle the physical cards accepted into the player action queue. Captured
/// level packets precede its first action: normal, support, ultimate and the
/// medicine cards with actor UID 0 all contribute. Card type 7 is only the
/// device placeholder and is excluded. A generic skill3 basic is excluded
/// unless it is an actual hero precast (other such cases remain inferred).
/// Invalid cards dissolved before commitment do not enter `played`; a card
/// committed here still counts if its actor later dies or its target vanishes.
pub(crate) fn settle_queue(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(active) = managers.rouge.active.as_ref() else {
        return Vec::new();
    };
    if active.context.incantation_rules.is_none() || active.incantation.queue_settled {
        return Vec::new();
    }
    let mut counts = BTreeMap::<i32, (i32, i64)>::new();
    for played in managers.card.played() {
        let card_type = played.card.card_type.unwrap_or_default();
        if card_type == 7
            || (card_type == 6
                && !managers.catalog().skill_is_big(played.skill_id)
                && !managers.card.is_hero_precast_play(played.card_index))
        {
            continue;
        }
        let kind = normalized_category(managers.catalog().skill_effect_tag(played.skill_id));
        // The captured tag 14 (device-themed character card, not type 7)
        // contributes a distinct category to resonance. Its own polarization
        // table is not shipped; unknown tags are not invented.
        if !matches!(kind, 1 | 3 | 4 | 5 | 6 | 8 | 9 | 14) {
            continue;
        }
        let entry = counts.entry(kind).or_insert((0, played.caster_uid));
        entry.0 = entry.0.saturating_add(1);
    }
    {
        let state = managers.rouge.active.as_mut().unwrap();
        state.incantation.queue_settled = true;
        for (&kind, &(count, _)) in &counts {
            let value = state.incantation.casts_by_type.entry(kind).or_default();
            *value = value.saturating_add(count);
        }
    }
    // No captured queue has category 14 as the unique maximum. Conservatively
    // let it compete for the maximum instead of promoting a smaller group;
    // its own unsupported polarization remains unknown.
    let maximum = counts.values().map(|(count, _)| *count).max().unwrap_or_default();
    let mut candidates = counts
        .iter()
        .filter(|(_, (count, _))| *count == maximum && maximum >= 3)
        .map(|(&kind, &(_, uid))| (kind, uid))
        .collect::<Vec<_>>();
    if candidates.is_empty()
        && let Some((&kind, &(_, uid))) = counts.first_key_value()
    {
        candidates.push((kind, uid));
    }
    let mut markers = Vec::new();
    let mut effects = Vec::new();
    for (kind, source_uid) in candidates {
        for op in settle_kind(managers, pool, source_uid, kind, determinism) {
            if matches!(&op, RuleOp::EffectMarker { effect_type: 183 | 184, .. }) {
                markers.push(op);
            } else {
                effects.push(op);
            }
        }
    }
    // The client receives one final level for each tied maximal category,
    // followed by one resonance level, before the corresponding artefacts.
    markers.sort_by_key(|op| match op {
        RuleOp::EffectMarker { effect_type, config_effect, .. } => (*effect_type, *config_effect),
        _ => unreachable!(),
    });
    markers.extend(effects);
    markers
}

fn settle_kind(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    kind: i32,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context().cloned() else {
        return Vec::new();
    };
    let sources = context
        .growth_sources
        .iter()
        .filter(|source| context.artefacts.iter().any(|item| item.uid == source.uid && item.item_id == source.item_id))
        .cloned()
        .collect::<Vec<_>>();
    let items = context.artefacts.clone();
    let mut ops = Vec::new();
    let Some(rules) = managers
        .rouge
        .context()
        .and_then(|c| c.incantation_rules.clone())
    else {
        return ops;
    };
    let mut bonuses = super::catalyst::level_bonus(managers);
    bonuses.0 = bonuses.0.saturating_add(
        managers
            .rouge
            .active
            .as_ref()
            .map_or(0, |state| state.current_syntony_bonus),
    );
    let style = managers
        .rouge
        .context()
        .and_then(|context| context.catalyst.as_ref())
        .map_or(0, |catalyst| catalyst.style_id);
    let state = managers.rouge.active.as_mut().unwrap();
    let incantation = &mut state.incantation;
    let count = incantation.casts_by_type.get(&kind).copied().unwrap_or_default();
    let new_syntony = if count >= 3 && matches!(kind, 1 | 3 | 4 | 5 | 6 | 8 | 9) {
        (count - 2)
            .saturating_add(bonuses.0)
            .min(rules.syntony_level_limit.clamp(1, 7))
    } else {
        0
    };
    let old_syntony = incantation.levels.get(&kind).copied().unwrap_or_default();
    if new_syntony > old_syntony {
        incantation.levels.insert(kind, new_syntony);
        state.summary.syntony_max_level = state.summary.syntony_max_level.max(new_syntony);
    }
    let types = incantation.casts_by_type.len() as i32;
    let new_resonance = if types >= 3 {
        (types - 2)
            .saturating_add(bonuses.1)
            .min(rules.resonance_level_limit.clamp(1, 7))
    } else {
        0
    };
    let old_resonance = incantation.resonance_level;
    incantation.resonance_level = new_resonance;
    state.summary.resonance_max_level = state.summary.resonance_max_level.max(new_resonance);

    // Both level packets address the team marker (UID 0), as in the capture.
    let target = 0;
    if new_syntony > old_syntony {
        ops.push(level_marker(target, 183, new_syntony, kind));
        for source in sources
            .iter()
            .filter(|source| matches!(source.item_id, 8310405 | 8330418))
        {
            if new_syntony >= 2 {
                award(managers, source, 0, 2, 4);
            }
            if new_syntony >= 3 && source.syntony_artefact_count >= 10 {
                award(managers, source, 0, 3, 8);
            }
        }
    }
    if new_resonance > old_resonance {
        ops.push(level_marker(target, 184, new_resonance, 0));
    }
    // A level can jump over two or three after a catalyst bonus; crossing the
    // threshold is still one trigger. Random recipients are living fielded
    // allies, and a missing support mapping never fabricates another hero.
    if (style == 5 && old_syntony < 3 && new_syntony >= 3)
        || (style == 6 && old_resonance < 3 && new_resonance >= 3)
    {
        let allies = pool
            .attacker_main
            .iter()
            .filter(|entity| managers.hp.current(entity.uid) > 0)
            .map(|entity| entity.uid)
            .collect::<Vec<_>>();
        if let Some(index) = determinism.lua_random_index(allies.len()) {
            ops.push(super::grant(allies[index], allies[index], 90331));
        }
    }
    // The Eye has two independent once-per-fight grants. Both thresholds
    // fire when a catalyst raises the first trigger directly to level three.
    for item in items.iter().filter(|item| item.item_id == 8210411) {
        let thresholds = [
            (
                old_syntony < 2 && new_syntony >= 2 && item.syntony_in_range,
                82104112,
                6,
            ),
            (
                old_syntony < 3 && new_syntony >= 3,
                82104113,
                9_i32.saturating_add(item.count2).max(0),
            ),
        ];
        for (trigger, key, amount) in thresholds {
            if trigger
                && managers
                    .rouge
                    .active
                    .as_mut()
                    .unwrap()
                    .growth_triggers
                    .insert((item.uid, 0, key))
            {
                for entity in &pool.attacker_main {
                    if managers.hp.current(entity.uid) > 0 {
                        ops.push(grant_stacks(source_uid, entity.uid, 90001, amount));
                    }
                }
            }
        }
    }
    if old_syntony < 2 && new_syntony >= 2 {
        for item in items.iter().filter(|item| item.item_id == 8310407) {
            let limit = if item.support_in_range_count >= 4 {
                i32::MAX
            } else {
                5
            };
            if quota_available(managers, item.uid, 83104072, limit)
                && determinism.roll_permille(
                    item.proc_chance(400_i32.saturating_add(item.count2.saturating_mul(10))),
                )
            {
                increment_quota(managers, item.uid, 83104072);
                for entity in &pool.attacker_main {
                    if managers.hp.current(entity.uid) > 0 {
                        ops.push(grant_stacks(entity.uid, entity.uid, 90001, 12));
                    }
                }
            }
        }
    }
    if old_syntony < 5 && new_syntony >= 5 {
        for _ in items.iter().filter(|item| item.item_id == 8320408) {
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    ops.push(grant_stacks(entity.uid, entity.uid, 90201, 1));
                }
            }
        }
    }
    if old_syntony < 2 && new_syntony >= 2 {
        for item in &items {
            match item.item_id {
                8120406 => {
                    // One actual trigger per artefact per fight, even if a
                    // different incantation type later reaches level two.
                    let state = managers.rouge.active.as_mut().unwrap();
                    if state.growth_triggers.insert((item.uid, 0, 10002)) {
                        for entity in &pool.attacker_main {
                            if managers.hp.current(entity.uid) > 0 {
                                ops.push(grant_stacks(
                                    entity.uid,
                                    entity.uid,
                                    90281,
                                    2_i32.saturating_add(item.count2),
                                ));
                            }
                        }
                    }
                }
                8130403 => {
                    for enemy in &pool.defender_main {
                        if managers.hp.current(enemy.uid) > 0 {
                            ops.push(grant_stacks(source_uid, enemy.uid, 90101, 4));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if new_syntony > old_syntony {
        for item in items.iter().filter(|item| item.item_id == 8110410) {
            let chance = item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            if determinism.roll_permille(chance) {
                for enemy in &pool.defender_main {
                    if managers.hp.current(enemy.uid) > 0 {
                        ops.push(grant_stacks(source_uid, enemy.uid, 90061, 2));
                    }
                }
            }
        }
    }
    if new_syntony > old_syntony {
        // Multiple copies resolve in UID order, using each enemy's remaining
        // HP after earlier queued copies (local sequential-stacking policy).
        let mut sources: Vec<_> = items
            .iter()
            .filter(|item| item.item_id == 8330413)
            .collect();
        sources.sort_unstable_by_key(|item| item.uid);
        for enemy in &pool.defender_main {
            let mut remaining = managers.hp.current(enemy.uid).max(0);
            for _ in &sources {
                let amount = (i64::from(remaining) * 50 / 1000) as i32;
                if amount <= 0 {
                    break;
                }
                remaining = remaining.saturating_sub(amount);
                ops.push(RuleOp::Command(BattleCommand::Hp(
                    crate::engine::manager::hp::HpCommand::Lose(
                        crate::engine::manager::hp::HpLoss {
                            origin: ORIGIN,
                            source_uid: enemy.uid,
                            target_uid: enemy.uid,
                            amount,
                            config_effect: 0,
                            hurt: None,
                        },
                    ),
                )));
            }
        }
    }
    if new_syntony > old_syntony {
        for _ in items.iter().filter(|item| item.item_id == 8330412) {
            // Stable slot order breaks equal effective-attack ties locally.
            let best = pool
                .attacker_main
                .iter()
                .filter(|e| managers.hp.current(e.uid) > 0)
                .enumerate()
                .max_by_key(|(i, e)| {
                    (
                        managers.origin_attribute(e.uid, AttrId::Attack),
                        std::cmp::Reverse(*i),
                    )
                })
                .map(|(_, e)| e.uid);
            if let Some(uid) = best {
                ops.push(grant_stacks(uid, uid, 500212, 1));
            }
            if old_syntony < 3 && new_syntony >= 3 {
                for entity in &pool.attacker_main {
                    if managers.hp.current(entity.uid) > 0 {
                        ops.push(grant_stacks(entity.uid, entity.uid, 500212, 1));
                    }
                }
            }
        }
    }
    if new_syntony > old_syntony {
        for _ in items.iter().filter(|item| item.item_id == 8130404) {
            let enemies = pool
                .defender_main
                .iter()
                .filter(|entity| managers.hp.current(entity.uid) > 0)
                .map(|entity| entity.uid)
                .collect::<Vec<_>>();
            if let Some(index) = determinism.lua_random_index(enemies.len()) {
                ops.push(grant_stacks(source_uid, enemies[index], 90101, 10));
            }
        }
    }
    if new_resonance > old_resonance {
        for item in items.iter().filter(|item| item.item_id == 8120512) {
            let chance = item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            if determinism.roll_permille(chance) {
                for ally in &pool.attacker_main {
                    if managers.hp.current(ally.uid) > 0 {
                        ops.push(grant_stacks(source_uid, ally.uid, 90021, 2));
                    }
                }
            }
        }
    }
    if new_resonance > old_resonance {
        for item in items.iter().filter(|item| item.item_id == 8210508) {
            let amount = 2_i32
                .saturating_add(item.count4)
                .saturating_add(i32::from(item.non_resonance_in_range))
                .max(0);
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    ops.push(grant_stacks(source_uid, entity.uid, 90021, amount));
                }
            }
        }
    }
    if new_resonance > old_resonance {
        for _ in items.iter().filter(|item| item.item_id == 8320709) {
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    ops.push(grant_stacks(entity.uid, entity.uid, 90021, 1));
                    ops.push(grant_stacks(entity.uid, entity.uid, 91210030, 1));
                }
            }
        }
    }
    if new_resonance > old_resonance {
        for _ in items.iter().filter(|item| item.item_id == 8320506) {
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    for buff_id in [90001, 90031, 90021] {
                        ops.push(grant_stacks(entity.uid, entity.uid, buff_id, 1));
                    }
                }
            }
        }
    }

    if new_syntony > old_syntony {
        for _ in items.iter().filter(|item| item.item_id == 8120416) {
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    ops.push(grant_stacks(entity.uid, entity.uid, 90031, 5));
                    ops.push(grant_stacks(entity.uid, entity.uid, 90032, 5));
                }
            }
        }
    }
    if new_resonance > old_resonance {
        for item in &items {
            if item.item_id == 8130505 {
                for entity in &pool.defender_main {
                    if managers.hp.current(entity.uid) > 0 {
                        ops.push(grant_stacks(source_uid, entity.uid, 4150001, 10));
                    }
                }
            } else if item.item_id == 8330513 {
                let crossed_two = old_resonance < 2 && new_resonance >= 2;
                let count = if crossed_two { 4 } else { 2 };
                if crossed_two {
                    use crate::engine::manager::buff::{BuffConsume, BuffSelector, DepletedBuff};
                    for enemy in &pool.defender_main {
                        if managers.hp.current(enemy.uid) > 0 {
                            ops.push(RuleOp::Command(BattleCommand::Buff(
                                BuffCommand::ConsumeCoalesced(BuffConsume {
                                    origin: ORIGIN,
                                    target_uid: enemy.uid,
                                    selector: BuffSelector::ExactId(90021),
                                    amount: 2,
                                    depleted: DepletedBuff::Remove,
                                }),
                            )));
                        }
                    }
                }
                for entity in &pool.attacker_main {
                    if managers.hp.current(entity.uid) > 0 {
                        ops.push(grant_stacks(entity.uid, entity.uid, 90021, count));
                    }
                }
            }
        }
    }
    if new_resonance > old_resonance {
        for item in items.iter().filter(|item| item.item_id == 8210105) {
            let chance = item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            if !determinism.roll_permille(chance) {
                continue;
            }
            let allies: Vec<_> = pool
                .attacker_main
                .iter()
                .filter(|e| managers.hp.current(e.uid) > 0)
                .map(|e| e.uid)
                .collect();
            if let Some(index) = determinism.lua_random_index(allies.len()) {
                let uid = allies[index];
                // Current client carrier IDs: one-round Empower I/II and
                // next-round rank-1/2 Support anticipation respectively.
                let (empower, support) = if item.non_support_in_range {
                    (5024, 90332)
                } else {
                    (5021, 90331)
                };
                ops.push(grant_stacks(uid, uid, empower, 1));
                ops.push(grant_stacks(uid, uid, support, 1));
            }
        }
    }
    if new_resonance > old_resonance {
        for item in items.iter().filter(|item| item.item_id == 8320514) {
            // The group matches the current client status family. Four-turn
            // variants are unavailable for four families, so those cap at 3.
            // The four-status immunity is granted separately at battle entry.
            const GROUPS: [&[i32]; 5] = [
                &[90061, 90062, 90063],
                &[90231, 90232, 90233],
                &[90151, 90152, 90153, 90154],
                &[90221, 90222, 90223],
                &[90161, 90162, 90163],
            ];
            // Multiple skills and durations can use different buff IDs for
            // the same status; compare the client status type instead.
            const TYPES: [i32; 5] = [90061, 3120001, 6003, 6004, 6002];
            let duration = 1_i32
                .saturating_add(item.count2)
                .saturating_add(i32::from(item.non_resonance_in_range))
                .max(1) as usize;
            for enemy in &pool.defender_main {
                if managers.hp.current(enemy.uid) <= 0 {
                    continue;
                }
                let missing: Vec<usize> = GROUPS
                    .iter()
                    .enumerate()
                    .filter_map(|(index, _ids)| {
                        (!lantern_status_present(managers, enemy.uid, TYPES[index], &ops))
                            .then_some(index)
                    })
                    .collect();
                let candidates: Vec<usize> = if missing.is_empty() {
                    (0..GROUPS.len()).collect()
                } else {
                    missing
                };
                if let Some(index) = determinism.lua_random_index(candidates.len()) {
                    let group = GROUPS[candidates[index]];
                    ops.push(grant_stacks(
                        source_uid,
                        enemy.uid,
                        group[duration.min(group.len()) - 1],
                        1,
                    ));
                }
            }
        }
    }
    let mut next_ap = 0_i32;
    if new_syntony > old_syntony {
        next_ap = next_ap.saturating_add(
            items
                .iter()
                .filter(|item| matches!(item.item_id, 8230414 | 8330417))
                .count()
                .min(i32::MAX as usize) as i32,
        );
    }
    if new_resonance > old_resonance {
        next_ap = next_ap.saturating_add(
            items
                .iter()
                .filter(|item| matches!(item.item_id, 8230509 | 8330417))
                .count()
                .min(i32::MAX as usize) as i32,
        );
    }
    super::queue_next_round_action_points(managers, next_ap);
    // Resonance random buffs share the same pending-buff view, so a later
    // source still prefers effects that earlier queued draws have not granted.
    if new_resonance > old_resonance {
        for item in &items {
            let draws = match item.item_id {
                8130503 if old_resonance < 2 && new_resonance >= 2 => 1,
                8130504 => 1,
                8220516 => 1_i32
                    .saturating_add(item.count2)
                    .saturating_add(i32::from(item.non_resonance_in_range)),
                8320511 => 1_i32
                    .saturating_add(item.count2)
                    .saturating_add(item.resonance_artefact_count),
                _ => continue,
            }
            .clamp(0, 1024) as usize;
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    let extra = super::artefact_turns::permanent_buffs_with_pending(
                        managers,
                        entity.uid,
                        draws,
                        determinism,
                        &ops,
                    );
                    ops.extend(extra);
                }
            }
        }
    }
    // Each bomb has independent once-per-fight syntony and resonance grants.
    // Counters are replayed with the battle and never reset between rounds.
    for item in &items {
        if item.item_id == 8230614 {
            for (triggered, key) in [
                (new_syntony > old_syntony, 61001),
                (new_resonance > old_resonance, 61002),
            ] {
                if triggered
                    && managers
                        .rouge
                        .active
                        .as_mut()
                        .unwrap()
                        .growth_triggers
                        .insert((item.uid, 0, key))
                {
                    ops.extend(super::catalyst::coin_change(managers, 20));
                }
            }
        } else if item.item_id == 8310510 && new_resonance > old_resonance {
            ops.extend(super::catalyst::coin_change(
                managers,
                5_i32
                    .saturating_add(item.count2)
                    .saturating_add(if item.non_resonance_in_range { 5 } else { 0 }),
            ));
            for entity in &pool.attacker_main {
                if managers.hp.current(entity.uid) > 0 {
                    ops.push(grant_stacks(source_uid, entity.uid, 90021, 1));
                }
            }
        }
    }

    if new_syntony > old_syntony || new_resonance > old_resonance {
        update_round_attributes(managers, pool);
    }
    ops
}

/// Observe actual committed bullet count changes, not casts or previews.
/// The last observed amount suppresses duplicate/stale notifications while an
/// intervening real refill resets the observed level for subsequent losses.
pub(super) fn observe_bullet_change(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    event: &BattleEvent,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
) {
    let (change, added) = match event {
        BattleEvent::BuffAdded(change) => (change, true),
        BattleEvent::BuffChanged(change) | BattleEvent::BuffRemoved(change) => (change, false),
        _ => return,
    };
    if !pool.source_is_attacker(change.target_uid)
        || change.buff_uid == 0
        || !managers
            .catalog()
            .buff_features(change.buff_id)
            .iter()
            .any(|feature| feature.values.first() == Some(&827))
    {
        return;
    }
    let Some(state) = managers.rouge.active.as_mut() else {
        return;
    };
    if added {
        state
            .bullet_observed_amounts
            .insert(change.buff_uid, change.after_amount);
        return;
    }
    if let Some(previous) = state.bullet_observed_amounts.get(&change.buff_uid) {
        if *previous != change.before_amount {
            return;
        }
    }
    state
        .bullet_observed_amounts
        .insert(change.buff_uid, change.after_amount);
    let lost = change
        .before_amount
        .saturating_sub(change.after_amount)
        .max(0);
    if lost == 0 || managers.hp.current(change.target_uid) <= 0 {
        return;
    }
    let items = managers
        .rouge
        .context()
        .map(|c| c.artefacts.clone())
        .unwrap_or_default();
    for item in items.iter().filter(|item| item.item_id == 8220415) {
        let limit = 2_i32
            .saturating_add(item.count4)
            .saturating_add(if item.syntony_in_range_count > 0 {
                2
            } else {
                0
            })
            .max(0);
        for _ in 0..lost.min(100) {
            if !quota_available(managers, item.uid, 8220415, limit) {
                break;
            }
            if determinism.roll_permille(
                item.proc_chance(800_i32.saturating_add(item.count2.saturating_mul(10))),
            ) {
                queue_syntony_bonus(managers, item.uid, 8220415, 1);
            }
        }
    }
}

fn quota_available(managers: &BattleManagers, uid: i64, key: i32, limit: i32) -> bool {
    managers
        .rouge
        .active
        .as_ref()
        .and_then(|s| s.artefact_trigger_counts.get(&(uid, key)))
        .copied()
        .unwrap_or_default()
        < limit
}
fn increment_quota(managers: &mut BattleManagers, uid: i64, key: i32) {
    if let Some(state) = managers.rouge.active.as_mut() {
        let count = state.artefact_trigger_counts.entry((uid, key)).or_default();
        *count = count.saturating_add(1);
    }
}
fn queue_syntony_bonus(managers: &mut BattleManagers, uid: i64, key: i32, amount: i32) {
    if let Some(state) = managers.rouge.active.as_mut() {
        let count = state.artefact_trigger_counts.entry((uid, key)).or_default();
        *count = count.saturating_add(1);
        state.next_syntony_bonus = state.next_syntony_bonus.saturating_add(amount.max(0));
    }
}

/// Both active statuses and queued grants are matched by the client's type.
pub(super) fn lantern_status_present(
    managers: &BattleManagers,
    uid: i64,
    type_id: i32,
    pending: &[RuleOp],
) -> bool {
    managers.buff.has_active_buff_id_or_type(uid, type_id)
        || pending.iter().any(|op| {
            matches!(op,
            RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(grant)))
                if grant.target_uid == uid
                    && managers.catalog().buff_type_id(grant.buff_id) == type_id)
        })
}

fn grant_stacks(source_uid: i64, target_uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid,
        target_uid,
        buff_id,
        amount: Some(amount.max(0)),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn level_marker(target_uid: i64, effect_type: i32, level: i32, kind: i32) -> RuleOp {
    // FightWorkPolarizationLevel indexes configEffect by incantation type;
    // FightWorkResonanceLevel reads the numeric effectNum directly.
    RuleOp::EffectMarker {
        target_uid,
        effect_type,
        effect_num: level,
        config_effect: kind,
        reserve_id: None,
        reserve_str: None,
    }
}

fn level_value(level: i32, values: [i32; 7]) -> i32 {
    usize::try_from(level - 1)
        .ok()
        .and_then(|index| values.get(index))
        .copied()
        .unwrap_or_default()
}

fn add_rate(rates: &mut BTreeMap<i32, i32>, attr: AttrId, value: i32) {
    let total = rates.entry(attr.id()).or_default();
    *total = total.saturating_add(value);
}

fn add_attack_defense(rates: &mut BTreeMap<i32, i32>, attack: i32, defense: i32) {
    // The engine keeps the flat base separately. These normal attribute
    // lanes are per-mille modifiers, matching damage/healing and shields.
    add_rate(rates, AttrId::Attack, attack);
    add_rate(rates, AttrId::RealityDef, defense);
    add_rate(rates, AttrId::MentalDef, defense);
}

fn update_round_attributes(managers: &mut BattleManagers, pool: &TargetPool) {
    let state = managers.rouge.active.as_ref().unwrap();
    let mut rates = BTreeMap::new();
    // Exact values from the shipped polarization/resonance descriptions.
    for (&kind, &level) in &state.incantation.levels {
        let attack = level_value(level, [50, 100, 200, 300, 400, 600, 800]);
        match kind {
            1 => add_attack_defense(&mut rates, attack, 0),
            3 | 4 => {
                let rate = level_value(level, [30, 60, 120, 180, 240, 360, 480]);
                add_attack_defense(&mut rates, rate, rate);
            }
            5 => add_attack_defense(
                &mut rates,
                level_value(level, [20, 50, 100, 150, 200, 300, 400]),
                attack,
            ),
            6 => add_rate(&mut rates, AttrId::HealingDone, attack),
            8 => add_attack_defense(&mut rates, attack, attack),
            9 => add_rate(
                &mut rates,
                AttrId::DmgTakenReduction,
                level_value(level, [80, 160, 320, 480, 640, 960, 1280]),
            ),
            _ => {}
        }
    }
    let resonance = level_value(
        state.incantation.resonance_level,
        [40, 80, 160, 240, 320, 480, 640],
    );
    add_attack_defense(&mut rates, resonance, resonance);
    let mut desired = BTreeMap::new();
    for uid in pool
        .attacker_all
        .iter()
        .map(|entity| entity.uid)
        .collect::<BTreeSet<_>>()
    {
        for (&attr, &value) in &rates {
            desired.insert((uid, attr), value);
        }
    }
    let previous = std::mem::replace(
        &mut managers
            .rouge
            .active
            .as_mut()
            .unwrap()
            .incantation
            .attributes,
        desired.clone(),
    );
    for key in previous
        .keys()
        .chain(desired.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let delta = desired
            .get(&key)
            .copied()
            .unwrap_or_default()
            .saturating_sub(previous.get(&key).copied().unwrap_or_default());
        if delta != 0
            && let Some(attr) = AttrId::from_raw(key.1)
        {
            managers.attribute.add(key.0, attr, delta);
        }
    }
}

pub(crate) fn end_round(managers: &mut BattleManagers) -> Vec<RuleOp> {
    super::catalyst::clear_round_boosts(managers);
    let Some(state) = managers.rouge.active.as_mut() else {
        return Vec::new();
    };
    let previous = std::mem::take(&mut state.incantation);
    let mut markers = previous
        .levels
        .keys()
        .map(|&kind| level_marker(0, 183, 0, kind))
        .collect::<Vec<_>>();
    if previous.resonance_level > 0 {
        markers.push(level_marker(0, 184, 0, 0));
    }
    state.current_syntony_bonus = std::mem::take(&mut state.next_syntony_bonus);
    for ((uid, raw), value) in previous.attributes {
        if let Some(attr) = AttrId::from_raw(raw) {
            managers.attribute.add(uid, attr, value.saturating_neg());
        }
    }
    markers
}
