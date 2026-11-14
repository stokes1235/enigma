//! Rose and Slough's extra settlement, reusing each status's real damage rule.
//!
//! The description establishes the five families and action timing; the
//! repeated settlement, status ordering and interaction with other triggers
//! are local interpretations until a layout-linked capture is available.

use super::*;
use crate::engine::{
    event::kind::EventKind,
    runtime::determinism::RoundDeterminism,
    skill::{
        action::ActionEvent,
        buff_act::{self, registry::BuffActKind},
    },
};

pub(super) fn action(
    managers: &BattleManagers,
    pool: &TargetPool,
    action: &ActionEvent,
    random: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    if !action.mode.completes_action()
        || !pool.source_is_attacker(action.source_uid)
        || managers.hp.current(action.source_uid) <= 0
        || !managers
            .rouge
            .context()
            .is_some_and(|context| context.artefacts.iter().any(|item| item.item_id == 8330311))
    {
        return Vec::new();
    }

    let mut features = managers.buff.active_features(&managers.hp);
    features.retain(|feature| feature.owner_uid == action.source_uid);
    features.sort_by_key(|feature| (feature.buff_uid, feature.act_id().unwrap_or_default()));
    let mut ops = Vec::new();
    for feature in features {
        let type_id = managers.catalog().buff_type_id(feature.buff_id);
        let Some(subscriber) = buff_act::subscriber_from_feature(feature, EventKind::AllyAction)
        else {
            continue;
        };
        let kind = buff_act::subscriber_kind(&subscriber);
        match (type_id, kind) {
            // Bleed, Corrode, and Pollution have ordinary Dot subscribers;
            // Poison uses its recorded source-attack contribution instead.
            (6002 | 6004 | 3120001 | 107313303, Some(BuffActKind::Dot))
            | (6003, Some(BuffActKind::Poison)) => ops.extend(
                buff_act::damage_over_time::damage_rule_ops(managers, pool, random, &subscriber),
            ),
            // Ordinary, Searing and Venomous Gash share opcode 721, but
            // unrelated Fracture and other 721 statuses must not trigger.
            (90061 | 90251 | 90261, Some(BuffActKind::DotNoLimit)) => {
                if let Some(wound) = buff_act::dot_no_limit::trigger_rule_ops(managers, &subscriber)
                {
                    ops.extend(wound.into_iter().map(|entry| entry.op));
                }
            }
            _ => {}
        }
    }
    ops
}

#[cfg(test)]
#[path = "artefact_thorns_tests.rs"]
mod tests;
