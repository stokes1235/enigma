//! Frozen base-Rouge difficulty, including enemies registered after opening.
//! The frozen local HP/step approximation also applies to later waves.

use super::*;
use crate::engine::{
    manager::card::{CardCommand, CardEnchantHand, EnchantedType},
    runtime::determinism::RoundDeterminism,
};
use sonettobuf::FightEntityInfo;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Difficulty {
    pub attack_growth_start_round: i32,
    pub attack_growth_per_round: i32,
    pub scalding_cards: i32,
    pub stronger_afflatus: i32,
    pub extra_enemy_ap: i32,
    pub enemy_attributes: Vec<(i32, i32)>,
    /// These enemies already include static difficulty in the frozen Fight.
    pub initial_enemy_uids: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Runtime {
    seen_enemies: BTreeSet<i64>,
    last_round: Option<i32>,
}

pub(crate) fn configure(managers: &mut BattleManagers) {
    let Some(state) = managers.rouge.active.as_mut() else {
        return;
    };
    state.difficulty = Runtime {
        seen_enemies: state
            .context
            .difficulty
            .as_ref()
            .map(|profile| profile.initial_enemy_uids.iter().copied().collect())
            .unwrap_or_default(),
        ..Default::default()
    };
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

/// Called after plain enemy registration and before its entry passives. Build
/// the same static combat baseline as the initial Fight; a later round's5%
/// growth therefore has the same basis on an opening enemy or a wave newcomer.
pub(crate) fn register_enemy(managers: &mut BattleManagers, uid: i64) {
    if managers.entity.team_type(uid) != Some(2) {
        return;
    }
    let Some(mut entity) = managers
        .entity
        .snapshot(uid)
        .filter(|entity| entity.entity_type == Some(2) && entity.model_id.unwrap_or(0) > 0)
    else {
        return;
    };
    let Some(state) = managers.rouge.active.as_mut() else {
        return;
    };
    let Some(profile) = state.context.difficulty.clone() else {
        return;
    };
    if !state.difficulty.seen_enemies.insert(uid) {
        return;
    }
    let mut rates = BTreeMap::<i32, i32>::new();
    for (id, value) in profile.enemy_attributes {
        *rates.entry(id).or_default() += value;
    }
    let old_max = entity.attr.as_ref().and_then(|attr| attr.hp);
    for attr in [&mut entity.attr, &mut entity.base_attr]
        .into_iter()
        .flatten()
    {
        scale(&mut attr.hp, rates.get(&101).copied().unwrap_or(0));
        scale(&mut attr.attack, rates.get(&102).copied().unwrap_or(0));
        scale(&mut attr.defense, rates.get(&103).copied().unwrap_or(0));
        scale(&mut attr.mdefense, rates.get(&104).copied().unwrap_or(0));
    }
    scale_current_hp(&mut entity, old_max, rates.get(&101).copied().unwrap_or(0));
    if rates.get(&101).copied().unwrap_or(0) != 0 {
        // This is a newly registered enemy; rebuild its HP state from the
        // scaled snapshot before entry passives can observe current/max HP.
        managers.hp.register(&entity);
    }
    // register_with_catalog also initializes these six lanes. Preserve any
    // explicit values already seeded for the new enemy before rebasing stats.
    let preserved = [
        AttrId::CriticalRate,
        AttrId::CriticalResistRate,
        AttrId::CriticalDmg,
        AttrId::CriticalDef,
        AttrId::DmgBonus,
        AttrId::DmgTakenReduction,
    ]
    .map(|attr| (attr, managers.attribute.get(uid, attr)));
    let catalog = managers.catalog();
    managers.attribute.register_with_catalog(catalog, &entity);
    for (attr, value) in preserved {
        managers.attribute.add(
            uid,
            attr,
            value.saturating_sub(managers.attribute.get(uid, attr)),
        );
    }
    managers.entity.update(entity);
    for (id, value) in rates {
        if matches!(id, 205..=208)
            && let Some(attr) = AttrId::from_raw(id)
        {
            managers.attribute.add(uid, attr, value);
        }
    }
}

pub(crate) fn enemy_ap_bonus(managers: &BattleManagers) -> i32 {
    managers
        .rouge
        .context()
        .and_then(|context| context.difficulty.as_ref())
        .map(|profile| profile.extra_enemy_ap.max(0))
        .unwrap_or(0)
}

/// The damage formula calls this only for a stronger-afflatus hit. It is not
/// an unconditional damage bonus and does not change weaker-afflatus damage.
pub(crate) fn stronger_afflatus_bonus(managers: &BattleManagers, source_uid: i64) -> i32 {
    if managers.entity.team_type(source_uid) != Some(2) {
        return 0;
    }
    managers
        .rouge
        .context()
        .and_then(|context| context.difficulty.as_ref())
        .map(|profile| profile.stronger_afflatus.max(0))
        .unwrap_or(0)
}

/// Run after the round's hand has been dealt, once per increasing round number.
/// Local timing: grow all present living enemies (including reserve), and
/// choose distinct live allied hand cards with no existing surface enchant.
/// Later-wave arrivals do not retroactively earn growth from missed rounds.
pub(crate) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    deterministic: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    let Some(profile) = state.context.difficulty.clone() else {
        return vec![];
    };
    if round <= 0
        || state
            .difficulty
            .last_round
            .is_some_and(|seen| seen >= round)
    {
        return vec![];
    }
    state.difficulty.last_round = Some(round);
    if profile.attack_growth_start_round > 0
        && round >= profile.attack_growth_start_round
        && profile.attack_growth_per_round > 0
    {
        for enemy in &pool.defender_all {
            if managers.hp.current(enemy.uid) > 0 {
                managers
                    .attribute
                    .add(enemy.uid, AttrId::Attack, profile.attack_growth_per_round);
            }
        }
    }
    let mut candidates: Vec<usize> = managers
        .card
        .hand()
        .iter()
        .enumerate()
        .filter(|(_, card)| {
            card.enchants.is_empty()
                && card.skill_id.is_some_and(|id| id > 0)
                && card.uid.is_some_and(|uid| {
                    managers.entity.team_type(uid) == Some(1) && managers.hp.current(uid) > 0
                })
        })
        .map(|(index, _)| index)
        .collect();
    let mut selected = Vec::new();
    for _ in 0..(profile.scalding_cards.clamp(0, 2) as usize).min(candidates.len()) {
        let Some(index) = deterministic.card_random_index(candidates.len()) else {
            break;
        };
        selected.push(candidates.swap_remove(index));
    }
    if selected.is_empty() {
        return vec![];
    }
    selected.sort_unstable();
    vec![RuleOp::Command(BattleCommand::Card(
        CardCommand::EnchantHand(CardEnchantHand {
            origin: CommandOrigin {
                domain: RuleDomain::Lifecycle,
                key: DefinitionKey::new(0, "RougeDifficulty"),
            },
            indices: selected,
            enchant: EnchantedType::Burn,
            duration: -1,
            team_type: 1,
        }),
    ))]
}

#[cfg(test)]
#[path = "difficulty_tests.rs"]
mod tests;
