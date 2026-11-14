//! Optional base-Rouge battle context and deterministic event state.
//!
//! The server persists the initial context with the battle seed and replays
//! committed operations on reconnect. Never rebuild it from a changed bag.
use std::collections::{BTreeMap, BTreeSet};

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

pub mod catalyst;
mod growth;
mod statistics;
pub use catalyst::{Catalyst, Resources, SupportUltimate};
pub mod artefact_entry;
pub use artefact_entry::EntryContext;
mod artefact_attack_proc;
pub(crate) use artefact_attack_proc::fowling_piece_bonus;
mod artefact_balance;
mod artefact_bullet;
mod artefact_coin_growth;
mod artefact_dolls;
mod artefact_electric;
mod artefact_ending;
mod artefact_events;
mod artefact_opening_hand;
mod artefact_ritual;
mod artefact_storm;
pub(crate) use artefact_storm::prepare as prepare_storm_contracts;
mod artefact_turns;
pub mod difficulty;
pub use difficulty::Difficulty;
pub(crate) use growth::end_round as end_incantation_round;
pub(crate) use growth::settle_queue as settle_incantation_queue;
pub(crate) use statistics::record_hp;

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
    /// Frozen map classification; false for ordinary battles and old saves.
    pub danger_or_crisis: bool,
    /// Frozen map type for effects applying only to Present Danger.
    pub present_danger: bool,
    /// Whether the entered fight belongs to the final-battle map.
    pub final_battle: bool,
    pub conditional_modifiers: Vec<ConditionalModifier>,
    pub support_skills: Vec<SupportSkill>,
    /// Actual deployed mains and selected supports, frozen for ending badges.
    pub party_hero_ids: Vec<i32>,
    /// Previously earned growth. The frozen Fight already includes these
    /// attributes; this is a baseline for persistence, never applied twice.
    pub permanent_growth: Counters,
    pub growth_sources: Vec<GrowthSource>,
    /// None preserves old contexts and ordinary battles without this unlock.
    pub incantation_rules: Option<IncantationRules>,
    pub catalyst: Option<Catalyst>,
    pub support_ultimates: Vec<SupportUltimate>,
    pub entry: Option<EntryContext>,
    pub difficulty: Option<Difficulty>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrowthSource {
    pub uid: i64,
    pub item_id: i32,
    pub syntony_artefact_count: i32,
    pub rare_artefact_count: i32,
    /// Deployed actor UIDs; support incantations retain their main's UID.
    pub actor_uids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IncantationRules {
    pub syntony_level_limit: i32,
    pub resonance_level_limit: i32,
}

impl Default for IncantationRules {
    fn default() -> Self {
        Self {
            syntony_level_limit: 5,
            resonance_level_limit: 5,
        }
    }
}

/// Committed events only. Damage includes absorbed shields and clipped HP
/// loss, never overkill. An action maximum is the completed action's damage;
/// it is separate from the maximum single-target HP command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BattleSummary {
    pub damage_dealt: i64,
    pub damage_taken: i64,
    pub healing_done: i64,
    pub max_single_damage: i64,
    pub max_action_damage: i64,
    pub enemy_kills: i32,
    pub ally_deaths: i32,
    pub ally_revives: i32,
    pub casts: i32,
    pub rank1_casts: i32,
    pub rank2_casts: i32,
    pub rank3_casts: i32,
    pub ultimate_casts: i32,
    pub support_casts: i32,
    pub extra_action_casts: i32,
    pub syntony_max_level: i32,
    pub resonance_max_level: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportSkill {
    /// Deployed actor whose attributes and life govern the support cast.
    pub owner_uid: i64,
    /// Support character shown by CardInfo and the skill's animation.
    pub hero_id: i32,
    /// Frozen selected basic skill group, containing its rank-one/two/three IDs.
    /// This batch does not turn a selected ultimate into a basic Aid card.
    pub skill_ids: Vec<i32>,
}

pub(crate) fn hand_limit_bonus(managers: &BattleManagers) -> i32 {
    let Some(context) = managers.rouge.context() else {
        return 0;
    };
    let style = match context.catalyst.as_ref().map(|catalyst| catalyst.style_id) {
        Some(4) => 2_i32,
        Some(5 | 6) => 3,
        _ => 0,
    };
    context
        .artefacts
        .iter()
        .map(|item| match item.item_id {
            8230305 => 1,
            8330306 => 2,
            _ => 0,
        })
        .fold(style, i32::saturating_add)
}

/// Old Bag's configured 4/3/2-main thresholds give +3/+2/+1. One
/// remaining actor gives no additional slots; stored artefacts are excluded.
pub(crate) fn old_bag_hand_limit_bonus(managers: &BattleManagers, pool: &TargetPool) -> i32 {
    let Some(context) = managers.rouge.context() else {
        return 0;
    };
    let active = pool
        .attacker_main
        .iter()
        .filter(|actor| managers.hp.current(actor.uid) > 0)
        .count();
    let per_item = active.saturating_sub(1).min(3) as i32;
    context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8230039)
        .fold(0_i32, |sum, _| sum.saturating_add(per_item))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackPredicate {
    RankAtLeast(i32),
    Support,
    ExtraAction,
    HasBullet,
    NonExtraAction,
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
    pub has_bullet: bool,
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
            Self::HasBullet => context.has_bullet,
            Self::NonExtraAction => !context.extra_action,
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

/// Target-specific artefact modifiers must be evaluated for each hit, not
/// attached to the whole action: a multi-target spell can hit different states.
pub(crate) fn target_attack_attributes(
    managers: &BattleManagers,
    source_uid: i64,
    target_uid: i64,
    is_ultimate: bool,
) -> Vec<(AttrId, i32)> {
    use crate::engine::manager::buff::BuffStatus;
    use crate::engine::skill::buff_act::registry::BuffActKind as Kind;
    let Some(context) = managers.rouge.context() else {
        return Vec::new();
    };
    let has = |kind| managers.buff.has_buff_act_kind(target_uid, kind);
    let controlled = [
        Kind::Dizzy,
        Kind::Petrified,
        Kind::Sleep,
        Kind::Frozen,
        Kind::Disarm,
        Kind::Forbid,
        Kind::Seal,
    ]
    .into_iter()
    .any(has);
    let mut result = Vec::new();
    for item in &context.artefacts {
        match item.item_id {
            8220033 => {
                // Each component is a per-mille attack rate for this hit;
                // the source condition is limited to ordinary incantations.
                if !is_ultimate
                    && !managers
                        .buff
                        .buffs_with_status(source_uid, BuffStatus::Channel)
                        .is_empty()
                {
                    result.push((AttrId::Attack, 100));
                }
                if !managers
                    .buff
                    .buffs_with_status(target_uid, BuffStatus::Channel)
                    .is_empty()
                {
                    result.push((AttrId::Attack, 150));
                }
            }
            // The sleeping target is also controlled; both bonuses apply.
            8210031 => {
                if has(Kind::Sleep) {
                    result.push((AttrId::CriticalRate, 1000));
                }
                if controlled {
                    result.push((AttrId::DmgBonus, 300));
                }
            }
            8330211 if has(Kind::Burn) => result.push((AttrId::DmgBonus, 500)),
            _ => {}
        }
    }
    result
}

/// Royal Horseshoe affects only allied targets of an enemy Ultimate; the
/// modifier lives on this hit and cannot leak to other targets or battles.
pub(crate) fn target_defense_attributes(
    managers: &BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    target_uid: i64,
    is_ultimate: bool,
) -> Vec<(AttrId, i32)> {
    if !is_ultimate
        || pool.team_type(source_uid) != Some(2)
        || pool.team_type(target_uid) != Some(1)
    {
        return Vec::new();
    }
    managers
        .rouge
        .context()
        .into_iter()
        .flat_map(|context| &context.artefacts)
        .filter(|item| item.item_id == 8120216)
        .map(|_| (AttrId::DmgTakenReduction, 200))
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

/// Keep the actor's numeric attributes but honor the selected support skill's
/// configured Reality/Mental damage tag for this cast only.
pub(crate) fn support_target_pool(
    managers: &BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    skill_id: i32,
    card_index: i32,
) -> Option<Box<TargetPool>> {
    managers.rouge.context()?;
    if !is_support_card(managers, source_uid, skill_id, card_index) {
        return None;
    }
    let damage_type = match managers.catalog().skill_effect_tag(skill_id) {
        1 => crate::engine::skill::target::EntityDamageType::Reality,
        2 => crate::engine::skill::target::EntityDamageType::Mental,
        _ => return None,
    };
    let mut view = Box::new(pool.clone());
    for entity in view.attacker_main.iter_mut().chain(&mut view.attacker_all) {
        if entity.uid == source_uid {
            entity.damage_type = damage_type;
        }
    }
    Some(view)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Artefact {
    pub uid: i64,
    pub item_id: i32,
    /// Static enhancement units from the matching inserted Disks.
    pub count2: i32,
    pub count4: i32,
    /// Number of placed telescopes whose actual range covers this source.
    pub extra_action_chance_doublers: i32,
    pub only_support_below: bool,
    /// Frozen layout condition; stored items and the source itself do not count.
    pub non_resonance_in_range: bool,
    /// A different mechanic from Support intersects this item's range.
    pub non_support_in_range: bool,
    /// Frozen count of medication-tagged artefacts intersecting this range.
    pub medication_in_range: i32,
    /// Whether a physically placed syntony-tagged artefact intersects this range.
    pub syntony_in_range: bool,
    pub syntony_in_range_count: i32,
    pub support_in_range_count: i32,
    /// Placed Support-tagged artefacts touching the source's lower edge.
    pub support_below_count: i32,
    /// Frozen number of physically placed, effectively electric-tagged items.
    pub electric_artefact_count: i32,
    /// Effective resonance-tagged physical items in the frozen layout.
    pub resonance_artefact_count: i32,
    /// Placed Ritual-tagged artefacts intersecting this item's range.
    pub ritual_in_range_count: i32,
    /// Physical artefacts intersecting this source's frozen range.
    pub artefacts_in_range_count: i32,
}

impl Artefact {
    /// Apply explicit telescope coverage to this artefact's own probability.
    /// Multiple telescopes multiply in stable local policy, capped at 100%.
    pub(crate) fn proc_chance(&self, chance: i32) -> i32 {
        let mut value = chance.clamp(0, 1000);
        for _ in 0..self.extra_action_chance_doublers.clamp(0, 10) {
            value = value.saturating_mul(2).min(1000);
        }
        value
    }
}

/// Per-item growth earned during this battle, relative to the initial context.
/// `growth_pct` is percentage points, separate from inserted-Disk count2.
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
    aid_since_settlement: bool,
    next_round_action_points: i32,
    current_syntony_bonus: i32,
    next_syntony_bonus: i32,
    artefact_trigger_counts: BTreeMap<(i64, i32), i32>,
    bullet_observed_amounts: BTreeMap<i64, i32>,
    bullet_buff_amounts: BTreeMap<i64, i32>,
    ritual_buff_amounts: BTreeMap<i64, i32>,
    ritual_loss_progress: BTreeMap<i64, i64>,
    electric_observed_amounts: BTreeMap<(i64, i64), i32>,
    electric_remainders: BTreeMap<(i64, i64, i32), i32>,
    /// Actual post-cap Dynamo gains by round and deployed actor.
    electric_round_gains: BTreeMap<(i32, i64), i32>,
    /// Prevents repeated round-end effects and separates the enemy phase.
    electric_ended_rounds: BTreeSet<i32>,
    coin_growth_remainders: BTreeMap<i64, i32>,
    doll_buff_amounts: BTreeMap<i64, i32>,
    shield_round: Option<i32>,
    counters: Counters,
    growth_triggers: BTreeSet<(i64, i64, i32)>,
    ending_last_round: Option<i32>,
    summary: BattleSummary,
    incantation: IncantationState,
    catalyst: Option<catalyst::State>,
    turn_effects: artefact_turns::TurnLedger,
    opening_cards_done: bool,
    difficulty: difficulty::Runtime,
}

#[derive(Debug, Clone, Default)]
struct IncantationState {
    casts_by_type: BTreeMap<i32, i32>,
    levels: BTreeMap<i32, i32>,
    resonance_level: i32,
    /// A real committed action queue has already settled this round.
    queue_settled: bool,
    /// Exact deltas owned by this round, so permanent growth is not removed.
    attributes: BTreeMap<(i64, i32), i32>,
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
    pub(crate) fn summary(&self) -> Option<BattleSummary> {
        self.active.as_ref().map(|active| active.summary.clone())
    }
    pub(crate) fn resources(&self) -> Option<Resources> {
        self.active
            .as_ref()?
            .catalyst
            .as_ref()
            .map(|state| state.resources.clone())
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
            catalyst: context.catalyst.as_ref().map(catalyst::State::new),
            context,
            ..Default::default()
        })),
    };
    difficulty::configure(managers);
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

/// Team-level AP is independent of any particular character remaining alive.
pub(crate) fn queue_next_round_action_points(managers: &mut BattleManagers, amount: i32) {
    if let Some(active) = managers.rouge.active.as_mut() {
        active.next_round_action_points = active
            .next_round_action_points
            .saturating_add(amount.max(0));
    }
}

pub(crate) fn pending_action_point_bonus(managers: &BattleManagers) -> i32 {
    managers
        .rouge
        .active
        .as_ref()
        .map_or(0, |active| active.next_round_action_points)
}

pub(crate) fn consume_pending_action_points(managers: &mut BattleManagers) {
    if let Some(active) = managers.rouge.active.as_mut() {
        active.next_round_action_points = 0;
    }
}

pub(crate) fn persistent_action_point_bonus(managers: &BattleManagers) -> i32 {
    let Some(context) = managers.rouge.context() else {
        return 0;
    };
    i32::try_from(
        context
            .artefacts
            .iter()
            .filter(|item| {
                (context.danger_or_crisis && item.item_id == 8330417)
                    || (context.final_battle && item.item_id == 8180502)
            })
            .count(),
    )
    .unwrap_or(i32::MAX)
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
    let lantern = state
        .context
        .artefacts
        .iter()
        .any(|item| item.item_id == 8320514);
    let rose_and_slough = state
        .context
        .artefacts
        .iter()
        .any(|item| item.item_id == 8330311);
    managers.rouge.active.as_mut().unwrap().entered = true;
    // Shipped Saliva Bottle skill 91220070 uses 5042 at entry and 5041 on
    // casts. Entry's two ticks include the opening duration settlement.
    let mut ops = artefact_opening_hand::opening(managers, pool);
    ops.extend(artefact_storm::opening_marker(managers));
    if bottle && let Some(uid) = first_ally(managers, pool) {
        ops.push(grant(uid, uid, 5042));
    }
    if lantern {
        // The configured permanent carrier excludes Pollution, Poison,
        // Corrode and Burn by buff type. The battle-start rule targets allied
        // mains; including only living deployed actors follows the existing
        // entry policy and remains an inference for reserve/revival cases.
        for entity in &pool.attacker_main {
            if managers.hp.current(entity.uid) > 0 {
                // In the difficulty-6 capture, this carrier is granted by
                // the field (source UID 0), not by each receiving actor.
                ops.push(grant(0, entity.uid, 832051401));
            }
        }
    }
    if rose_and_slough {
        // The configured carrier uses RealHurtFix -500 and is single-instance.
        // Its separate early settlement of five negative statuses is not yet
        // implemented: Detonate2 cannot currently resolve all those families.
        for entity in &pool.attacker_main {
            if managers.hp.current(entity.uid) > 0 {
                ops.push(grant(entity.uid, entity.uid, 833031101));
            }
        }
    }
    if let Some(entry) = managers
        .rouge
        .context()
        .and_then(|context| context.entry.clone())
    {
        ops.extend(artefact_entry::opening(managers, pool, &entry));
    }
    let catalyst_opening = catalyst::opening(managers, pool, &ops);
    ops.extend(catalyst_opening);
    ops.extend(artefact_entry::paid_opening(managers, pool));
    ops
}

pub(crate) fn opening_cards(managers: &mut BattleManagers) -> Vec<RuleOp> {
    let Some(active) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    if active.opening_cards_done {
        return vec![];
    }
    active.opening_cards_done = true;
    active
        .context
        .entry
        .as_ref()
        .map(artefact_entry::opening_cards)
        .unwrap_or_default()
}

pub(crate) fn before_card(
    managers: &BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    skill_id: i32,
    card_index: i32,
    is_ultimate: bool,
) -> Vec<RuleOp> {
    artefact_turns::before_card(
        managers,
        pool,
        source_uid,
        skill_id,
        card_index,
        is_ultimate,
    )
}

pub(crate) fn fade_before_duration(
    managers: &BattleManagers,
    stage: i32,
    snapshot: &[(i64, i64)],
) -> Vec<RuleOp> {
    artefact_turns::fade_before_duration(managers, stage, snapshot)
}

pub(crate) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
) -> Vec<RuleOp> {
    let mut ops = artefact_balance::round(managers, round, true);
    ops.extend(catalyst::round_start(managers, pool, round, determinism));
    ops.extend(artefact_turns::round_start(
        managers,
        pool,
        round,
        determinism,
    ));
    ops.extend(artefact_ending::round_start(managers, pool, round));
    ops.extend(difficulty::round_start(managers, pool, round, determinism));
    ops
}

