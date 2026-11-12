//! Base Rouge Anticipation: Support I/II/III (90331..90333).
//! The verified grammar is rank#1#1; other AddSupportCard modes stay unsupported.
use crate::engine::{
    event::{kind::EventKind, payload::BattleEvent},
    manager::{
        buff::{BuffCommand, BuffRemove, BuffRemoveSelector},
        card::{CardAddTemporary, CardCommand, TemporaryCardKind},
    },
    skill::{
        rule::output::{BattleCommand, RuleOp},
        subscriber::BuffActSubscriber,
    },
};

use super::{
    BuffActRuleOp,
    registry::{BuffActKind, RuntimeContext},
};

pub fn supports(args: &[i32]) -> bool {
    matches!(args,[rank,1,1] if (1..=3).contains(rank))
}

fn remove_once(subscriber: &BuffActSubscriber) -> Option<BuffActRuleOp> {
    Some(BuffActRuleOp::separate_independent_command(
        RuleOp::Command(BattleCommand::Buff(BuffCommand::RemoveAfterTrigger(
            BuffRemove {
                origin: super::command_origin(subscriber)?,
                target_uid: subscriber.owner_uid,
                selector: BuffRemoveSelector::Uid(subscriber.buff_uid),
            },
        ))),
    ))
}

pub fn subscriber_rule_ops(context: &mut RuntimeContext<'_>) -> Option<Vec<BuffActRuleOp>> {
    let subscriber = context.subscriber;
    if !super::subscriber_is_kind(subscriber, BuffActKind::AddSupportCard)
        || !supports(&subscriber.args)
        || !matches!(
            context.event,
            Some(BattleEvent::Kind(EventKind::RoundStartCard))
        )
    {
        return None;
    }
    let rouge = context.managers.rouge.context()?;
    let mapping = rouge
        .support_skills
        .iter()
        .find(|mapping| mapping.owner_uid == subscriber.owner_uid && mapping.hero_id > 0);
    let selected = mapping.and_then(|mapping| {
        mapping
            .skill_ids
            .iter()
            .copied()
            .find(|id| {
                context.managers.catalog().skill_rank(*id) == subscriber.args[0]
                    && !context.managers.catalog().skill_is_big(*id)
            })
            .map(|skill_id| (mapping.hero_id, skill_id))
    });
    let mut result = Vec::new();
    if subscriber.team_type == 1
        && subscriber.owner_alive
        && let Some((hero_id, skill_id)) = selected
    {
        let origin = super::command_origin(subscriber)?;
        for _ in 0..subscriber.amount.max(0) {
            result.push(BuffActRuleOp::subscriber_from_owner(RuleOp::Command(
                BattleCommand::Card(CardCommand::AddTemporary(CardAddTemporary {
                    origin,
                    target_uid: subscriber.owner_uid,
                    skill_id,
                    hero_id: Some(hero_id),
                    reserve_id: i64::from(hero_id),
                    team_type: 1,
                    kind: TemporaryCardKind::SupportSkill,
                })),
            )));
        }
    }
    // A missing frozen mapping is a deterministic no-card result, not a
    // substitution with the host's ordinary incantation or an endless retry.
    result.push(remove_once(subscriber)?);
    Some(result)
}
