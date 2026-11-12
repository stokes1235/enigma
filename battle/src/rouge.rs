//! Optional base-Rouge battle context and deterministic event state.
//!
//! The server persists the initial context with the battle seed and replays
//! committed operations on reconnect. Never rebuild it from a changed bag.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::{
    entity::attr::AttrId,
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffGrant},
        shield::{ShieldCarrierUid, ShieldCommand, ShieldScope},
    },
    skill::{
        rule::{
            CommandOrigin, DefinitionKey, RuleDomain,
            output::{BattleCommand, RuleOp},
        },
        target::TargetPool,
    },
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RougeContext {
    /// Server-only per-mille attributes which have no HeroSpAttribute field.
    pub extra_attributes: Vec<(i64, i32, i32)>,
    /// Active, placed base artefacts, captured before entering this fight.
    pub artefacts: Vec<Artefact>,
    /// Exact deployed slot one, if the server has that mapping. Otherwise the
    /// first main ally is used for compatibility with older saved contexts.
    pub first_slot_uid: Option<i64>,
    pub conditional_modifiers: Vec<ConditionalModifier>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackPredicate {
    RankAtLeast(i32),
    Support,
    ExtraAction,
    HasBullet,
    NonUltimate,
    Ultimate,
    All(Vec<AttackPredicate>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionalModifier {
    /// Artefact instance UID, not the attacking hero UID.
    pub source_uid: i64,
    pub item_id: i32,
    pub predicate: AttackPredicate,
    pub attr_id: i32,
    pub value: i32,
}

pub(crate) struct AttackContext {
    pub rank: i32,
    pub ultimate: bool,
    pub support: bool,
    pub extra_action: bool,
}

impl AttackPredicate {
    fn matches(&self, context: &AttackContext, depth: usize) -> bool {
        if depth >= 16 {
            return false;
        }
        match self {
            Self::RankAtLeast(rank) => context.rank >= *rank,
            Self::Support => context.support,
            Self::ExtraAction => context.extra_action,
            Self::NonUltimate => !context.ultimate,
            Self::Ultimate => context.ultimate,
            // The first batch does not infer ammunition from an unrelated buff.
            Self::HasBullet => false,
            Self::All(predicates) => predicates.iter().all(|p| p.matches(context, depth + 1)),
        }
    }
}

pub(crate) fn attack_attributes(
    managers: &BattleManagers,
    context: &AttackContext,
) -> Vec<(AttrId, i32)> {
    managers
        .rouge
        .context()
        .into_iter()
        .flat_map(|context| &context.conditional_modifiers)
        .filter(|modifier| modifier.predicate.matches(context, 0))
        .filter_map(|modifier| Some((AttrId::from_raw(modifier.attr_id)?, modifier.value)))
        .collect()
}

pub(crate) fn is_support_card(
    managers: &BattleManagers,
    source_uid: i64,
    skill_id: i32,
    card_index: i32,
) -> bool {
    card_index > 0
        && managers.card.played().iter().any(|played| {
            played.card_index == card_index
                && played.skill_id == skill_id
                && played.caster_uid == source_uid
                && matches!(played.card.card_type, Some(2 | 3))
        })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Artefact {
    pub uid: i64,
    pub item_id: i32,
    /// Static enhancement units from the matching inserted Disks.
    pub count2: i32,
    pub only_support_below: bool,
}

/// Per-item growth earned during this battle, relative to the initial context.
/// The first supported triggers do not create permanent growth, so this is
/// empty until a corresponding verified growth route is implemented.
pub type Counters = BTreeMap<i64, BTreeMap<String, i32>>;

#[derive(Debug, Clone, Default)]
pub(crate) struct RougeRuntime {
    // Ordinary battles keep one empty pointer rather than inline Rouge state.
    // Deep reaction chains also clone BattleManagers while planning commands.
    active: Option<Box<ActiveRouge>>,
}

#[derive(Debug, Clone, Default)]
struct ActiveRouge {
    context: RougeContext,
    entered: bool,
    aid_round: Option<i32>,
    shield_round: Option<i32>,
    counters: Counters,
}

impl RougeRuntime {
    pub(crate) fn context(&self) -> Option<&RougeContext> {
        self.active.as_ref().map(|active| &active.context)
    }
    pub(crate) fn counters(&self) -> Counters {
        self.active
            .as_ref()
            .map(|active| active.counters.clone())
            .unwrap_or_default()
    }
}

pub(crate) fn configure(managers: &mut BattleManagers, context: RougeContext) {
    // This input is intentionally restricted to the missing Genesis field;
    // existing extended/special fields retain their normal protocol path.
    let mut additions = BTreeMap::<i64, i32>::new();
    for (uid, raw_attr, value) in &context.extra_attributes {
        if *raw_attr == AttrId::GenesisDmgBonus.id() {
            let sum = additions.entry(*uid).or_default();
            *sum = sum.saturating_add(*value);
        }
    }
    for (uid, value) in additions {
        managers.attribute.add(uid, AttrId::GenesisDmgBonus, value);
    }
    managers.rouge = RougeRuntime {
        active: Some(Box::new(ActiveRouge {
            context,
            ..Default::default()
        })),
    };
}

const ORIGIN: CommandOrigin = CommandOrigin {
    domain: RuleDomain::Lifecycle,
    key: DefinitionKey::new(0, "BaseRougeArtefact"),
};

fn grant(source_uid: i64, target_uid: i64, buff_id: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid,
        target_uid,
        buff_id,
        amount: None,
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn first_ally(managers: &BattleManagers, pool: &TargetPool) -> Option<i64> {
    let uid = managers
        .rouge
        .context()?
        .first_slot_uid
        .or_else(|| pool.attacker_main.first().map(|entity| entity.uid))?;
    (pool.source_is_attacker(uid) && managers.hp.current(uid) > 0).then_some(uid)
}

pub(crate) fn opening(managers: &mut BattleManagers, pool: &TargetPool) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_ref() else {
        return vec![];
    };
    if state.entered {
        return vec![];
    }
    let bottle = state
        .context
        .artefacts
        .iter()
        .any(|item| item.item_id == 8130315);
    managers.rouge.active.as_mut().unwrap().entered = true;
    // Shipped Saliva Bottle skill 91220070 uses 5042 at entry and 5041 on
    // casts. Entry's two ticks include the opening duration settlement.
    if bottle && let Some(uid) = first_ally(managers, pool) {
        vec![grant(uid, uid, 5042)]
    } else {
        vec![]
    }
}

/// Observe events once at the manager commit boundary, not once per reaction
/// lane. The resulting ordinary commands keep packet projection and secondary
/// reactions on the same path as every other battle effect.
pub(crate) fn observe_events(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    events: &[BattleEvent],
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let bottle = context.artefacts.iter().any(|item| item.item_id == 8130315);
    let mut result = Vec::new();
    for event in events {
        match event {
            BattleEvent::AllyAction(action) if pool.source_is_attacker(action.source_uid) => {
                // FightEnum.CardType SUPPORT_NORMAL=2 / SUPPORT_EX=3. The
                // action's extra_skill_kind describes a different concept.
                let aid = is_support_card(
                    managers,
                    action.source_uid,
                    action.skill_id,
                    action.card_index,
                );
                if aid {
                    managers.rouge.active.as_mut().unwrap().aid_round = Some(round);
                }
                if bottle
                    && matches!(action.rank, 2 | 3)
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && let Some(uid) = first_ally(managers, pool)
                {
                    result.push(grant(uid, uid, 5041));
                }
            }
            // Off-brand Serum's kill buffs are identified (90331/90082),
            // but 783 AddSupportCard and 739's random-group route are not
            // implemented. Do not grant a state that stalls the next round.
            _ => {}
        }
    }
    result
}

pub(crate) fn round_end(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_ref() else {
        return vec![];
    };
    if state.aid_round != Some(round) || state.shield_round == Some(round) {
        return vec![];
    }
    let rate = state
        .context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8320109)
        .map(|item| {
            150_i32
                .saturating_add(item.count2.saturating_mul(10))
                .saturating_add(if item.only_support_below { 100 } else { 0 })
                .max(0)
        })
        .fold(0_i32, i32::saturating_add);
    managers.rouge.active.as_mut().unwrap().shield_round = Some(round);
    let mut result = Vec::new();
    if rate > 0 {
        for entity in pool
            .attacker_main
            .iter()
            .filter(|entity| managers.hp.current(entity.uid) > 0)
        {
            let hp = i64::from(managers.hp.get(entity.uid).max.max(1));
            // Preserve an existing shield and allow multiple marionettes to
            // add together; a smaller new shield must never reduce old HP.
            let old_rate = (i64::from(managers.hp.shield(entity.uid)) * 1000 + hp - 1) / hp;
            let max_rate = (old_rate + i64::from(rate)).min(i64::from(i32::MAX)) as i32;
            // Verified neutral Shield carrier: duringTime=1, typeId=5001,
            // features=501#0#101#150 and no extra status/attribute features.
            // The original artefact carrier ID is unavailable. Reusing this
            // plain shield is an explicit local wire policy; amount/lifetime
            // follow the artefact, not the character that also uses the ID.
            result.push(RuleOp::Command(BattleCommand::Shield(ShieldCommand {
                origin: ORIGIN,
                source_uid: entity.uid,
                target_uid: entity.uid,
                buff_id: 30740121,
                amount_attr: AttrId::Hp,
                amount_rate: rate,
                multiplier_bonus: None,
                max_attr: AttrId::Hp,
                max_rate,
                scope: ShieldScope::Entity,
                carrier_uid: ShieldCarrierUid::Definition,
            })));
        }
    }
    result
}

#[cfg(test)]
mod tests;
