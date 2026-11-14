//! Permanent growth from actual in-battle coin consumption.
//!
//! Only frozen, placed sources are eligible. Each fight's remainder resets;
//! the report carries new whole percentage points and is persisted only by the
//! server's existing successful-settlement watermark. A 100% lifetime cap is
//! applied before mutating this fight. HP growth uses the entity's unscaled
//! base attribute when available, falling back to the initial maximum for
//! entities without one. It accumulates all new HP-growth points for each
//! actor before rounding; increasing max preserves lost HP. Since the initial
//! Rouge-wide multiplier was already rounded separately, a newly built fight
//! can differ by one HP; the exact pre-rounding rate is not reconstructed.
use super::*;
use crate::engine::manager::hp::{HpCommand, MaxHpAdjust};

const KEY: &str = "growth_pct";

pub(super) fn spend(managers: &mut BattleManagers, spent: i32) -> Vec<RuleOp> {
    if spent <= 0 {
        return vec![];
    }
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let sources = context
        .growth_sources
        .iter()
        .filter(|source| {
            matches!(source.item_id, 8310608 | 8320617)
                && context
                    .artefacts
                    .iter()
                    .any(|item| item.uid == source.uid && item.item_id == source.item_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let old = context.permanent_growth.clone();
    let mut ops = Vec::new();
    for source in &sources {
        let baseline = old
            .get(&source.uid)
            .and_then(|values| values.get(KEY))
            .copied()
            .unwrap_or_default()
            .clamp(0, 100);
        let state = managers.rouge.active.as_mut().unwrap();
        let remainder = state.coin_growth_remainders.entry(source.uid).or_default();
        let amount = remainder.saturating_add(spent);
        let units = amount / 10;
        *remainder = amount % 10;
        let before = state
            .counters
            .get(&source.uid)
            .and_then(|values| values.get(KEY))
            .copied()
            .unwrap_or_default()
            .max(0);
        let increase = units.min(
            100_i32
                .saturating_sub(baseline)
                .saturating_sub(before)
                .max(0),
        );
        if increase <= 0 {
            continue;
        }
        state
            .counters
            .entry(source.uid)
            .or_default()
            .insert(KEY.into(), before.saturating_add(increase));
        for uid in source.actor_uids.iter().copied().collect::<BTreeSet<_>>() {
            if managers.entity.team_type(uid) != Some(1) {
                continue;
            }
            if source.item_id == 8310608 {
                managers
                    .attribute
                    .add(uid, AttrId::DmgBonus, increase.saturating_mul(10));
            } else {
                // A hero's seeded maximum already includes this artefact's old
                // growth and other Rouge HP rates. The untouched base_attr is
                // the correct percentage base when the entity provides it.
                let base = managers
                    .entity
                    .snapshot(uid)
                    .and_then(|entity| entity.base_attr.and_then(|attr| attr.hp))
                    .filter(|hp| *hp > 0)
                    .unwrap_or_else(|| managers.hp.base_max(uid).max(0));
                let state = managers.rouge.active.as_ref().unwrap();
                let total_after = sources
                    .iter()
                    .filter(|other| other.item_id == 8320617 && other.actor_uids.contains(&uid))
                    .map(|other| other.uid)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|other_uid| {
                        state
                            .counters
                            .get(&other_uid)
                            .and_then(|values| values.get(KEY))
                            .copied()
                            .unwrap_or_default()
                            .max(0)
                    })
                    .fold(0_i64, |sum, value| sum.saturating_add(i64::from(value)));
                let total_before = total_after.saturating_sub(i64::from(increase));
                // Calculate the difference of cumulative rounded totals so
                // spending in multiple steps or across items does not lose HP.
                let base = i64::from(base);
                let after = base.saturating_mul(total_after) / 100;
                let before_amount = base.saturating_mul(total_before) / 100;
                let delta = (after - before_amount).clamp(0, i64::from(i32::MAX)) as i32;
                if delta > 0 {
                    ops.push(RuleOp::Command(BattleCommand::Hp(HpCommand::AdjustMax(
                        MaxHpAdjust {
                            origin: ORIGIN,
                            source_uid: uid,
                            target_uid: uid,
                            delta,
                        },
                    ))));
                }
            }
        }
    }
    ops
}

#[cfg(test)]
#[path = "artefact_coin_growth_tests.rs"]
mod tests;
