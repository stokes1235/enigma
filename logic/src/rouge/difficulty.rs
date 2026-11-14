//! Confirmed cumulative original-Rouge difficulty rules.
//!
//! Sources: `rouge_difficulty.desc/initCollections`, the saved main Wiki's
//! twenty-row difficulty table, and the client's cumulative difficulty view.
//! Numeric percentages of the same attribute add on the existing local monster
//! template baseline; this rounding/stacking policy is explicit local behavior.
//! The exported `rouge_layer_difficulty.stepAttr` lacks its server algorithm.
//! Difficulty1 packet samples also disagree with a direct table multiplier, so
//! Its server formula remains unknown. The deliberately bounded local HP and
//! progression approximation below is not presented as an official formula.

use anyhow::{Result, ensure};
use database::models::game::rouge::{RougeRun, RougeStart};
use serde::{Deserialize, Serialize};
use sonettobuf::{Fight, FightEntityInfo, HeroExAttribute, HeroSpAttribute};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encounter {
    MinorObstacle,
    PresentDanger,
    Crisis,
    FinalBoss,
    Other,
}

impl Encounter {
    pub fn from_event_type(kind: i32) -> Self {
        match kind {
            1 => Self::MinorObstacle,
            2 => Self::PresentDanger,
            3 | 4 => Self::Crisis,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DynamicRules {
    pub attack_growth_start_round: i32,
    /// Per-mille ATK increase each qualifying round.
    pub attack_growth_per_round: i32,
    pub scalding_cards: i32,
    pub stronger_afflatus: i32,
    pub extra_enemy_ap: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub difficulty: i32,
    /// Per-mille increments, not replacements for the monster's base values.
    pub enemy_attributes: BTreeMap<i32, i32>,
    pub dynamic: DynamicRules,
    pub initial_collections: Vec<i32>,
}

/// Tables prove the thresholds and values. The same-kind additive policy
/// avoids successively multiplying cumulative percentage descriptions.
pub fn profile(tables: &config::GameDB, difficulty: i32, encounter: Encounter) -> Result<Profile> {
    let row = tables
        .rouge_difficulty(difficulty)
        .filter(|row| row.season == 1 && row.version.is_empty())
        .ok_or_else(|| anyhow::anyhow!("unknown original Rouge difficulty {difficulty}"))?;
    let mut result = Profile {
        difficulty,
        initial_collections: tables
            .rouge_difficulty_initial_collections(row.difficulty)
            .ok_or_else(|| anyhow::anyhow!("invalid Rouge initial noise collections"))?,
        ..Default::default()
    };
    for threshold in tables
        .rouge_difficulty
        .iter()
        .filter(|row| row.season == 1 && row.version.is_empty() && row.difficulty <= difficulty)
    {
        let values: &[(i32, i32)] = match threshold.difficulty {
            2 => &[(205, 150)],
            3 => &[(206, 150)],
            5 => &[(102, 150)],
            7 => &[(205, 200), (207, 100)],
            10 => &[(102, 150), (103, 150), (104, 150)],
            14 => &[(206, 200), (208, 100)],
            15 => &[(102, 250), (103, 250), (104, 250)],
            20 => &[(207, 400), (208, 400)],
            _ => &[],
        };
        for (attr, amount) in values {
            *result.enemy_attributes.entry(*attr).or_default() += amount;
        }
    }
    if difficulty >= 9 {
        result.dynamic.attack_growth_start_round = 6;
        result.dynamic.attack_growth_per_round = 50;
    }
    result.dynamic.scalding_cards = i32::from(difficulty >= 11) + i32::from(difficulty >= 16);
    result.dynamic.stronger_afflatus = if difficulty >= 12 { 300 } else { 0 };
    result.dynamic.extra_enemy_ap = match encounter {
        Encounter::MinorObstacle | Encounter::PresentDanger => i32::from(difficulty >= 17),
        Encounter::Crisis | Encounter::FinalBoss => i32::from(difficulty >= 19),
        Encounter::Other => 0,
    };
    Ok(result)
}

pub fn for_run(tables: &config::GameDB, start: &RougeStart, run: &RougeRun) -> Result<Profile> {
    ensure!(
        start.season == 1 && run.season == 1,
        "invalid Rouge difficulty season"
    );
    let encounter = if run.map_type == 2 {
        Encounter::FinalBoss
    } else {
        let graph: super::map::LayerMap = serde_json::from_str(&run.graph)?;
        graph
            .node(run.node_id)
            .map(|node| Encounter::from_event_type(node.node_type))
            .ok_or_else(|| anyhow::anyhow!("Rouge difficulty has no current node"))?
    };
    let mut profile = profile(tables, start.difficulty, encounter)?;
    apply_run_progression(&mut profile, run);
    Ok(profile)
}

/// The pre-fight monster panel only needs HP, whose local calculation is
/// independent of encounter type and does not require a selected map node.
pub fn hp_rate_for_run(tables: &config::GameDB, start: &RougeStart, run: &RougeRun) -> Result<i32> {
    ensure!(
        start.season == 1 && run.season == 1,
        "invalid Rouge difficulty season"
    );
    if start.difficulty == 0 {
        return Ok(0); // difficulty has not yet been selected
    }
    let mut profile = profile(tables, start.difficulty, Encounter::Other)?;
    apply_run_progression(&mut profile, run);
    Ok(profile.enemy_attributes.get(&101).copied().unwrap_or(0))
}

fn apply_run_progression(profile: &mut Profile, run: &RougeRun) {
    // Final-boss legacy saves can have an empty graph. Step count itself only
    // needs the same resolved-node union as settlement, not graph metadata.
    let current_steps = super::current_resolved_nodes(run)
        .len()
        .min(i32::MAX as usize) as i32;
    apply_local_progression(profile, run.step_count.saturating_add(current_steps));
}

/// Temporary balancing policy, independent of the exported `stepAttr` rows:
/// HP gains 2.5% per difficulty above 1 plus 1% per resolved node, while ATK
/// gains 0.5% per node. Cap the counted nodes at 30 and HP growth at 100%.
/// The run's archived steps and current-layer resolved nodes share the same
/// accounting as the result screen, and are frozen when the fight is built.
fn apply_local_progression(profile: &mut Profile, completed_steps: i32) {
    const MAX_STEPS: i32 = 30;
    const HP_PER_DIFFICULTY: i32 = 25;
    const HP_PER_STEP: i32 = 10;
    const ATTACK_PER_STEP: i32 = 5;
    const MAX_HP_BONUS: i32 = 1000;
    let steps = completed_steps.clamp(0, MAX_STEPS);
    let hp = (profile.difficulty.saturating_sub(1).clamp(0, 19) * HP_PER_DIFFICULTY
        + steps * HP_PER_STEP)
        .min(MAX_HP_BONUS);
    if hp > 0 {
        *profile.enemy_attributes.entry(101).or_default() += hp;
    }
    if steps > 0 {
        *profile.enemy_attributes.entry(102).or_default() += steps * ATTACK_PER_STEP;
    }
}

/// Freeze the profile and the already-scaled enemy UIDs, so reconnect and new
/// wave registration cannot apply the initial static modifier a second time.
pub fn runtime_context(profile: &Profile, fight: &Fight) -> Option<battle::rouge::Difficulty> {
    if profile.enemy_attributes.is_empty() && profile.dynamic == DynamicRules::default() {
        return None;
    }
    Some(battle::rouge::Difficulty {
        attack_growth_start_round: profile.dynamic.attack_growth_start_round,
        attack_growth_per_round: profile.dynamic.attack_growth_per_round,
        scalding_cards: profile.dynamic.scalding_cards,
        stronger_afflatus: profile.dynamic.stronger_afflatus,
        extra_enemy_ap: profile.dynamic.extra_enemy_ap,
        enemy_attributes: profile
            .enemy_attributes
            .iter()
            .map(|(id, value)| (*id, *value))
            .collect(),
        initial_enemy_uids: fight
            .defender
            .iter()
            .flat_map(|team| {
                team.entitys
                    .iter()
                    .chain(&team.sub_entitys)
                    .chain(&team.sp_entitys)
                    .chain(&team.sp_fight_entities)
            })
            .filter(|entity| entity.entity_type == Some(2) && entity.model_id.unwrap_or(0) > 0)
            .filter_map(|entity| entity.uid)
            .collect(),
    })
}

fn base_damage_rates(tables: &config::GameDB, entity: &FightEntityInfo) -> (i32, i32) {
    let Some(monster) = entity.model_id.and_then(|id| tables.monster.get(id)) else {
        return (0, 0);
    };
    let template = if monster.template != 0 {
        monster.template
    } else {
        monster.id
    };
    let level = entity.level.unwrap_or_else(|| {
        if monster.level_true != 0 {
            monster.level_true
        } else {
            monster.level
        }
    });
    tables
        .monster_template
        .iter()
        .find(|row| row.template == template)
        .map(|row| {
            (
                row.add_dmg
                    .saturating_add(row.add_dmg_grow.saturating_mul(level)),
                row.drop_dmg
                    .saturating_add(row.drop_dmg_grow.saturating_mul(level)),
            )
        })
        .unwrap_or_default()
}

fn scale(value: &mut Option<i32>, rate: i32) {
    if rate != 0
        && let Some(base) = value
    {
        *base = ((*base as i64 * (1000_i64 + rate as i64).max(0)) / 1000).clamp(0, i32::MAX as i64)
            as i32;
    }
}

fn scale_current_hp(entity: &mut FightEntityInfo, old_max: Option<i32>, rate: i32) {
    if rate == 0 {
        return;
    }
    match (
        old_max,
        entity.attr.as_ref().and_then(|attr| attr.hp),
        entity.current_hp,
    ) {
        (Some(before), Some(after), Some(current)) if before > 0 => {
            entity.current_hp =
                Some(((current.clamp(0, before) as i64 * after as i64) / before as i64) as i32);
        }
        _ => scale(&mut entity.current_hp, rate),
    }
}

/// Apply once to a newly built fight, before freezing it for runtime/reconnect.
/// Main, reserve and special defender monsters are included. Placeholder team
/// entities and every attacker remain untouched. HP and current HP both scale,
/// preserving the health fraction with integer rounding.
/// Runtime must apply the frozen profile separately to later-wave newcomers.
pub fn apply_to_defenders(
    tables: &config::GameDB,
    fight: &mut Fight,
    ex: &mut Vec<(i64, HeroExAttribute)>,
    sp: &mut Vec<(i64, HeroSpAttribute)>,
    profile: &Profile,
) -> usize {
    if profile.enemy_attributes.is_empty() {
        return 0;
    }
    let Some(defender) = &mut fight.defender else {
        return 0;
    };
    let rate = |id| profile.enemy_attributes.get(&id).copied().unwrap_or(0);
    let mut count = 0;
    for entity in defender
        .entitys
        .iter_mut()
        .chain(&mut defender.sub_entitys)
        .chain(&mut defender.sp_entitys)
        .chain(&mut defender.sp_fight_entities)
        .filter(|entity| entity.entity_type == Some(2) && entity.model_id.unwrap_or(0) > 0)
    {
        let old_max = entity.attr.as_ref().and_then(|attr| attr.hp);
        for attr in [&mut entity.attr, &mut entity.base_attr]
            .into_iter()
            .flatten()
        {
            scale(&mut attr.hp, rate(101));
            scale(&mut attr.attack, rate(102));
            scale(&mut attr.defense, rate(103));
            scale(&mut attr.mdefense, rate(104));
        }
        scale_current_hp(entity, old_max, rate(101));
        let Some(uid) = entity.uid else {
            continue;
        };
        let (base_add, base_drop) = base_damage_rates(tables, entity);
        if rate(205) != 0 || rate(206) != 0 {
            if !ex.iter().any(|(id, _)| *id == uid) {
                ex.push((uid, HeroExAttribute::default()));
            }
            let attr = &mut ex.iter_mut().find(|(id, _)| *id == uid).unwrap().1;
            if rate(205) != 0 {
                attr.add_dmg = Some(attr.add_dmg.unwrap_or(base_add).saturating_add(rate(205)));
            }
            if rate(206) != 0 {
                attr.drop_dmg = Some(attr.drop_dmg.unwrap_or(base_drop).saturating_add(rate(206)));
            }
        }
        if rate(207) != 0 || rate(208) != 0 {
            if !sp.iter().any(|(id, _)| *id == uid) {
                sp.push((uid, HeroSpAttribute::default()));
            }
            let attr = &mut sp.iter_mut().find(|(id, _)| *id == uid).unwrap().1;
            if rate(207) != 0 {
                attr.final_add_dmg =
                    Some(attr.final_add_dmg.unwrap_or(0).saturating_add(rate(207)));
            }
            if rate(208) != 0 {
                attr.final_drop_dmg =
                    Some(attr.final_drop_dmg.unwrap_or(0).saturating_add(rate(208)));
            }
        }
        count += 1;
    }
    count
}

#[cfg(test)]
#[path = "difficulty_tests.rs"]
mod tests;
