//! Opening-only spell-acquisition capacity for placed inspiration artefacts.

use super::*;
use crate::engine::{
    manager::card::{CardCommand, CardHandLimitChange, start::hand_size_from_count},
    mechanic::card::CardMechanic,
};

const ORIGIN: CommandOrigin = CommandOrigin {
    domain: RuleDomain::Behavior,
    key: DefinitionKey::new(40007, "AddActAndCardLimit"),
};

pub(super) fn opening(managers: &BattleManagers, pool: &TargetPool) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return Vec::new();
    };
    // The supplied text only prohibits stacking 8240302. Until captured
    // evidence resolves the other cases, each placed 8140301 adds one, and
    // it stacks with the single +2 from any number of placed 8240302s. The
    // context contains physical placements, not stored or inserted objects.
    let small = i32::try_from(
        context
            .artefacts
            .iter()
            .filter(|artefact| artefact.item_id == 8140301)
            .count(),
    )
    .unwrap_or(i32::MAX);
    let big = if context
        .artefacts
        .iter()
        .any(|artefact| artefact.item_id == 8240302)
    {
        2
    } else {
        0
    };
    let delta = small.saturating_add(big);
    if delta == 0 {
        return Vec::new();
    }

    // The effect is team-wide; select a real living deployed actor as its
    // packet target. Which actor receives the marker is a local inference.
    let Some(target) = pool
        .attacker_main
        .iter()
        .find(|entity| managers.hp.current(entity.uid) > 0)
    else {
        return Vec::new();
    };
    let base = hand_size_from_count(
        pool.attacker_main
            .iter()
            .filter(|entity| managers.hp.current(entity.uid) > 0)
            .count(),
        managers.fight_version(),
    );
    let current = CardMechanic.normal_hand_limit(base, managers, pool);
    let resulting_limit = i32::try_from(current)
        .unwrap_or(i32::MAX)
        .saturating_add(delta);

    // Use one command for the aggregate so the published absolute limit is
    // correct even when multiple artefacts are placed. CardSetup draws against
    // it, then the existing start schedule clears the temporary bonus.
    vec![RuleOp::Command(BattleCommand::Card(
        CardCommand::ChangeHandLimit(CardHandLimitChange {
            origin: ORIGIN,
            target_uid: target.uid,
            delta,
            resulting_limit,
        }),
    ))]
}

#[cfg(test)]
#[path = "artefact_opening_hand_tests.rs"]
mod tests;
