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
    let Some(rules) = managers
        .rouge
        .context()
        .and_then(|context| context.incantation_rules.clone())
    else {
        return vec![];
    };
    // Local timing policy: only actually completed, selected basic/Aid cards
    // count. Ultimates, passive/extra actions and unexecuted queued cards do
    // not fabricate syntony or permanent growth. A new level benefits later
    // actions, not the action which has already completed.
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
    if !selected
        || ultimate
        || crate::engine::skill::action::performs_extra_action(
            action.card_index,
            action.mode,
            action.extra_skill_kind,
        )
    {
        return vec![];
    }
    // Reality/Mental are both the Attack card type. `skill_type` is the
    // active/passive discriminator and is not the syntony category.
    let kind = match managers.catalog().skill_effect_tag(action.skill_id) {
        1 | 2 => 1,
        tag => tag,
    };
    // These seven types are the shipped base polarization table's keys.
    if !matches!(kind, 1 | 3 | 4 | 5 | 6 | 8 | 9) {
        return vec![];
    }
    let state = managers.rouge.active.as_mut().unwrap();
    let incantation = &mut state.incantation;
    let count = incantation.casts_by_type.entry(kind).or_default();
    *count = count.saturating_add(1);
    let new_syntony = if *count >= 3 {
        (*count - 2).min(rules.syntony_level_limit.clamp(1, 7))
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
        (types - 2).min(rules.resonance_level_limit.clamp(1, 7))
    } else {
        0
    };
    let old_resonance = incantation.resonance_level;
    incantation.resonance_level = new_resonance;
    state.summary.resonance_max_level = state.summary.resonance_max_level.max(new_resonance);

    let mut ops = Vec::new();
    let target = pool
        .attacker_main
        .first()
        .map_or(action.source_uid, |entity| entity.uid);
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
    if new_syntony > old_syntony || new_resonance > old_resonance {
        update_round_attributes(managers, pool);
    }
    ops
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

pub(crate) fn end_round(managers: &mut BattleManagers) {
    let Some(state) = managers.rouge.active.as_mut() else {
        return;
    };
    let previous = std::mem::take(&mut state.incantation);
    for ((uid, raw), value) in previous.attributes {
        if let Some(attr) = AttrId::from_raw(raw) {
            managers.attribute.add(uid, attr, value.saturating_neg());
        }
    }
}
