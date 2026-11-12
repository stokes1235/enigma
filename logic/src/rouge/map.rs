//! Deterministic layer-map generation for the original Rouge (A Series of Dusks).
//!
//! The client positions nodes procedurally from `(stage, index within stage)` and
//! only needs the logical graph (stage / node id / links / event), so the server
//! owns the layout.
//!
//! The shipped `rouge_layer_rule` snapshot is empty (`{"version": ""}` for all 236
//! rows), so the node-type distribution below is an explicit placeholder anchored
//! on the base-game event types. Replace [`weights_for_stage`] once the rule data
//! is available; nothing else in the generator depends on it.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// `rouge_event.type` values that exist in the base game (9..=13 are expansion-only).
pub const NODE_MINOR_OBSTACLE: i32 = 1;
pub const NODE_PRESENT_DANGER: i32 = 2;
pub const NODE_TREASURE: i32 = 5;
pub const NODE_DISCOVERY: i32 = 6;
pub const NODE_SHOP: i32 = 7;
pub const NODE_REST: i32 = 8;
/// Final node of a layer: the mandatory crisis fight.
pub const NODE_BOSS: i32 = 4;

/// Default number of stages per layer (`rouge_const` 45).
pub const DEFAULT_STAGES: i32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MapNode {
    pub stage: i32,
    pub node_id: i32,
    pub last: Vec<i32>,
    pub next: Vec<i32>,
    pub event_id: i32,
    pub node_type: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerMap {
    pub layer_id: i32,
    pub stages: i32,
    pub nodes: Vec<MapNode>,
}

impl LayerMap {
    pub fn node(&self, node_id: i32) -> Option<&MapNode> {
        self.nodes.iter().find(|node| node.node_id == node_id)
    }

    pub fn start_node(&self) -> Option<&MapNode> {
        self.nodes.first()
    }
}

/// Node-type weights for a non-terminal stage.
///
/// Placeholder distribution: fights and discoveries dominate, with a shop, a
/// treasure and a rest site spread across the layer.
fn weights_for_stage(stage: i32, stages: i32) -> &'static [(i32, u32)] {
    const EARLY: &[(i32, u32)] = &[
        (NODE_MINOR_OBSTACLE, 40),
        (NODE_DISCOVERY, 35),
        (NODE_TREASURE, 15),
        (NODE_PRESENT_DANGER, 10),
    ];
    const MIDDLE: &[(i32, u32)] = &[
        (NODE_MINOR_OBSTACLE, 30),
        (NODE_DISCOVERY, 25),
        (NODE_PRESENT_DANGER, 20),
        (NODE_SHOP, 15),
        (NODE_TREASURE, 10),
    ];
    const LATE: &[(i32, u32)] = &[
        (NODE_PRESENT_DANGER, 35),
        (NODE_MINOR_OBSTACLE, 25),
        (NODE_SHOP, 20),
        (NODE_REST, 20),
    ];

    let third = stages / 3;
    if stage <= third {
        EARLY
    } else if stage <= third * 2 {
        MIDDLE
    } else {
        LATE
    }
}

fn pick_weighted(rng: &mut StdRng, weights: &[(i32, u32)]) -> i32 {
    let total: u32 = weights.iter().map(|(_, weight)| *weight).sum();
    let mut roll = rng.random_range(0..total);
    for (node_type, weight) in weights {
        if roll < *weight {
            return *node_type;
        }
        roll -= *weight;
    }
    weights.last().map(|(node_type, _)| *node_type).unwrap_or(0)
}

fn pick_event(rng: &mut StdRng, events: &HashMap<i32, Vec<i32>>, node_type: i32) -> i32 {
    match events.get(&node_type) {
        Some(ids) if !ids.is_empty() => ids[rng.random_range(0..ids.len())],
        _ => 0,
    }
}

