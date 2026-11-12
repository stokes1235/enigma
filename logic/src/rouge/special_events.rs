//! Original-Rouge story branches whose server scripts were not fully exported.
//!
//! Event/choice/item IDs and fixed resource amounts come from `rouge_effect`,
//! `rouge_choice`, `rouge_entrust`, and the supplied Wiki. Local conventions are
//! explicit: type-30 replacements use the first suitable reachable node, nested
//! choices change the current event, and an ambiguous HP cost uses current HP
//! with a nonlethal floor. No official spawn probability is inferred.
//!
//! The caller persists State inside Details and applies grants in the same
//! transaction as the originating choice/victory. This module does not save a
//! run or apply resource/inventory mutations.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::map::{LayerMap, MapNode};

pub const CHALICE_ENTRUST: i32 = 1_310_234;
pub const CHALICE_DISCOVERY: i32 = 106_141;
pub const CHALICE_BATTLE: i32 = 110_109;
pub const ANCIENT_CHALICE: i32 = 8_320_025;
pub const AACHEN_ROSE: i32 = 8_320_907;
pub const LITTLE_CONCH: i32 = 8_180_501;
pub const FOUNTAIN_EVENTS: [i32; 3] = [106113, 1061131, 1061132];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EffectSource {
    pub layer_id: i32,
    pub node_id: i32,
    /// The validated choice/piece ID, not an arbitrary client-provided token.
    pub origin_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementRequest {
    pub source: EffectSource,
    pub effect_id: i32,
    pub event_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub source: EffectSource,
    pub effect_id: i32,
    pub layer_id: i32,
    pub node_id: i32,
    pub previous_event: i32,
    pub event_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BattleSource {
    pub layer_id: i32,
    pub node_id: i32,
    pub event_id: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub pending: Vec<ReplacementRequest>,
    pub seen_effects: BTreeSet<(EffectSource, i32)>,
    pub placements: Vec<Placement>,
    pub rewarded_battles: BTreeSet<BattleSource>,
    /// Keys are run-seed/layer/node strings so JSON map serialization is stable.
    /// Receipts prevent reconnects from rerolling or applying the same reward.
    pub fountains: BTreeMap<String, FountainProgress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FountainSource {
    pub run_seed: u64,
    pub layer_id: i32,
    pub node_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FountainOutcome {
    pub source: FountainSource,
    pub event_id: i32,
    pub choice_id: i32,
    /// Net change from the displayed outcome, with no hidden upfront stake.
    pub coin_delta: i32,
    /// None means the player selected Leave and no random roll occurred.
    pub won: Option<bool>,
    pub next_event_id: Option<i32>,
    /// Candidate only: caller also checks the actual applied balance increased.
    /// Observation choices are absent from badge 19's configured three IDs.
    pub badge_choice: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FountainProgress {
    /// None means the event has ended.
    pub current_event: Option<i32>,
    pub outcomes: BTreeMap<i32, FountainOutcome>,
}

impl Default for FountainProgress {
    fn default() -> Self {
        Self {
            current_event: Some(FOUNTAIN_EVENTS[0]),
            outcomes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FountainResolution {
    Apply(FountainOutcome),
    /// Return current map info without applying this old currency delta again.
    Replayed(FountainOutcome),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FountainError {
    #[error("invalid Fountain source")]
    InvalidSource,
    #[error("unknown Fountain phase")]
    InvalidPhase,
    #[error("choice does not belong to this Fountain phase")]
    InvalidChoice,
    #[error("Fountain phase was skipped or has already ended")]
    PhaseMismatch,
    #[error("insufficient coins for the displayed Fountain threshold")]
    InsufficientCoin,
}

pub fn fountain_choice_ids(event: i32) -> Option<&'static [i32]> {
    match event {
        106113 => Some(&[10611301, 10611302, 10611303]),
        1061131 => Some(&[10611311, 10611312, 10611313]),
        1061132 => Some(&[10611321, 10611322, 10611323]),
        _ => None,
    }
}

fn fountain_won(source: FountainSource, choice: i32) -> bool {
    // The 50/50 chance is explicit in both Throw and Observe descriptions.
    // Only the deterministic sampler/seed derivation is a local convention.
    let mut value = source.run_seed
        ^ (source.layer_id as u64).rotate_left(13)
        ^ (source.node_id as u64).rotate_left(29)
        ^ (choice as u64).rotate_left(43);
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    (value ^ (value >> 31)) & 1 == 0
}

/// Resolve only this three-stage chain. All currency changes and the receipt
/// must be committed with the owning action. The data has no next-stage field:
/// advancing after each Throw/Observe, on either win or loss, is an explicit
/// local convention; it is not claimed as a captured official transition.
///
/// Stage two uses the displayed +60, not the conflicting rouge_drop +80.
/// Thresholds 30/80/120 are eligibility gates, never additional charges.
pub fn resolve_fountain(
    state: &mut State,
    source: FountainSource,
    event: i32,
    choice: i32,
    coin: i32,
) -> Result<FountainResolution, FountainError> {
    if source.layer_id <= 0 || source.node_id < 0 {
        return Err(FountainError::InvalidSource);
    }
    let phase = FOUNTAIN_EVENTS
        .iter()
        .position(|id| *id == event)
        .ok_or(FountainError::InvalidPhase)?;
    let menu = fountain_choice_ids(event).ok_or(FountainError::InvalidPhase)?;
    let selected = menu
        .iter()
        .position(|id| *id == choice)
        .ok_or(FountainError::InvalidChoice)?;
    let key = format!("{}:{}:{}", source.run_seed, source.layer_id, source.node_id);
    if let Some(previous) = state.fountains.get(&key) {
        if let Some(outcome) = previous
            .outcomes
            .get(&choice)
            .filter(|outcome| outcome.event_id == event)
        {
            return Ok(FountainResolution::Replayed(*outcome));
        }
        if previous.current_event != Some(event) {
            return Err(FountainError::PhaseMismatch);
        }
    } else if phase != 0 {
        return Err(FountainError::PhaseMismatch);
    }
    let leave = selected == 2;
    if !leave && coin < [30, 80, 120][phase] {
        return Err(FountainError::InsufficientCoin);
    }
    let won = (!leave).then(|| fountain_won(source, choice));
    let delta = match won {
        Some(true) => [30, 60, 120][phase],
        Some(false) => -[20, 40, 100][phase],
        None => 0,
    };
    let next = if leave {
        None
    } else {
        FOUNTAIN_EVENTS.get(phase + 1).copied()
    };
    let outcome = FountainOutcome {
        source,
        event_id: event,
        choice_id: choice,
        coin_delta: delta,
        won,
        next_event_id: next,
        badge_choice: (selected == 0 && won == Some(true) && delta > 0).then_some(choice),
    };
    let progress = state.fountains.entry(key).or_default();
    progress.current_event = next;
    progress.outcomes.insert(choice, outcome);
    Ok(FountainResolution::Apply(outcome))
}

/// Preserve the current node's path/position while changing its Fountain
/// phase. Caller clears the old choices/finished entry and interaction cursor.
/// A terminal result returns None; caller finishes the existing node normally.
pub fn fountain_transition_current(
    tables: &config::GameDB,
    graph: &mut LayerMap,
    outcome: &FountainOutcome,
) -> Result<Option<Transition>, RuleError> {
    let source = outcome.source;
    if graph.layer_id != source.layer_id {
        return Err(RuleError::InvalidNode(source.node_id));
    }
    let node = graph
        .nodes
        .iter_mut()
        .find(|node| node.node_id == source.node_id)
        .ok_or(RuleError::InvalidNode(source.node_id))?;
    if node.event_id != outcome.event_id
        || fountain_choice_ids(outcome.event_id).is_none_or(|ids| !ids.contains(&outcome.choice_id))
    {
        return Err(RuleError::InvalidEvent(outcome.event_id));
    }
    let Some(next) = outcome.next_event_id else {
        return Ok(None);
    };
    let expected = FOUNTAIN_EVENTS
        .iter()
        .position(|id| *id == outcome.event_id)
        .and_then(|index| FOUNTAIN_EVENTS.get(index + 1))
        .copied();
    if expected != Some(next) {
        return Err(RuleError::InvalidEvent(next));
    }
    let kind = event_type(tables, next)?;
    node.event_id = next;
    node.node_type = kind;
    Ok(Some(Transition {
        node_id: source.node_id,
        previous_event: outcome.event_id,
        event_id: next,
        effect_id: None,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueResult {
    NotType30,
    AlreadyRequested,
    Queued,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("unknown or expansion-only target event {0}")]
    InvalidEvent(i32),
    #[error("invalid type-30 effect {0}")]
    InvalidEffect(i32),
    #[error("unknown current node {0}")]
    InvalidNode(i32),
}

fn event_type(tables: &config::GameDB, event: i32) -> Result<i32, RuleError> {
    tables
        .rouge_event(event)
        .filter(|row| row.version.is_empty() && (1..=8).contains(&row.r#type))
        .map(|row| row.r#type)
        .ok_or(RuleError::InvalidEvent(event))
}

/// Queue a configured map replacement exactly once per originating action.
/// The request survives a reconnect or a layer that has no suitable node.
pub fn queue_effect(
    tables: &config::GameDB,
    state: &mut State,
    source: EffectSource,
    effect_id: i32,
) -> Result<QueueResult, RuleError> {
    let Some(row) = tables.rouge_effect.get(effect_id) else {
        return Ok(QueueResult::NotType30);
    };
    if row.r#type != 30 {
        return Ok(QueueResult::NotType30);
    }
    if !row.version.is_empty() {
        return Err(RuleError::InvalidEffect(effect_id));
    }
    let event_id = row
        .type_param
        .trim()
        .parse::<i32>()
        .map_err(|_| RuleError::InvalidEffect(effect_id))?;
    event_type(tables, event_id)?;
    if !state.seen_effects.insert((source, effect_id)) {
        return Ok(QueueResult::AlreadyRequested);
    }
    state.pending.push(ReplacementRequest {
        source,
        effect_id,
        event_id,
    });
    Ok(QueueResult::Queued)
}

fn descendants(graph: &LayerMap, from: i32) -> Result<BTreeSet<i32>, RuleError> {
    let start = graph.node(from).ok_or(RuleError::InvalidNode(from))?;
    let mut queue: VecDeque<i32> = start.next.iter().copied().collect();
    let mut reached = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if reached.insert(id)
            && let Some(node) = graph.node(id)
        {
            queue.extend(node.next.iter().copied());
        }
    }
    reached.remove(&from);
    Ok(reached)
}

fn is_scripted(tables: &config::GameDB, event: i32) -> bool {
    matches!(event, 106_151..=106_154 | 1_061_511 | 1_061_521 | 301_901)
        || tables.rouge_effect.iter().any(|effect| {
            effect.r#type == 30 && effect.type_param.trim().parse::<i32>().ok() == Some(event)
        })
}

fn replaceable(tables: &config::GameDB, node: &MapNode) -> bool {
    // Preserve start/leaf and the layer's guaranteed shop, respite and treasure.
    node.stage > 0
        && !node.next.is_empty()
        && matches!(node.node_type, 1 | 2 | 6)
        && !is_scripted(tables, node.event_id)
}

/// Place requests in stable order, preferring an existing node of the target
/// type. `after_node=None` means a newly generated layer, starting at its entry.
/// `resolved` must contain both finished nodes and choices already completed.
/// Missing space leaves the request queued; it never overwrites a resolved or
/// unreachable branch, a previously placed story, or the mandatory leaf.
pub fn place_pending(
    tables: &config::GameDB,
    state: &mut State,
    graph: &mut LayerMap,
    after_node: Option<i32>,
    resolved: &BTreeSet<i32>,
) -> Result<Vec<Placement>, RuleError> {
    let from = after_node
        .or_else(|| graph.start_node().map(|node| node.node_id))
        .ok_or(RuleError::InvalidNode(0))?;
    let from_stage = graph.node(from).ok_or(RuleError::InvalidNode(from))?.stage;
    let reachable = descendants(graph, from)?;
    // Validate first so an invalid later request cannot partially consume work.
    let types = state
        .pending
        .iter()
        .map(|request| event_type(tables, request.event_id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut placed = Vec::new();
    let mut remaining = Vec::new();
    for (request, target_type) in state.pending.iter().copied().zip(types) {
        let candidate = graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.stage > from_stage
                    && reachable.contains(&node.node_id)
                    && !resolved.contains(&node.node_id)
                    && replaceable(tables, node)
                    && !state.placements.iter().any(|placement| {
                        placement.layer_id == graph.layer_id && placement.node_id == node.node_id
                    })
            })
            .min_by_key(|(_, node)| (node.node_type != target_type, node.stage, node.node_id))
            .map(|(index, _)| index);
        let Some(index) = candidate else {
            remaining.push(request);
            continue;
        };
        let node = &mut graph.nodes[index];
        let placement = Placement {
            source: request.source,
            effect_id: request.effect_id,
            layer_id: graph.layer_id,
            node_id: node.node_id,
            previous_event: node.event_id,
            event_id: request.event_id,
        };
        node.event_id = request.event_id;
        node.node_type = target_type;
        placed.push(placement);
        state.placements.push(placement);
    }
    state.pending = remaining;
    Ok(placed)
}

/// Stop an unplaced quest event when its entrust is explicitly abandoned.
pub fn cancel_entrust(state: &mut State, entrust_id: i32) {
    state
        .pending
        .retain(|request| request.effect_id != entrust_id);
}

/// Explicit phase menus: dividing the exported choice IDs by 100 incorrectly
/// exposes the second-stage rewards in the first stage, and gives stage two no
/// choices at all (`1061511 -> 10615111/12`, likewise for `1061521`).
pub fn choice_ids(event: i32) -> Option<&'static [i32]> {
    if let Some(ids) = fountain_choice_ids(event) {
        return Some(ids);
    }
    Some(match event {
        CHALICE_DISCOVERY => &[10_614_101, 10_614_102],
        106_151 => &[10_615_101, 10_615_102],
        1_061_511 => &[10_615_111, 10_615_112],
        106_152 => &[10_615_201, 10_615_202],
        1_061_521 => &[10_615_211, 10_615_212],
        106_153 => &[10_615_301, 10_615_302],
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedReward {
    pub grant_items: &'static [i32],
    pub coin_gain: i32,
    /// A strict cost: reject when the current balance is below this amount.
    pub coin_cost: i32,
    /// Consume up to this amount; the conch choice explicitly permits zero.
    pub power_cost_up_to: i32,
    /// Local convention matches the base repair shop: lose this fraction of
    /// current life, keep dead heroes dead and living heroes at >=1 life unit.
    pub current_life_cost_per_mille: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceAction {
    ChangeCurrentEvent {
        event_id: i32,
        /// Present for a table-confirmed type-30 transition.
        effect_id: Option<i32>,
    },
    Reward(FixedReward),
    Finish,
}

/// Fixed outcomes from the displayed choice data. Returning None means this
/// module owns neither that event nor that choice/phase combination.
pub fn choice_action(event: i32, choice: i32) -> Option<ChoiceAction> {
    // Fountain requires its persisted random resolver, not the generic fixed
    // drop path. In particular the exported +80 stage-two row is not applied.
    if fountain_choice_ids(event).is_some() {
        return None;
    }
    if !choice_ids(event)?.contains(&choice) {
        return None;
    }
    Some(match choice {
        10_614_101 => ChoiceAction::ChangeCurrentEvent {
            event_id: CHALICE_BATTLE,
            effect_id: Some(10_614_101),
        },
        10_615_101 => ChoiceAction::ChangeCurrentEvent {
            event_id: 1_061_511,
            effect_id: None,
        },
        10_615_201 => ChoiceAction::ChangeCurrentEvent {
            event_id: 1_061_521,
            effect_id: None,
        },
        10_615_102 => ChoiceAction::Reward(FixedReward {
            coin_gain: 100,
            ..Default::default()
        }),
        10_615_111 => ChoiceAction::Reward(FixedReward {
            grant_items: &[LITTLE_CONCH],
            power_cost_up_to: 50,
            ..Default::default()
        }),
        10_615_211 => ChoiceAction::Reward(FixedReward {
            grant_items: &[ANCIENT_CHALICE],
            current_life_cost_per_mille: 750,
            ..Default::default()
        }),
        10_615_301 => ChoiceAction::Reward(FixedReward {
            grant_items: &[AACHEN_ROSE],
            coin_cost: 300,
            ..Default::default()
        }),
        _ => ChoiceAction::Finish,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub node_id: i32,
    pub previous_event: i32,
    pub event_id: i32,
    pub effect_id: Option<i32>,
}

/// Change the current node's script without changing its path or position.
/// The caller must remove this node from `choices`/`finished`, reset the old
/// interactive pipeline/retry count, and save everything atomically. Otherwise
/// the old phase's selection incorrectly marks the new phase as finished.
pub fn transition_current(
    tables: &config::GameDB,
    graph: &mut LayerMap,
    node_id: i32,
    choice: i32,
) -> Result<Option<Transition>, RuleError> {
    let index = graph
        .nodes
        .iter()
        .position(|node| node.node_id == node_id)
        .ok_or(RuleError::InvalidNode(node_id))?;
    let previous_event = graph.nodes[index].event_id;
    let Some(ChoiceAction::ChangeCurrentEvent {
        event_id,
        effect_id,
    }) = choice_action(previous_event, choice)
    else {
        return Ok(None);
    };
    let target_type = event_type(tables, event_id)?;
    graph.nodes[index].event_id = event_id;
    graph.nodes[index].node_type = target_type;
    Ok(Some(Transition {
        node_id,
        previous_event,
        event_id,
        effect_id,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleKeyReward {
    pub reward: FixedReward,
    pub complete_entrust: Option<i32>,
}

/// Extra story keys, separate from the battle's ordinary coin/drop reward.
/// Grant timing is a local convention: once at successful battle completion.
/// No key or completion is issued on failure or for an unrelated active quest.
pub fn complete_battle(
    state: &mut State,
    source: BattleSource,
    entrust_id: i32,
    won: bool,
) -> Option<BattleKeyReward> {
    if !won || state.rewarded_battles.contains(&source) {
        return None;
    }
    let (item, complete_entrust) = match source.event_id {
        CHALICE_BATTLE if entrust_id == CHALICE_ENTRUST => (ANCIENT_CHALICE, Some(CHALICE_ENTRUST)),
        301_901 => (AACHEN_ROSE, None),
        _ => return None,
    };
    state.rewarded_battles.insert(source);
    Some(BattleKeyReward {
        reward: FixedReward {
            grant_items: match item {
                ANCIENT_CHALICE => &[ANCIENT_CHALICE],
                _ => &[AACHEN_ROSE],
            },
            ..Default::default()
        },
        complete_entrust,
    })
}

/// Bounded local spawn rules for the missing server-side story prerequisites.
/// Entrust targets and nested phases only enter through their owning scripts.
/// The Wiki requires prior101 for103/104. The conch references in the text are
/// honored by requiring it before randomly offering106152/106153. Their exact
/// official ordering/weights are unknown, so no probability is invented here.
pub fn can_spawn_randomly(
    tables: &config::GameDB,
    event: i32,
    passed_endings: &[i32],
    owned_items: &[i32],
) -> bool {
    if event_type(tables, event).is_err() {
        return false;
    }
    if matches!(event, 1_061_511 | 1_061_521 | 1_061_131 | 1_061_132)
        || tables.rouge_effect.iter().any(|effect| {
            effect.r#type == 30 && effect.type_param.trim().parse::<i32>().ok() == Some(event)
        })
    {
        return false;
    }
    match event {
        301_901 | 106_151 => passed_endings.contains(&101),
        106_152 | 106_153 => passed_endings.contains(&101) && owned_items.contains(&LITTLE_CONCH),
        _ => true,
    }
}

#[cfg(test)]
#[path = "fountain_tests.rs"]
mod fountain_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> &'static config::GameDB {
        super::super::original_tests::init_config();
        config::configs::get()
    }

    fn source(origin_id: i32) -> EffectSource {
        EffectSource {
            layer_id: 131,
            node_id: 0,
            origin_id,
        }
    }

    fn graph() -> LayerMap {
        LayerMap {
            layer_id: 141,
            stages: 5,
            nodes: vec![
                MapNode {
                    stage: 0,
                    node_id: 1,
                    last: vec![],
                    next: vec![11, 12],
                    event_id: 0,
                    node_type: 0,
                },
                MapNode {
                    stage: 1,
                    node_id: 11,
                    last: vec![1],
                    next: vec![21],
                    event_id: 106103,
                    node_type: 6,
                },
                MapNode {
                    stage: 1,
                    node_id: 12,
                    last: vec![1],
                    next: vec![21],
                    event_id: 101101,
                    node_type: 1,
                },
                MapNode {
                    stage: 2,
                    node_id: 21,
                    last: vec![11, 12],
                    next: vec![31],
                    event_id: 107010,
                    node_type: 7,
                },
                MapNode {
                    stage: 3,
                    node_id: 31,
                    last: vec![21],
                    next: vec![41],
                    event_id: 106103,
                    node_type: 6,
                },
                MapNode {
                    stage: 4,
                    node_id: 41,
                    last: vec![31],
                    next: vec![],
                    event_id: 103401,
                    node_type: 3,
                },
            ],
        }
    }

    #[test]
    fn accepted_chalice_entrust_survives_reconnect_and_places_only_once() {
        let tables = tables();
        let mut state = State::default();
        assert_eq!(
            queue_effect(tables, &mut state, source(CHALICE_ENTRUST), CHALICE_ENTRUST).unwrap(),
            QueueResult::Queued
        );
        let saved = serde_json::to_string(&state).unwrap();
        let mut state: State = serde_json::from_str(&saved).unwrap();
        let mut graph = graph();
        let original = graph.clone();
        let placed = place_pending(tables, &mut state, &mut graph, None, &BTreeSet::new()).unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].node_id, 11);
        assert_eq!(graph.node(11).unwrap().event_id, CHALICE_DISCOVERY);
        assert!(state.pending.is_empty());
        for (before, after) in original.nodes.iter().zip(&graph.nodes) {
            assert_eq!((&before.last, &before.next), (&after.last, &after.next));
            if before.node_id != 11 {
                assert_eq!(before, after);
            }
        }
        assert_eq!(
            queue_effect(tables, &mut state, source(CHALICE_ENTRUST), CHALICE_ENTRUST).unwrap(),
            QueueResult::AlreadyRequested
        );
        assert!(
            place_pending(tables, &mut state, &mut graph, None, &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn replacements_preserve_resolved_nodes_sibling_branches_and_utility_leaf() {
        let tables = tables();
        let mut state = State::default();
        queue_effect(tables, &mut state, source(CHALICE_ENTRUST), CHALICE_ENTRUST).unwrap();
        let mut graph = graph();
        let placed = place_pending(
            tables,
            &mut state,
            &mut graph,
            Some(11),
            &BTreeSet::from([21]),
        )
        .unwrap();
        assert_eq!(placed[0].node_id, 31);
        assert_eq!(graph.node(12).unwrap().event_id, 101101);
        assert_eq!(graph.node(21).unwrap().node_type, 7);
        assert_eq!(graph.node(41).unwrap().node_type, 3);

        let mut blocked = State::default();
        queue_effect(
            tables,
            &mut blocked,
            source(CHALICE_ENTRUST),
            CHALICE_ENTRUST,
        )
        .unwrap();
        assert!(
            place_pending(tables, &mut blocked, &mut graph, Some(31), &BTreeSet::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            blocked.pending.len(),
            1,
            "carry forward rather than overwrite the leaf"
        );
        cancel_entrust(&mut blocked, CHALICE_ENTRUST);
        assert!(blocked.pending.is_empty());
    }

    #[test]
    fn current_discovery_changes_into_the_configured_quest_fight() {
        let tables = tables();
        let mut graph = graph();
        graph.nodes[1].event_id = CHALICE_DISCOVERY;
        let change = transition_current(tables, &mut graph, 11, 10_614_101)
            .unwrap()
            .unwrap();
        assert_eq!(change.event_id, CHALICE_BATTLE);
        assert_eq!(change.effect_id, Some(10_614_101));
        assert_eq!(
            tables.rouge_fight_episode(graph.node(11).unwrap().event_id),
            Some(2902507)
        );
        assert_eq!(graph.node(11).unwrap().node_type, 2);
        assert!(
            transition_current(tables, &mut graph, 11, 10_614_101)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn nested_menus_cannot_skip_directly_to_rewards() {
        let tables = tables();
        for (first, second, next, terminal) in [
            (106151, 10615101, 1061511, 10615111),
            (106152, 10615201, 1061521, 10615211),
        ] {
            assert!(!choice_ids(first).unwrap().contains(&terminal));
            assert!(choice_action(first, terminal).is_none());
            let mut graph = graph();
            graph.nodes[1].event_id = first;
            transition_current(tables, &mut graph, 11, second)
                .unwrap()
                .unwrap();
            assert_eq!(graph.node(11).unwrap().event_id, next);
            assert!(choice_ids(next).unwrap().contains(&terminal));
            assert!(matches!(
                choice_action(next, terminal),
                Some(ChoiceAction::Reward(_))
            ));
        }
        for event in [106141, 106151, 1061511, 106152, 1061521, 106153] {
            for id in choice_ids(event).unwrap() {
                assert!(
                    tables.rouge_choice.get(*id).is_some(),
                    "choice{id} must be real"
                );
            }
        }
    }

    #[test]
    fn key_choice_costs_are_bounded_by_the_displayed_data() {
        let Some(ChoiceAction::Reward(conch)) = choice_action(1061511, 10615111) else {
            panic!()
        };
        assert_eq!(conch.grant_items, &[LITTLE_CONCH]);
        assert_eq!(conch.power_cost_up_to, 50);
        let Some(ChoiceAction::Reward(chalice)) = choice_action(1061521, 10615211) else {
            panic!()
        };
        assert_eq!(chalice.grant_items, &[ANCIENT_CHALICE]);
        assert_eq!(chalice.current_life_cost_per_mille, 750);
        let Some(ChoiceAction::Reward(rose)) = choice_action(106153, 10615301) else {
            panic!()
        };
        assert_eq!(rose.grant_items, &[AACHEN_ROSE]);
        assert_eq!(rose.coin_cost, 300);
        assert_eq!(choice_action(1061511, 10615112), Some(ChoiceAction::Finish));
        assert_eq!(choice_action(106141, 10614102), Some(ChoiceAction::Finish));
    }

    #[test]
    fn successful_battle_keys_are_extra_once_only_and_match_the_active_entrust() {
        let mut state = State::default();
        let source = BattleSource {
            layer_id: 141,
            node_id: 11,
            event_id: CHALICE_BATTLE,
        };
        assert!(complete_battle(&mut state, source, CHALICE_ENTRUST, false).is_none());
        assert!(complete_battle(&mut state, source, 0, true).is_none());
        let reward = complete_battle(&mut state, source, CHALICE_ENTRUST, true).unwrap();
        assert_eq!(reward.reward.grant_items, &[ANCIENT_CHALICE]);
        assert_eq!(reward.complete_entrust, Some(CHALICE_ENTRUST));
        let mut state: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(complete_battle(&mut state, source, CHALICE_ENTRUST, true).is_none());
        let aachen = BattleSource {
            event_id: 301901,
            ..source
        };
        assert_eq!(
            complete_battle(&mut state, aachen, 0, true)
                .unwrap()
                .reward
                .grant_items,
            &[AACHEN_ROSE]
        );
    }

    #[test]
    fn random_generation_never_spawns_internal_phases_or_locked_story_keys() {
        let tables = tables();
        for event in [106141, 110109, 1061511, 1061521] {
            assert!(!can_spawn_randomly(tables, event, &[101], &[LITTLE_CONCH]));
        }
        for event in [106151, 301901] {
            assert!(!can_spawn_randomly(tables, event, &[], &[]));
            assert!(!can_spawn_randomly(tables, event, &[102], &[LITTLE_CONCH]));
            assert!(can_spawn_randomly(tables, event, &[101], &[]));
        }
        for event in [106152, 106153] {
            assert!(!can_spawn_randomly(tables, event, &[101], &[]));
            assert!(!can_spawn_randomly(tables, event, &[], &[LITTLE_CONCH]));
            assert!(!can_spawn_randomly(tables, event, &[102], &[LITTLE_CONCH]));
            assert!(can_spawn_randomly(tables, event, &[101], &[LITTLE_CONCH]));
        }
        assert!(can_spawn_randomly(tables, 106103, &[], &[]));
        assert!(!can_spawn_randomly(tables, -1, &[101], &[LITTLE_CONCH]));
        for event in tables
            .rouge_event
            .iter()
            .filter(|event| !event.version.is_empty())
        {
            assert!(!can_spawn_randomly(
                tables,
                event.id,
                &[101],
                &[LITTLE_CONCH]
            ));
        }
    }
}
