//! Current original-Rouge artefact entry effects.
//!
//! All state IDs below were compared individually with current `skill_buff`
//! and the saved 3.9 Wiki. No name-based legacy `rouge_special_trigger`
//! mapping is used. Random choices are a stable local policy keyed by the
//! frozen battle seed, item instance and recipient, so replay cannot reroll.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::engine::{
    entity::attr::AttrId,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffGrant},
        card::{CardCommand, CardRefillOne},
        eureka::{EUREKA_RESOURCE_ID, EurekaChange, EurekaCommand},
        ex_point::{ExPointChange, ExPointCommand},
        shield::{ShieldCarrierUid, ShieldCommand, ShieldScope},
    },
    skill::{
        rule::output::{BattleCommand, RuleOp},
        target::TargetPool,
    },
};
use sonettobuf::{CardInfo, card_info::CardType, effect_type_enum::EffectType};

use super::ORIGIN;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EntryContext {
    /// Entry balance, already including the map's own EnterBattle rewards.
    pub coin: i32,
    pub seed: u64,
    pub medication_cards: Vec<MedicationCard>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MedicationCard {
    /// Artefact instance UID, never the scene or hero UID.
    pub source_uid: i64,
    pub item_id: i32,
    pub skill_id: i32,
    /// Base one plus matching inlay and in-range Medication contributions.
    pub count: i32,
}

pub fn medication_skill_id(item_id: i32) -> Option<i32> {
    matches!(
        item_id,
        8150006
            | 8150008
            | 8150010
            | 8150012
            | 8150014
            | 8150016
            | 8250007
            | 8250009
            | 8250011
            | 8250013
            | 8250015
            | 8250017
    )
    .then_some(item_id * 100 + 11)
}

/// Effects whose primary description is implemented by this entry module.
/// Used by the server's explicit layout display adapter, not prose matching.
pub fn has_entry_effect(item_id: i32) -> bool {
    matches!(
        item_id,
        8140201
            | 8240202
            | 8140401
            | 8240402
            | 8140501
            | 8240502
            | 8140701
            | 8240702
            | 8140801
            | 8240802
            | 8220318
            | 8320320
            | 8230204
            | 8230042
            | 8320025
            | 8230019
            | 8330607
            | 8120610
    )
}

/// Explicit roots for the current skill catalog. The caller must load these
/// before the entry cards are offered; merely displaying a card is not enough.
pub fn medication_skill_ids(entry: &EntryContext) -> Vec<i32> {
    entry
        .medication_cards
        .iter()
        .filter(|card| card.count > 0 && medication_skill_id(card.item_id) == Some(card.skill_id))
        .map(|card| card.skill_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn grant(source_uid: i64, target_uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid,
        target_uid,
        buff_id,
        amount: Some(amount),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn random_index(seed: u64, item_uid: i64, actor_uid: i64, draw: usize, len: usize) -> usize {
    let key = seed
        ^ (item_uid as u64).rotate_left(13)
        ^ (actor_uid as u64).rotate_left(37)
        ^ (draw as u64).rotate_left(51);
    (mix(key) % len as u64) as usize
}

/// Mirror's total shield is capped at 50% of each recipient's own Max HP.
/// A nonpositive divisor is clamped to one coin locally; it is not an infinite
/// shield and does not divide by zero for extreme inserted-Disk combinations.
pub fn coin_shield_rate(coin: i32, count2: i32) -> i32 {
    let divisor = 50_i32.saturating_sub(count2).max(1);
    100_i32
        .saturating_add((coin.max(0) / divisor).saturating_mul(10))
        .min(500)
}

/// Buff/resource opening only. The parent's once-only entry guard owns this
/// call. Medication must be added separately after the initial hand is dealt.
pub(crate) fn opening(
    managers: &BattleManagers,
    pool: &TargetPool,
    entry: &EntryContext,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    let allies: Vec<i64> = pool
        .attacker_main
        .iter()
        .filter(|entity| managers.hp.current(entity.uid) > 0)
        .map(|entity| entity.uid)
        .collect();
    if allies.is_empty() {
        return vec![];
    }
    let enemies: Vec<i64> = pool
        .defender_main
        .iter()
        .filter(|entity| managers.hp.current(entity.uid) > 0)
        .map(|entity| entity.uid)
        .collect();
    let mut items = context.artefacts.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| item.uid);
    let mut seen = BTreeSet::new();
    // Six literal effects in 永久增益集合. Weal is 90281, not Gash 90061.
    const PERMANENT_BUFFS: [i32; 6] = [90001, 90011, 90041, 90021, 90031, 90281];
    let mut owned: BTreeMap<i64, BTreeSet<i32>> = allies
        .iter()
        .map(|uid| {
            (
                *uid,
                PERMANENT_BUFFS
                    .into_iter()
                    .filter(|id| managers.buff.has_active_buff_id(*uid, *id))
                    .collect(),
            )
        })
        .collect();
    let mut shield_rates: BTreeMap<i64, i32> = BTreeMap::new();
    let mut ops = Vec::new();
    for item in items {
        if !seen.insert(item.uid) {
            continue;
        }
        let fixed = match item.item_id {
            8140401 => Some((90001, 1, &allies)),
            8240402 => Some((90001, 2, &allies)),
            8140701 => Some((90051, 2, &allies)),
            8240702 => Some((90051, 4, &allies)),
            8140501 => Some((90101, 4, &enemies)),
            8240502 => Some((90101, 8, &enemies)),
            8230042 => Some((90201, 1, &allies)),
            _ => None,
        };
        if let Some((id, count, targets)) = fixed {
            for &uid in targets {
                ops.push(grant(uid, uid, id, count));
                if let Some(owned) = owned.get_mut(&uid) {
                    owned.insert(id);
                }
            }
            continue;
        }
        match item.item_id {
            8140801 | 8240802 => {
                let uid = allies[random_index(entry.seed, item.uid, 0, 0, allies.len())];
                ops.push(grant(
                    uid,
                    uid,
                    90071,
                    if item.item_id == 8140801 { 5 } else { 10 },
                ));
            }
            8140201 | 8240202 | 8230204 => {
                let delta = if item.item_id == 8240202 { 2 } else { 1 };
                let targets = if item.item_id == 8230204 {
                    allies.clone()
                } else {
                    vec![allies[random_index(entry.seed, item.uid, 0, 0, allies.len())]]
                };
                for uid in targets {
                    ops.push(RuleOp::Command(BattleCommand::ExPoint(
                        ExPointCommand::Change(ExPointChange {
                            origin: ORIGIN,
                            source_uid: uid,
                            target_uid: uid,
                            delta,
                            config_effect: 0,
                            effect_type: EffectType::Expointchange as i32,
                        }),
                    )));
                }
            }
            8220318 | 8320320 => {
                let delta = if item.item_id == 8220318 { 2 } else { 4 };
                for &uid in &allies {
                    ops.push(RuleOp::Command(BattleCommand::Eureka(
                        EurekaCommand::Change(EurekaChange {
                            origin: ORIGIN,
                            source_uid: uid,
                            target_uid: uid,
                            power_id: EUREKA_RESOURCE_ID,
                            delta,
                            effect_type: EffectType::Powerchange as i32,
                        }),
                    )));
                }
            }
            8320025 => {
                for &uid in &allies {
                    ops.push(grant(uid, uid, 90171, 2));
                    ops.push(grant(uid, uid, 90031, 2));
                    owned.get_mut(&uid).unwrap().insert(90031);
                }
            }
            8230019 | 8330607 => {
                let count = if item.item_id == 8230019 { 2 } else { 5 };
                for &uid in &allies {
                    let known = owned.get_mut(&uid).unwrap();
                    let mut remaining = PERMANENT_BUFFS.to_vec();
                    for draw in 0..count {
                        let preferred: Vec<i32> = remaining
                            .iter()
                            .copied()
                            .filter(|id| !known.contains(id))
                            .collect();
                        let candidates = if preferred.is_empty() {
                            &remaining
                        } else {
                            &preferred
                        };
                        let id = candidates
                            [random_index(entry.seed, item.uid, uid, draw, candidates.len())];
                        remaining.retain(|other| *other != id);
                        known.insert(id);
                        ops.push(grant(uid, uid, id, 1));
                    }
                }
            }
            8120610 => {
                let rate = coin_shield_rate(entry.coin, item.count2);
                for &uid in &allies {
                    let max_hp = i64::from(managers.hp.get(uid).max.max(1));
                    let old_rate =
                        (i64::from(managers.hp.shield(uid)) * 1000 + max_hp - 1) / max_hp;
                    let cumulative = shield_rates.entry(uid).or_default();
                    *cumulative = cumulative.saturating_add(rate);
                    let max_rate = old_rate
                        .saturating_add(i64::from(*cumulative))
                        .min(i64::from(i32::MAX)) as i32;
                    // Exact current Mirror carrier, duration 2. Its native
                    // feature 782 is not registered in the generic engine;
                    // ShieldCommand supplies the verified numeric amount.
                    ops.push(RuleOp::Command(BattleCommand::Shield(ShieldCommand {
                        origin: ORIGIN,
                        source_uid: uid,
                        target_uid: uid,
                        buff_id: 812061001,
                        amount_attr: AttrId::Hp,
                        amount_rate: rate,
                        multiplier_bonus: None,
                        max_attr: AttrId::Hp,
                        max_rate,
                        scope: ShieldScope::Entity,
                        carrier_uid: ShieldCarrierUid::Definition,
                    })));
                }
            }
            _ => {}
        }
    }
    ops
}

/// The New Art Box buys its unstackable protection for the living ally with
/// the lowest current HP ratio. Insufficient coins or an existing copy of the
/// protection leave the balance unchanged; ties use the field order. These
/// two edge cases are a local policy until a matching capture is available.
pub(crate) fn paid_opening(managers: &mut BattleManagers, pool: &TargetPool) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    if context.catalyst.is_none() {
        return vec![];
    }
    let mut boxes = context
        .artefacts
        .iter()
        .filter(|item| item.item_id == 8220613)
        .cloned()
        .collect::<Vec<_>>();
    boxes.sort_by_key(|item| item.uid);
    let target = pool
        .attacker_main
        .iter()
        .filter(|entity| managers.hp.current(entity.uid) > 0)
        .min_by(|left, right| {
            let left_hp = managers.hp.get(left.uid);
            let right_hp = managers.hp.get(right.uid);
            (i64::from(left_hp.current) * i64::from(right_hp.max.max(1)))
                .cmp(&(i64::from(right_hp.current) * i64::from(left_hp.max.max(1))))
        })
        .map(|entity| entity.uid);
    let Some(target) = target else { return vec![] };
    if managers.buff.has_active_buff_id(target, 822061301) {
        return vec![];
    }
    let mut ops = Vec::new();
    for item in boxes {
        let cost = 50_i32.saturating_sub(item.count2).max(0);
        let Some(payment) = super::catalyst::try_change_resources(managers, -cost, 0) else {
            continue;
        };
        ops.extend(payment);
        ops.push(grant(target, target, 822061301, 1));
        // Goldfinch's Blessing does not stack on the same chosen target.
        break;
    }
    ops
}

/// Called once AFTER opening draw/refill. RefillOne preserves the real scene
/// owner and RougeSp type without consuming a normal deck draw or resetting
/// the dealt hand. These are persistent battle cards, not zero-AP temporary
/// hero cards. Client FightCardDataHelper charges RougeSp one action point.
pub(crate) fn opening_cards(entry: &EntryContext) -> Vec<RuleOp> {
    let mut sources = BTreeSet::new();
    let mut cards = entry.medication_cards.iter().collect::<Vec<_>>();
    cards.sort_by_key(|card| card.source_uid);
    let mut ops = Vec::new();
    for card in cards {
        if !sources.insert(card.source_uid)
            || medication_skill_id(card.item_id) != Some(card.skill_id)
        {
            continue;
        }
        for _ in 0..card.count.max(0) {
            ops.push(RuleOp::Command(BattleCommand::Card(
                CardCommand::RefillOne(CardRefillOne {
                    origin: ORIGIN,
                    card: CardInfo {
                        uid: Some(crate::engine::fight::rules::ATTACKER_SIDE_UID),
                        skill_id: Some(card.skill_id),
                        temp_card: Some(false),
                        card_type: Some(CardType::RougeSp as i32),
                        hero_id: Some(0),
                        status: Some(0),
                        energy: Some(0),
                        target_uid: Some(0),
                        ..Default::default()
                    },
                    consume_draw_pile: false,
                    consume_deck: false,
                }),
            )));
        }
    }
    ops
}

#[cfg(test)]
mod tests;