/// Builds a connected, stage-layered map. Deterministic for a given `seed`.
pub fn generate(
    layer_id: i32,
    stages: i32,
    seed: u64,
    events: &HashMap<i32, Vec<i32>>,
) -> LayerMap {
    let stages = stages.max(3);
    let mut rng = StdRng::seed_from_u64(seed);

    // 1. Node ids per stage. First and last stages are single nodes.
    let mut per_stage: Vec<Vec<i32>> = Vec::with_capacity(stages as usize);
    for stage in 0..stages {
        let count = if stage == 0 || stage == stages - 1 {
            1
        } else {
            rng.random_range(1..=3)
        };
        per_stage.push((1..=count).map(|index| stage * 10 + index).collect());
    }

    // 2. Links between adjacent stages, keeping the graph fully connected.
    let mut last: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut next: HashMap<i32, Vec<i32>> = HashMap::new();
    for stage in 0..(stages - 1) {
        let sources = per_stage[stage as usize].clone();
        let targets = per_stage[(stage + 1) as usize].clone();
        for (index, source) in sources.iter().enumerate() {
            let start = index * targets.len() / sources.len();
            let end = ((index + 1) * targets.len() / sources.len())
                .max(start + 1)
                .min(targets.len());
            for target in &targets[start..end] {
                next.entry(*source).or_default().push(*target);
                last.entry(*target).or_default().push(*source);
            }
        }
        // Every node in the next stage must be reachable from somewhere.
        for target in &targets {
            if !last.contains_key(target) {
                let source = sources[0];
                next.entry(source).or_default().push(*target);
                last.entry(*target).or_default().push(source);
            }
        }
    }

    // 3. Node content.
    let mut nodes = Vec::new();
    for stage in 0..stages {
        for node_id in &per_stage[stage as usize] {
            let (node_type, event_id) = if stage == 0 {
                (0, 0)
            } else if stage == stages - 1 {
                (NODE_BOSS, pick_event(&mut rng, events, NODE_BOSS))
            } else {
                let node_type = pick_weighted(&mut rng, weights_for_stage(stage, stages));
                (node_type, pick_event(&mut rng, events, node_type))
            };
            nodes.push(MapNode {
                stage,
                node_id: *node_id,
                last: last.get(node_id).cloned().unwrap_or_default(),
                next: next.get(node_id).cloned().unwrap_or_default(),
                event_id,
                node_type,
            });
        }
    }

    // 4. Guarantee the utility nodes every layer is expected to offer.
    for required in [NODE_SHOP, NODE_REST, NODE_TREASURE] {
        if nodes.iter().any(|node| node.node_type == required) {
            continue;
        }
        let candidates: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.stage > 0 && node.stage < stages - 1)
            .map(|(index, _)| index)
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let pick = candidates[rng.random_range(0..candidates.len())];
        nodes[pick].node_type = required;
        nodes[pick].event_id = pick_event(&mut rng, events, required);
    }

    LayerMap {
        layer_id,
        stages,
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> HashMap<i32, Vec<i32>> {
        let mut map = HashMap::new();
        map.insert(NODE_MINOR_OBSTACLE, vec![101001, 101002]);
        map.insert(NODE_PRESENT_DANGER, vec![102001]);
        map.insert(NODE_TREASURE, vec![105001]);
        map.insert(NODE_DISCOVERY, vec![106001]);
        map.insert(NODE_SHOP, vec![107001]);
        map.insert(NODE_REST, vec![108001]);
        map.insert(NODE_BOSS, vec![104001]);
        map
    }

    #[test]
    fn generated_map_is_layered_connected_and_deterministic() {
        let map = generate(111, DEFAULT_STAGES, 42, &events());
        assert_eq!(map.stages, DEFAULT_STAGES);
        assert_eq!(map.layer_id, 111);

        // First and last stages hold a single node.
        let first: Vec<_> = map.nodes.iter().filter(|n| n.stage == 0).collect();
        let last: Vec<_> = map
            .nodes
            .iter()
            .filter(|n| n.stage == DEFAULT_STAGES - 1)
            .collect();
        assert_eq!(first.len(), 1);
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].node_type, NODE_BOSS);
        assert!(first[0].last.is_empty());
        assert!(last[0].next.is_empty());

        // Every non-first node has a predecessor, every non-last node a successor.
        for node in &map.nodes {
            if node.stage > 0 {
                assert!(!node.last.is_empty(), "node {node:?} has no predecessor");
            }
            if node.stage < DEFAULT_STAGES - 1 {
                assert!(!node.next.is_empty(), "node {node:?} has no successor");
            }
        }

        // Every layer offers a shop, a rest site and a treasure node.
        for required in [NODE_SHOP, NODE_REST, NODE_TREASURE] {
            assert!(
                map.nodes.iter().any(|node| node.node_type == required),
                "layer is missing node type {required}"
            );
        }

        // Same seed -> identical map.
        assert_eq!(map, generate(111, DEFAULT_STAGES, 42, &events()));
        // Different seed -> different map (overwhelmingly likely).
        assert_ne!(map, generate(111, DEFAULT_STAGES, 43, &events()));
    }

    #[test]
    fn every_node_reachable_from_the_start() {
        let map = generate(111, DEFAULT_STAGES, 7, &events());
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![map.start_node().unwrap().node_id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = map.node(id) {
                stack.extend(node.next.iter().copied());
            }
        }
        assert_eq!(seen.len(), map.nodes.len());
    }
}
