//! Damage-rate procs that belong to a single actual skill execution.
//!
//! Original Rouge Fowling Piece has two independent branches: the fixed
//! Bullet bonus is represented in `conditional_modifiers`; an extra action
//! independently rolls to gain another 60% damage bonus for that execution.
use super::{Artefact, AttrId, BattleManagers};
use crate::engine::runtime::determinism::RoundDeterminism;

/// Roll once per placed instance, before damage is snapshotted. Multiple
/// copies roll independently in stable UID order; their bonuses add. The
/// frozen telescope coverage amplifies this artefact's own probability.
pub(crate) fn fowling_piece_bonus(
    managers: &BattleManagers,
    is_extra_action: bool,
    determinism: &mut RoundDeterminism,
) -> Vec<(AttrId, i32)> {
    if !is_extra_action {
        return vec![];
    }
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let mut sources: Vec<&Artefact> = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8210711)
        .collect();
    sources.sort_unstable_by_key(|item| item.uid);
    sources
        .into_iter()
        .filter_map(|item| {
            let probability =
                item.proc_chance(500_i32.saturating_add(item.count2.saturating_mul(10)));
            determinism
                .roll_permille(probability)
                .then_some((AttrId::DmgBonus, 600))
        })
        .collect()
}

#[cfg(test)]
#[path = "artefact_attack_proc_tests.rs"]
mod tests;