/// Observe events once at the manager commit boundary, not once per reaction
/// lane. The resulting ordinary commands keep packet projection and secondary
/// reactions on the same path as every other battle effect.
pub(crate) fn observe_events(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    determinism: &mut crate::engine::runtime::determinism::RoundDeterminism,
    events: &[BattleEvent],
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let bottle = context.artefacts.iter().any(|item| item.item_id == 8130315);
    let serums = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8210045)
        .count();
    let mut result = Vec::new();
    for event in events {
        result.extend(artefact_ending::observe(managers, pool, event));
        result.extend(artefact_ritual::observe(managers, pool, event));
        result.extend(artefact_electric::observe(
            managers,
            pool,
            event,
            round,
            determinism,
        ));
        growth::observe_bullet_change(managers, pool, event, determinism);
        result.extend(artefact_bullet::observe(managers, pool, event, determinism));
        result.extend(artefact_dolls::observe(managers, pool, event, determinism));
        result.extend(artefact_events::observe(
            managers,
            pool,
            event,
            round,
            determinism,
        ));
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
                    managers.rouge.active.as_mut().unwrap().aid_since_settlement = true;
                }
                statistics::record_action(managers, action, aid);
                result.extend(growth::completed_action(
                    managers,
                    pool,
                    action,
                    determinism,
                ));
                result.extend(artefact_turns::action(
                    managers,
                    pool,
                    action,
                    aid,
                    determinism,
                ));
                result.extend(catalyst::action(managers, action, aid));
                if bottle
                    && matches!(action.rank, 2 | 3)
                    && !managers.catalog().skill_is_big(action.skill_id)
                    && let Some(uid) = first_ally(managers, pool)
                {
                    result.push(grant(uid, uid, 5041));
                }
            }
            BattleEvent::EntityDied(death) => {
                let summary = &mut managers.rouge.active.as_mut().unwrap().summary;
                if pool.team_type(death.target_uid) == Some(1) {
                    summary.ally_deaths = summary.ally_deaths.saturating_add(1);
                } else if pool.team_type(death.target_uid) == Some(2)
                    && pool.source_is_attacker(death.source_uid)
                {
                    summary.enemy_kills = summary.enemy_kills.saturating_add(1);
                }
                if pool.source_is_attacker(death.source_uid)
                    && pool.team_type(death.target_uid) == Some(2)
                    && managers.hp.current(death.source_uid) > 0
                {
                    // Exact client states: Support I and random own Precast I.
                    // Credited killer remains an explicit local interpretation.
                    for _ in 0..serums {
                        result.push(grant(death.source_uid, death.source_uid, 90331));
                        result.push(grant(death.source_uid, death.source_uid, 90082));
                    }
                }
            }
            BattleEvent::ExPointChanged(change) => {
                result.extend(catalyst::ultimate_availability(
                    managers,
                    pool,
                    change.target_uid,
                ));
            }
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
    let mut result = artefact_balance::round(managers, round, false);
    result.extend(artefact_electric::round_end(managers, pool, round));
    result.extend(artefact_turns::round_end(managers, pool, round));
    let Some(state) = managers.rouge.active.as_ref() else {
        return result;
    };
    if !state.aid_since_settlement || state.shield_round == Some(round) {
        return result;
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
    let state = managers.rouge.active.as_mut().unwrap();
    state.aid_since_settlement = false;
    state.shield_round = Some(round);
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

#[cfg(test)]
mod artefact_lantern_tests;

#[cfg(test)]
mod support_tests;

#[cfg(test)]
mod growth_tests;

#[cfg(test)]
mod catalyst_tests;

#[cfg(test)]
mod bullet_tests;
