//! Original-Rouge catalyst snapshot and deterministic live resource state.
//! Magic is the second exTeamStr field, independent of ordinary cloth power.
use super::*;
use crate::engine::{
    manager::{
        card::{CardAddTemporary, CardAddUniversal, CardCommand, TemporaryCardKind},
        ex_point::{ExPointChange, ExPointCommand, ExPointSet},
    },
    runtime::determinism::RoundDeterminism,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalyst {
    pub style_id: i32,
    pub coin: i32,
    pub power: i32,
    pub power_limit: i32,
    /// One-based deployed slots with their actual combat actor UIDs.
    pub slots: Vec<(i32, i64)>,
    /// Frozen layout counts, computed by the server's shared layout policy.
    pub disk_count: i32,
    pub epic_count: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportUltimate {
    pub owner_uid: i64,
    pub hero_id: i32,
    pub skill_id: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Resources {
    pub coin: i32,
    pub power: i32,
    pub power_limit: i32,
    /// Gross consumption, distinct from balance deltas after any recovery.
    pub spent_coin: i32,
    pub spent_power: i32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    pub(super) resources: Resources,
    pub(super) round_uses: BTreeMap<(i32, i32), i32>,
    pub(super) total_uses: BTreeMap<i32, i32>,
    pub(super) passive_round: Option<i32>,
    pub(super) syntony_boost: i32,
    pub(super) resonance_boost: i32,
}

fn origin(opcode: i32, name: &'static str) -> CommandOrigin {
    CommandOrigin {
        domain: RuleDomain::Behavior,
        key: DefinitionKey::new(opcode, name),
    }
}

pub(crate) fn marker(effect_type: i32, delta: i32) -> RuleOp {
    RuleOp::EffectMarker {
        target_uid: 0,
        effect_type,
        effect_num: delta,
        config_effect: 0,
        reserve_id: Some(0),
        reserve_str: None,
    }
}

/// Applies actual resource changes once. Insufficient negative balances fail
/// without clamping away a cost. Recovery is capped, and packets show only the
/// applied delta. The outer battle transaction owns rollback on later errors.
pub(crate) fn try_change_resources(
    managers: &mut BattleManagers,
    coin_delta: i32,
    power_delta: i32,
) -> Option<Vec<RuleOp>> {
    let state = managers.rouge.active.as_mut()?.catalyst.as_mut()?;
    let coin = i64::from(state.resources.coin) + i64::from(coin_delta);
    let power = i64::from(state.resources.power) + i64::from(power_delta);
    if coin < 0 || power < 0 {
        return None;
    }
    let coin = coin.min(i64::from(i32::MAX)) as i32;
    let power = power.min(i64::from(state.resources.power_limit)) as i32;
    let applied_coin = coin - state.resources.coin;
    let applied_power = power - state.resources.power;
    state.resources.coin = coin;
    state.resources.power = power;
    state.resources.spent_coin = state
        .resources
        .spent_coin
        .saturating_add(applied_coin.saturating_neg().max(0));
    state.resources.spent_power = state
        .resources
        .spent_power
        .saturating_add(applied_power.saturating_neg().max(0));
    let mut result = Vec::new();
    if applied_power != 0 {
        result.push(marker(189, applied_power));
    }
    if applied_coin != 0 {
        result.push(marker(190, applied_coin));
    }
    if applied_coin < 0 {
        result.extend(super::artefact_coin_growth::spend(
            managers,
            applied_coin.saturating_neg(),
        ));
    }
    Some(result)
}

pub(crate) fn power_change(managers: &mut BattleManagers, delta: i32) -> Vec<RuleOp> {
    try_change_resources(managers, 0, delta).unwrap_or_default()
}

pub(crate) fn coin_change(managers: &mut BattleManagers, delta: i32) -> Vec<RuleOp> {
    try_change_resources(managers, delta, 0).unwrap_or_default()
}

pub(crate) fn sync_team(managers: &BattleManagers, fight: &mut sonettobuf::Fight) {
    let Some(resources) = managers.rouge.resources() else {
        return;
    };
    let Some(team) = fight.attacker.as_mut() else {
        return;
    };
    let suffix = team
        .ex_team_str
        .as_deref()
        .and_then(|text| text.splitn(4, '#').nth(3))
        .unwrap_or("{}");
    team.ex_team_str = Some(format!(
        "{}#{}#{}#{}",
        resources.coin, resources.power, resources.power_limit, suffix
    ));
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Terms {
    pub power_cost: i32,
    pub coin_cost: i32,
    pub round_limit: i32,
    pub all_limit: i32,
}

pub(crate) fn terms(managers: &BattleManagers, skill_id: i32) -> Option<Terms> {
    let snapshot = managers.rouge.context()?.catalyst.as_ref()?;
    let tables = managers.catalog().game_data();
    let style = tables
        .rouge_style
        .iter()
        .find(|row| row.id == snapshot.style_id && row.version.is_empty())?;
    if !style
        .active_skills
        .split('#')
        .any(|id| id.parse::<i32>().ok() == Some(skill_id))
    {
        return None;
    }
    let skill = tables
        .rouge_active_skill
        .iter()
        .find(|row| row.id == skill_id && row.version.is_empty())?;
    (skill.power_cost >= 0 && skill.coin_cost >= 0).then_some(Terms {
        power_cost: skill.power_cost,
        coin_cost: skill.coin_cost,
        round_limit: skill.round_limit,
        all_limit: skill.all_limit,
    })
}

pub(crate) fn cooldown(managers: &BattleManagers, skill_id: i32, round: i32) -> i32 {
    let Some(terms) = terms(managers, skill_id) else {
        return 1;
    };
    let Some(state) = managers
        .rouge
        .active
        .as_ref()
        .and_then(|active| active.catalyst.as_ref())
    else {
        return 1;
    };
    i32::from(
        (terms.round_limit > 0
            && state
                .round_uses
                .get(&(skill_id, round))
                .copied()
                .unwrap_or_default()
                >= terms.round_limit)
            || (terms.all_limit > 0
                && state.total_uses.get(&skill_id).copied().unwrap_or_default() >= terms.all_limit),
    )
}

pub(crate) fn charge(
    managers: &mut BattleManagers,
    skill_id: i32,
    round: i32,
) -> Option<Vec<RuleOp>> {
    let terms = terms(managers, skill_id)?;
    if cooldown(managers, skill_id, round) != 0 {
        return None;
    }
    let ops = try_change_resources(managers, -terms.coin_cost, -terms.power_cost)?;
    let state = managers.rouge.active.as_mut()?.catalyst.as_mut()?;
    let count = state.round_uses.entry((skill_id, round)).or_default();
    *count = count.saturating_add(1);
    let count = state.total_uses.entry(skill_id).or_default();
    *count = count.saturating_add(1);
    Some(ops)
}

fn ally(managers: &BattleManagers, pool: &TargetPool, uid: i64) -> Option<i64> {
    pool.attacker_main
        .iter()
        .any(|entity| entity.uid == uid && managers.hp.current(uid) > 0)
        .then_some(uid)
}

pub(crate) fn support_card(managers: &BattleManagers, owner: i64, rank: i32) -> Option<RuleOp> {
    let mapping = managers
        .rouge
        .context()?
        .support_skills
        .iter()
        .find(|mapping| mapping.owner_uid == owner && mapping.hero_id > 0)?;
    let skill = mapping.skill_ids.iter().copied().find(|skill| {
        managers.catalog().skill_rank(*skill) == rank && !managers.catalog().skill_is_big(*skill)
    })?;
    Some(RuleOp::Command(BattleCommand::Card(
        CardCommand::AddTemporary(CardAddTemporary {
            origin: origin(60050, "DealSupportCard"),
            target_uid: owner,
            skill_id: skill,
            hero_id: Some(mapping.hero_id),
            reserve_id: i64::from(mapping.hero_id),
            team_type: 1,
            kind: TemporaryCardKind::SupportSkill,
        }),
    )))
}

fn own_cards(
    managers: &BattleManagers,
    pool: &TargetPool,
    owner: i64,
    rank: i32,
    kind: TemporaryCardKind,
) -> Option<Vec<RuleOp>> {
    let entity = pool.entity(owner)?;
    [&entity.skill_group1, &entity.skill_group2]
        .into_iter()
        .map(|group| {
            let skill = group.iter().copied().find(|id| {
                managers.catalog().skill_rank(*id) == rank && !managers.catalog().skill_is_big(*id)
            })?;
            Some(RuleOp::Command(BattleCommand::Card(
                CardCommand::AddTemporary(CardAddTemporary {
                    origin: origin(60032, "AddSkillCard"),
                    target_uid: owner,
                    skill_id: skill,
                    hero_id: matches!(kind, TemporaryCardKind::CatalystMasterSkill | TemporaryCardKind::CatalystSyntonySkill).then_some(entity.model_id),
                    reserve_id: i64::from(entity.model_id),
                    team_type: 1,
                    kind,
                }),
            )))
        })
        .collect()
}

fn fill_moxie(managers: &BattleManagers, uid: i64) -> Option<RuleOp> {
    let max = managers.ex_point.maximum(uid)?;
    Some(RuleOp::Command(BattleCommand::ExPoint(
        ExPointCommand::Set(ExPointSet {
            origin: origin(60051, "AddExPointAndDealExCard"),
            source_uid: uid,
            target_uid: uid,
            value: max,
            config_effect: 60051,
            effect_type: sonettobuf::effect_type_enum::EffectType::Expointchange as i32,
        }),
    )))
}

/// The six current permanent effects; prefer one not already held until all
/// six are present. Track grants planned by this operation before executing
/// them so multiple disks cannot all choose the same still-missing state.
fn permanent_buffs(
    managers: &BattleManagers,
    pool: &TargetPool,
    count: i32,
    prior: &[RuleOp],
    mut random_index: impl FnMut(usize) -> Option<usize>,
) -> Vec<RuleOp> {
    const POOL: [i32; 6] = [90001, 90011, 90041, 90021, 90031, 90281];
    let mut ops = Vec::new();
    for entity in pool
        .attacker_main
        .iter()
        .filter(|entity| managers.hp.current(entity.uid) > 0)
    {
        let mut owned: BTreeSet<i32> = POOL
            .into_iter()
            .filter(|id| managers.buff.has_active_buff_id(entity.uid, *id))
            .collect();
        // Opening artefact effects are queued but not yet committed. Include
        // those grants in the same missing-state selection.
        for op in prior {
            if let RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(grant))) = op
                && grant.target_uid == entity.uid
                && POOL.contains(&grant.buff_id)
                && grant.amount != Some(0)
            {
                owned.insert(grant.buff_id);
            }
        }
        for _ in 0..count.clamp(0, 1024) {
            let missing = POOL
                .into_iter()
                .filter(|id| !owned.contains(id))
                .collect::<Vec<_>>();
            let candidates: &[i32] = if missing.is_empty() { &POOL } else { &missing };
            let Some(index) = random_index(candidates.len()) else {
                break;
            };
            let buff = candidates[index];
            owned.insert(buff);
            ops.push(super::grant(entity.uid, entity.uid, buff));
        }
    }
    ops
}

/// Validates the complete effect before charging. Only current base styles
/// have these exact verified grammars; unknown/DLC abilities fail closed.
pub(crate) fn plan(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    skill_id: i32,
    target: i64,
    determinism: &mut RoundDeterminism,
) -> Option<Vec<RuleOp>> {
    terms(managers, skill_id)?;
    let snapshot = managers.rouge.context()?.catalyst.as_ref()?.clone();
    match skill_id {
        800000001 => Some(vec![RuleOp::Command(BattleCommand::Card(
            CardCommand::RedealKeepRanks {
                origin: origin(60012, "RedealCardKeepStar2"),
            },
        ))]),
        800000002 | 800030001 => Some(vec![RuleOp::Command(BattleCommand::Card(
            CardCommand::AddUniversal(CardAddUniversal {
                origin: origin(60002, "AddUniversalCard"),
                count: 1,
                rank: 1,
            }),
        ))]),
        800010001 => {
            ally(managers, pool, target)?;
            Some(vec![support_card(managers, target, 2)?])
        }
        800010002 => {
            ally(managers, pool, target)?;
            if managers.ex_point.is_full(target) {
                return None;
            }
            Some(vec![fill_moxie(managers, target)?])
        }
        800020001 => {
            ally(managers, pool, target)?;
            if managers.ex_point.is_full(target) {
                return None;
            }
            Some(vec![RuleOp::Command(BattleCommand::ExPoint(
                ExPointCommand::Change(ExPointChange {
                    origin: origin(20002, "ExPointChange"),
                    source_uid: target,
                    target_uid: target,
                    delta: 1,
                    config_effect: 20002,
                    effect_type: sonettobuf::effect_type_enum::EffectType::Expointchange as i32,
                }),
            ))])
        }
        800020002 => {
            let targets = pool
                .attacker_main
                .iter()
                .filter(|entity| managers.hp.current(entity.uid) > 0)
                .map(|entity| entity.uid)
                .collect::<Vec<_>>();
            if targets.is_empty() {
                return None;
            }
            targets
                .into_iter()
                .map(|uid| fill_moxie(managers, uid))
                .collect()
        }
        800030002 | 800040001 | 800050001 => {
            ally(managers, pool, target)?;
            own_cards(
                managers,
                pool,
                target,
                if skill_id == 800030002 { 3 } else { 1 },
                // The syntony catalyst's two basics are likewise captured as
                // MASTERADDHANDCARD (197), ordinary type 0 temporary cards.
                match skill_id {
                    800030002 => TemporaryCardKind::CatalystMasterSkill,
                    800040001 => TemporaryCardKind::CatalystSyntonySkill,
                    _ => TemporaryCardKind::HeroSkill,
                },
            )
        }
        800040002 | 800050002 => {
            // The client allows catalyst use before the action queue. Keep
            // its exact one-round state and apply the boost when actual casts
            // subsequently trigger the matching base mechanic.
            let state = managers.rouge.active.as_mut()?.catalyst.as_mut()?;
            let (boost, buff) = if skill_id == 800040002 {
                (&mut state.syntony_boost, 90341)
            } else {
                (&mut state.resonance_boost, 90351)
            };
            *boost = boost.saturating_add(1);
            // 800040002 is captured with both the carrier and its level UI
            // update. The analogous resonance marker is a local inference.
            let effect_type = if skill_id == 800040002 { 177 } else { 181 };
            Some(vec![super::grant(0, 0, buff), marker(effect_type, 1)])
        }
        800060001 => Some(
            pool.defender_all
                .iter()
                .filter(|entity| managers.hp.current(entity.uid) > 0)
                .map(|entity| super::grant(0, entity.uid, 4011))
                .collect(),
        ),
        800060002 | 800070002 => Some(vec![RuleOp::Command(BattleCommand::Card(
            CardCommand::AddRougeSpecial(crate::engine::manager::card::CardAddPrecast {
                origin: origin(50031, "AddSpTempCard"),
                card: sonettobuf::CardInfo {
                    uid: Some(0),
                    hero_id: Some(0),
                    skill_id: Some(if skill_id == 800060002 {
                        800060003
                    } else {
                        800070003
                    }),
                    card_type: Some(1),
                    temp_card: Some(true),
                    ..Default::default()
                },
            }),
        ))]),
        800070001 | 800080001 => {
            ally(managers, pool, target)?;
            let count = if skill_id == 800070001 {
                1 + snapshot.epic_count.max(0) / 3
            } else {
                snapshot.disk_count.max(0) / 5
            };
            if count == 0 {
                return None;
            }
            let op = support_card(managers, target, 1)?;
            Some(std::iter::repeat_n(op, count as usize).collect())
        }
        800080002 => {
            if snapshot.disk_count <= 0 {
                return None;
            }
            Some(permanent_buffs(
                managers,
                pool,
                snapshot.disk_count,
                &[],
                |len| determinism.lua_random_index(len),
            ))
        }
        _ => None,
    }
}

pub(crate) fn extra_skill_roots(snapshot: Option<&Catalyst>) -> Vec<i32> {
    match snapshot.map(|snapshot| snapshot.style_id) {
        Some(7) => vec![800060003],
        Some(8) => vec![800070003],
        _ => vec![],
    }
}

pub(crate) fn configure_catalog(
    catalog: &mut crate::engine::skill::effect::SkillEffectCatalog,
    snapshot: Option<&Catalyst>,
) {
    let Some(snapshot) = snapshot.filter(|snapshot| snapshot.style_id == 8) else {
        return;
    };
    use crate::engine::skill::{
        behavior::classify::BehaviorSpec,
        effect::{ParsedBehavior, ParsedSkillEffect, SkillEffectSlot},
        target::TargetRequest,
    };
    // Verified current 30005#1#101#150, repeated once per three frozen Epic
    // artefacts. This replaces only its otherwise-unmapped layout condition,
    // not unrelated skills or statuses from the older trigger tables.
    catalog.insert(ParsedSkillEffect {
        skill_id: 800070003,
        slots: (0..(1 + snapshot.epic_count.max(0) / 3))
            .map(|_| {
                SkillEffectSlot::new(
                    ParsedBehavior::from_spec(
                        BehaviorSpec::new(30005, "LostLife"),
                        vec![1, 101, 150],
                        Vec::new(),
                    ),
                    TargetRequest {
                        code: 202,
                        raw: vec![],
                    },
                )
            })
            .collect(),
    });
}

pub(crate) fn level_bonus(managers: &BattleManagers) -> (i32, i32) {
    let Some(context) = managers.rouge.context() else {
        return (0, 0);
    };
    let style = context
        .catalyst
        .as_ref()
        .map_or(0, |snapshot| snapshot.style_id);
    let state = managers
        .rouge
        .active
        .as_ref()
        .and_then(|active| active.catalyst.as_ref());
    (
        i32::from(style == 5) + state.map_or(0, |state| state.syntony_boost),
        i32::from(style == 6) + state.map_or(0, |state| state.resonance_boost),
    )
}

pub(crate) fn clear_round_boosts(managers: &mut BattleManagers) {
    if let Some(state) = managers
        .rouge
        .active
        .as_mut()
        .and_then(|active| active.catalyst.as_mut())
    {
        state.syntony_boost = 0;
        state.resonance_boost = 0;
    }
}

impl State {
    pub(super) fn new(snapshot: &Catalyst) -> Self {
        Self {
            resources: Resources {
                coin: snapshot.coin.max(0),
                power: snapshot.power.clamp(0, snapshot.power_limit.max(0)),
                power_limit: snapshot.power_limit.max(0),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub(crate) fn ultimate_cost_offset(managers: &BattleManagers, uid: i64) -> i32 {
    -i32::from(
        managers
            .rouge
            .context()
            .and_then(|context| context.catalyst.as_ref())
            .is_some_and(|snapshot| {
                snapshot.style_id == 3 && snapshot.slots.iter().any(|(_, actor)| *actor == uid)
            }),
    )
}

pub(crate) fn opening(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    prior: &[RuleOp],
) -> Vec<RuleOp> {
    let Some(snapshot) = managers
        .rouge
        .context()
        .and_then(|context| context.catalyst.clone())
    else {
        return vec![];
    };
    let mut ops = Vec::new();
    if snapshot.style_id == 7 {
        let power = managers.rouge.resources().map_or(0, |state| state.power);
        ops.extend(
            try_change_resources(managers, power.saturating_mul(2), -power).unwrap_or_default(),
        );
    }
    if snapshot.style_id == 3 {
        for entity in &pool.attacker_main {
            if managers.ex_point.kind(entity.uid) == 0 {
                ops.push(RuleOp::Command(BattleCommand::ExPoint(
                    ExPointCommand::ChangeMax(crate::engine::manager::ex_point::ExPointMaxChange {
                        origin: ORIGIN,
                        target_uid: entity.uid,
                        delta: -1,
                        wire: crate::engine::manager::ex_point::ExPointMaxWire::Delta,
                    }),
                )));
            }
        }
    }
    if snapshot.style_id == 9 && snapshot.disk_count > 0 {
        // Use a separate stable seed for opening, avoiding interference with
        // card draws and retaining exactly the same choices on checkpoint replay.
        let seed = managers
            .rouge
            .context()
            .and_then(|context| context.entry.as_ref())
            .map_or(0, |entry| entry.seed)
            ^ 0x726f_7567_655f_3039;
        let mut random = RoundDeterminism::with_seed(seed);
        ops.extend(permanent_buffs(
            managers,
            pool,
            snapshot.disk_count,
            prior,
            |len| random.card_random_index(len),
        ));
    }
    ops
}

pub(crate) fn ultimate_availability(
    managers: &BattleManagers,
    pool: &TargetPool,
    uid: i64,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let mapping = context
        .support_ultimates
        .iter()
        .find(|mapping| mapping.owner_uid == uid && mapping.hero_id > 0 && mapping.skill_id > 0);
    if context.catalyst.is_none() && mapping.is_none() {
        return vec![];
    }
    let Some(entity) = pool.attacker_main.iter().find(|entity| entity.uid == uid) else {
        return vec![];
    };
    let ready = managers.hp.current(uid) > 0
        && crate::engine::mechanic::card::CardMechanic.ultimate_ready(managers, entity);
    let mut ops = Vec::new();
    if ready {
        if !managers
            .card
            .hand()
            .iter()
            .any(|card| card.uid == Some(uid) && card.skill_id == Some(entity.ex_skill))
        {
            if let Some(card) =
                crate::engine::manager::card::pool::card_for_target(entity, entity.ex_skill)
            {
                ops.push(RuleOp::Command(BattleCommand::Card(
                    CardCommand::SetUltimateAvailability(
                        crate::engine::manager::card::CardSetUltimateAvailability {
                            origin: ORIGIN,
                            card,
                            available: true,
                        },
                    ),
                )));
            }
        }
        if let Some(mapping) =
            mapping.filter(|mapping| managers.catalog().skill_is_big(mapping.skill_id))
        {
            if !managers.card.hand().iter().any(|card| {
                card.uid == Some(uid)
                    && card.skill_id == Some(mapping.skill_id)
                    && card.card_type == Some(3)
            }) {
                ops.push(RuleOp::Command(BattleCommand::Card(
                    CardCommand::AddSupportUltimate(crate::engine::manager::card::CardAddPrecast {
                        origin: ORIGIN,
                        card: sonettobuf::CardInfo {
                            uid: Some(uid),
                            skill_id: Some(mapping.skill_id),
                            hero_id: Some(mapping.hero_id),
                            card_type: Some(3),
                            temp_card: Some(false),
                            ..Default::default()
                        },
                    }),
                )));
            }
        }
    } else {
        let indices = managers
            .card
            .hand()
            .iter()
            .enumerate()
            .filter_map(|(index, card)| {
                (card.uid == Some(uid)
                    && card.card_type != Some(6)
                    && (card.skill_id == Some(entity.ex_skill)
                        || mapping.is_some_and(|mapping| card.skill_id == Some(mapping.skill_id))))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        for index in indices.into_iter().rev() {
            ops.push(RuleOp::Command(BattleCommand::Card(
                CardCommand::Dissolve {
                    origin: ORIGIN,
                    card_index: index,
                },
            )));
        }
    }
    ops
}

pub(crate) fn action(
    managers: &BattleManagers,
    action: &crate::engine::skill::action::ActionEvent,
    aid: bool,
) -> Vec<RuleOp> {
    let Some(snapshot) = managers
        .rouge
        .context()
        .and_then(|context| context.catalyst.as_ref())
    else {
        return vec![];
    };
    if managers.hp.current(action.source_uid) <= 0 {
        return vec![];
    }
    let mut ops = Vec::new();
    // "Aid incantation" excludes Aid ultimates, which spend their owner's
    // Moxie normally and must not refund one point through this passive.
    if snapshot.style_id == 2 && aid && !managers.catalog().skill_is_big(action.skill_id) {
        ops.push(RuleOp::Command(BattleCommand::ExPoint(
            ExPointCommand::Change(ExPointChange {
                origin: ORIGIN,
                source_uid: action.source_uid,
                target_uid: action.source_uid,
                delta: 1,
                config_effect: 0,
                effect_type: sonettobuf::effect_type_enum::EffectType::Expointchange as i32,
            }),
        )));
    }
    if snapshot.style_id == 3 && managers.catalog().skill_is_big(action.skill_id) {
        ops.push(super::grant(action.source_uid, action.source_uid, 90331));
    }
    if snapshot.style_id == 4 && !managers.catalog().skill_is_big(action.skill_id) {
        match action.rank {
            2 => ops.push(super::grant(action.source_uid, action.source_uid, 90331)),
            3 => ops.push(super::grant(action.source_uid, action.source_uid, 90332)),
            _ => {}
        }
    }
    ops
}

pub(crate) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    determinism: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(snapshot) = managers
        .rouge
        .context()
        .and_then(|context| context.catalyst.clone())
    else {
        return vec![];
    };
    let Some(state) = managers
        .rouge
        .active
        .as_mut()
        .and_then(|active| active.catalyst.as_mut())
    else {
        return vec![];
    };
    if round <= 0 || state.passive_round.is_some_and(|seen| seen >= round) {
        return vec![];
    }
    state.passive_round = Some(round);
    let eligible = pool
        .attacker_main
        .iter()
        .filter(|entity| {
            managers.hp.current(entity.uid) > 0 && support_card(managers, entity.uid, 1).is_some()
        })
        .map(|entity| entity.uid)
        .collect::<Vec<_>>();
    let mut ops = Vec::new();
    if snapshot.style_id == 1 {
        for (slot, uid) in &snapshot.slots {
            if ((round % 2 == 1 && matches!(*slot, 1 | 3))
                || (round % 2 == 0 && matches!(*slot, 2 | 4)))
                && eligible.contains(uid)
            {
                if let Some(card) = support_card(managers, *uid, 1) {
                    ops.push(card);
                }
            }
        }
    } else if matches!(snapshot.style_id, 2 | 7) && !eligible.is_empty() {
        let count = if snapshot.style_id == 2 {
            1
        } else {
            (managers.rouge.resources().map_or(0, |state| state.coin) / 100).clamp(0, 5)
        };
        for _ in 0..count {
            if let Some(index) = determinism.lua_random_index(eligible.len()) {
                if let Some(card) = support_card(managers, eligible[index], 1) {
                    ops.push(card);
                }
            }
        }
    }
    for entity in &pool.attacker_main {
        ops.extend(ultimate_availability(managers, pool, entity.uid));
    }
    ops
}
