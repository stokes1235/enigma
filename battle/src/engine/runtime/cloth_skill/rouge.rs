use super::*;
use crate::engine::{
    runtime::record::{FrameItem, FrameOwner, FrameTrigger, SemanticFrame},
    skill::target::{TargetContext, TargetPool},
};

impl BattleRuntime {
    pub(super) fn use_rouge_catalyst(
        &mut self,
        request: &UseClothSkillRequest,
    ) -> Option<Vec<FightStep>> {
        if self.round.is_none()
            || self.outcome() != crate::BattleOutcome::Unfinished
            || request.from_id.unwrap_or_default() != 0
        {
            return None;
        }
        let skill_id = request.skill_id?;
        let pool = TargetPool::from_fight_with_catalog(self.managers.catalog(), &self.fight)
            .runtime_view(&self.managers);
        let target = request.to_id.unwrap_or_default();
        let ops = crate::rouge::catalyst::plan(
            &mut self.managers,
            &pool,
            skill_id,
            target,
            &mut self.determinism,
        )?;
        let costs = crate::rouge::catalyst::charge(
            &mut self.managers,
            skill_id,
            self.round_state.cur_round,
        )?;
        self.managers.begin_cloth_sequence();
        let context = TargetContext {
            battle_id: self.fight.battle_id.unwrap_or_default(),
            current_round: self.round_state.cur_round,
            runtime_target_uid: target,
            ..Default::default()
        };
        // Captured Rouge replies put 189/190 deltas before the skill frame.
        let mut result = drain::run_command_group(
            &mut self.managers,
            &pool,
            &self.catalog,
            &mut self.determinism,
            context,
            costs,
        )
        .ok()?;
        let effects = drain::run_command_group(
            &mut self.managers,
            &pool,
            &self.catalog,
            &mut self.determinism,
            context,
            ops,
        )
        .ok()?;
        let mut items = Vec::new();
        for frame in effects.frames {
            if matches!(frame.owner, FrameOwner::Command) {
                items.extend(frame.items);
            } else {
                items.push(FrameItem::Child(Box::new(frame)));
            }
        }
        result.frames.push(SemanticFrame {
            owner: FrameOwner::Skill {
                source_uid: 0,
                skill_id,
                card_index: 0,
                target_uid: Some(target),
            },
            trigger: FrameTrigger::Active,
            items,
        });
        self.sync_rouge_skill_infos();
        project_result(
            result,
            self.fight.version.unwrap_or_default(),
            self.absorb_hurt_map_layout,
        )
        .ok()
    }

    pub(in crate::engine::runtime) fn sync_rouge_skill_infos(&mut self) {
        if self.managers.rouge.resources().is_none() {
            return;
        }
        if let Some(team) = self.fight.attacker.as_mut() {
            for skill in &mut team.skill_infos {
                if let Some(id) = skill.skill_id {
                    if let Some(terms) = crate::rouge::catalyst::terms(&self.managers, id) {
                        skill.need_power = Some(terms.power_cost);
                        // No captured allLimit>0 base skill exists. Using cd=1
                        // while unavailable is a local display policy; the
                        // authoritative per-round and per-fight ledgers guard it.
                        skill.cd = Some(crate::rouge::catalyst::cooldown(
                            &self.managers,
                            id,
                            self.round_state.cur_round,
                        ));
                    }
                }
            }
        }
    }
}
