use super::*;
use crate::engine::manager::hp::HpChanges;

/// Called once by the durable HP commit, never by HP previews or packet
/// projection. Includes shared hurt and periodic damage which publish no Hit.
pub(crate) fn record_hp(managers: &mut BattleManagers, changes: &HpChanges, is_heal: bool) {
    let Some(state) = managers.rouge.active.as_mut() else {
        return;
    };
    for change in changes.with_shares() {
        let source_team = managers.entity.team_type(change.source_uid);
        let target_team = managers.entity.team_type(change.target_uid);
        let damage = i64::from(change.applied_damage().max(0));
        if target_team == Some(1) {
            state.summary.damage_taken = state.summary.damage_taken.saturating_add(damage);
            if change.hp.is_some_and(|hp| hp.before == 0 && hp.after > 0) {
                state.summary.ally_revives = state.summary.ally_revives.saturating_add(1);
            }
        }
        if source_team == Some(1)
            && target_team == Some(2)
            && damage > 0
            && (change.damage.is_some() || change.hp.and_then(|hp| hp.hurt).is_some())
        {
            state.summary.damage_dealt = state.summary.damage_dealt.saturating_add(damage);
            state.summary.max_single_damage = state.summary.max_single_damage.max(damage);
        }
        if is_heal && source_team == Some(1) && target_team == Some(1) {
            let healed = i64::from(change.hp.map_or(0, |hp| hp.delta.max(0)));
            state.summary.healing_done = state.summary.healing_done.saturating_add(healed);
        }
    }
}

pub(super) fn record_action(
    managers: &mut BattleManagers,
    action: &crate::engine::skill::action::ActionEvent,
    support: bool,
) {
    let ultimate = managers.catalog().skill_is_big(action.skill_id);
    let extra = crate::engine::skill::action::performs_extra_action(
        action.card_index,
        action.mode,
        action.extra_skill_kind,
    );
    let summary = &mut managers.rouge.active.as_mut().unwrap().summary;
    summary.casts = summary.casts.saturating_add(1);
    summary.max_action_damage = summary
        .max_action_damage
        .max(i64::from(action.damage_amount.max(0)));
    let rank = if ultimate {
        Some(&mut summary.ultimate_casts)
    } else {
        match action.rank {
            1 => Some(&mut summary.rank1_casts),
            2 => Some(&mut summary.rank2_casts),
            3 => Some(&mut summary.rank3_casts),
            _ => None,
        }
    };
    if let Some(rank) = rank {
        *rank = rank.saturating_add(1);
    }
    if support {
        summary.support_casts = summary.support_casts.saturating_add(1);
    }
    if extra {
        summary.extra_action_casts = summary.extra_action_casts.saturating_add(1);
    }
}
