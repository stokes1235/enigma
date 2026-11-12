//! Conditional rates for one skill's damage only; never add these to the
//! permanent Fight entity attributes. Values come from the saved Wiki's
//! explicit base artefact descriptions and rouge_item_static_desc.

use super::{count_tag_in_range, inlay_counters, is_active, only_support_below};
use crate::rouge::bag::RunBag;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttackPredicate {
    RankAtLeast(i32),
    Support,
    ExtraAction,
    HasBullet,
    NonUltimate,
    Ultimate,
    All(Vec<AttackPredicate>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionalModifier {
    /// Artefact instance UID, never a hero UID.
    pub source_uid: i64,
    pub item_id: i32,
    pub predicate: AttackPredicate,
    pub attr_id: i32,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackContext {
    pub rank: i32,
    pub is_ultimate: bool,
    pub is_support: bool,
    pub is_extra_action: bool,
    pub has_bullet: bool,
}

impl AttackPredicate {
    pub fn matches(&self, context: &AttackContext) -> bool {
        match self {
            Self::RankAtLeast(rank) => context.rank >= *rank,
            Self::Support => context.is_support,
            Self::ExtraAction => context.is_extra_action,
            Self::HasBullet => context.has_bullet,
            Self::NonUltimate => !context.is_ultimate,
            Self::Ultimate => context.is_ultimate,
            Self::All(conditions) => conditions
                .iter()
                .all(|condition| condition.matches(context)),
        }
    }
}

pub fn attack_modifiers(tables: &config::GameDB, bag: &RunBag) -> Vec<ConditionalModifier> {
    let mut result = Vec::new();
    for item in bag.items.iter().filter(|item| is_active(item)) {
        let count2 = inlay_counters(tables, item)
            .get("count2")
            .copied()
            .unwrap_or(0);
        let high_rank = || {
            AttackPredicate::All(vec![
                AttackPredicate::RankAtLeast(2),
                AttackPredicate::NonUltimate,
            ])
        };
        let (predicate, attr_id, value) = match item.item_id {
            8110312 => (high_rank(), 205, 200 + count2 * 10),
            8210313 => (high_rank(), 205, 300 + count2 * 10),
            8230305 => (high_rank(), 203, 300),
            8330306 => (high_rank(), 203, 600),
            8210113 => (
                AttackPredicate::Support,
                213,
                100 + count2 * 10
                    + if only_support_below(item, bag) {
                        100
                    } else {
                        0
                    },
            ),
            8310108 => (AttackPredicate::Support, 205, 300),
            8310706 => (
                AttackPredicate::ExtraAction,
                205,
                500 + count2 * 10 + count_tag_in_range(item, bag, 7) * 200,
            ),
            _ => continue,
        };
        result.push(ConditionalModifier {
            source_uid: item.uid,
            item_id: item.item_id,
            predicate,
            attr_id,
            value,
        });
    }
    result
}

/// Pure reference evaluator. The battle crate may translate these predicates
/// into its equivalent context and apply the resulting deltas transiently.
pub fn evaluate_attack(
    tables: &config::GameDB,
    bag: &RunBag,
    context: &AttackContext,
) -> BTreeMap<i32, i32> {
    let mut result = BTreeMap::new();
    for modifier in attack_modifiers(tables, bag) {
        if modifier.predicate.matches(context) {
            let total: &mut i32 = result.entry(modifier.attr_id).or_default();
            *total = total.saturating_add(modifier.value);
        }
    }
    result
}
