//! Local entry wiring for the two Storm-area replacement artefacts.
//!
//! The client supplies actual rule skills for these replacements: 8018106
//! applies its marker and three stacks of Protection, while 8018105 supplies
//! its marker and heals 10% of the recipient's lost HP. Their exact ownership
//! by this server's region rule is a local inference: the currently available
//! battle capture does not contain an associated layout or both artefacts.

use super::{BattleManagers, RougeContext, RuleOp, grant};
use crate::{
    catalog::BattleCatalog,
    engine::fight::rules::{ATTACKER_SIDE_UID, AdditionRuleType, BattleRuleSide},
};
use sonettobuf::Fight;

const STORM_SLOW_RULE: i32 = 8018101;
const STORM_REWRITE_RULE: i32 = 8018104;
const HEAL_REPLACEMENT: i32 = 8018105;
const PROTECTION_REPLACEMENT: i32 = 8018106;

/// Attach the configured replacement skills to allied actors, including
/// reserves, before the runtime constructs its entity and skill catalogs.
/// The normal area rules themselves remain in place: their NoBuffId condition
/// ignores marked owners. Do not suppress similarly named enemy effects.
pub(crate) fn prepare(catalog: BattleCatalog, fight: &mut Fight, context: &RougeContext) {
    let rules = catalog.battle_rules(fight);
    let rule_is_attacker = |skill_id, rule_type| {
        rules.iter().any(|rule| {
            rule.skill_id == skill_id
                && rule.rule_type == rule_type
                && matches!(rule.side, BattleRuleSide::Attacker | BattleRuleSide::Both)
        })
    };
    let protection = context.artefacts.iter().any(|item| item.item_id == 8330615)
        && rule_is_attacker(STORM_SLOW_RULE, AdditionRuleType::Skill);
    let healing = context.artefacts.iter().any(|item| item.item_id == 8330710)
        && rule_is_attacker(STORM_REWRITE_RULE, AdditionRuleType::FightSkill);
    if !protection && !healing {
        return;
    }
    let Some(team) = fight.attacker.as_mut() else {
        return;
    };
    for entity in team
        .entitys
        .iter_mut()
        .chain(&mut team.sub_entitys)
        .chain(&mut team.sp_entitys)
        .chain(&mut team.sp_fight_entities)
    {
        for skill_id in [
            protection.then_some(PROTECTION_REPLACEMENT),
            healing.then_some(HEAL_REPLACEMENT),
        ]
        .into_iter()
        .flatten()
        {
            if !entity.passive_skill.contains(&skill_id) {
                entity.passive_skill.push(skill_id);
            }
        }
    }
}

/// Rule 8018104 is owned by the attacker's battlefield, rather than by a
/// hero. Its NoBuffId predicate therefore needs the same official 8018105
/// marker on that owner. The hero-owned replacement skill grants it to the
/// individual actors separately. Only this precise area skill is gated.
pub(super) fn opening_marker(managers: &BattleManagers) -> Vec<RuleOp> {
    let has_contract = managers
        .rouge
        .context()
        .is_some_and(|context| context.artefacts.iter().any(|item| item.item_id == 8330710));
    let has_area_rule = managers
        .battle_rule
        .owned_skills()
        .any(|(uid, skill)| uid == ATTACKER_SIDE_UID && skill == STORM_REWRITE_RULE);
    if has_contract && has_area_rule {
        vec![grant(
            ATTACKER_SIDE_UID,
            ATTACKER_SIDE_UID,
            HEAL_REPLACEMENT,
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "artefact_storm_tests.rs"]
mod tests;
