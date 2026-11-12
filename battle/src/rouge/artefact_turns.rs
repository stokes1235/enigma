//! Verified original-Rouge turn and completed-action artefacts.
//!
//! Values are taken from the saved 3.9 Collection of Artefacts Wiki and the
//! current rouge_desc, rather than the older same-name 9xxxxxx skills. Buff
//! definitions 90001/90011/90091/90301 encode the actual status, stack cap,
//! attributes and consumption rules in the shipped game data.
use super::*;
use crate::engine::{
    manager::{
        buff::{BuffConsume, BuffSelector, DepletedBuff},
        ex_point::{ExPointChange, ExPointCommand},
    },
    runtime::determinism::RoundDeterminism,
    skill::action::ActionEvent,
};
use sonettobuf::effect_type_enum::EffectType;

#[derive(Debug, Clone, Default)]
pub(super) struct TurnLedger {
    last_start_round: Option<i32>,
    last_end_round: Option<i32>,
}

fn give(uid: i64, buff_id: i32, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid: uid,
        target_uid: uid,
        buff_id,
        amount: Some(amount),
        occurrences: 1,
        child_uid_reservations: 0,
    })))
}

fn live_main(managers: &BattleManagers, pool: &TargetPool) -> Vec<i64> {
    pool.attacker_main
        .iter()
        .filter_map(|entity| (managers.hp.current(entity.uid) > 0).then_some(entity.uid))
        .collect()
}

/// A slingshot grants Lucky before the selected rank-2/3 card resolves, so
/// its crit rate applies to this same cast rather than only the next one.
pub(super) fn before_card(
    managers: &BattleManagers,
    pool: &TargetPool,
    source_uid: i64,
    skill_id: i32,
    card_index: i32,
    is_ultimate: bool,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    if card_index <= 0
        || is_ultimate
        || !matches!(managers.catalog().skill_rank(skill_id), 2 | 3)
        || !pool
            .attacker_main
            .iter()
            .any(|entity| entity.uid == source_uid)
        || managers.hp.current(source_uid) <= 0
    {
        return vec![];
    }
    context
        .artefacts
        .iter()
        .filter_map(|item| {
            let amount = match item.item_id {
                8210314 => 2,
                8310315 => 4,
                _ => return None,
            };
            Some(give(source_uid, 90011, amount))
        })
        .collect()
}

/// Called after the opening/next-round hand has been dealt. Take one snapshot
/// of the hand and existing buffs before generating any round-start rewards.
/// Count real incantation cards, including Support and special incantations,
/// but exclude Ultimate cards. This counting/timing is a local policy until a
/// trace containing the relevant artefacts establishes exact ordering.
pub(super) fn round_start(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
    determinism: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    if round <= 0
        || state
            .turn_effects
            .last_start_round
            .is_some_and(|seen| seen >= round)
    {
        return vec![];
    }
    state.turn_effects.last_start_round = Some(round);
    let items = state.context.artefacts.clone();
    let card_count = managers
        .card
        .hand()
        .iter()
        .filter(|card| {
            !matches!(card.card_type, Some(1 | 3))
                && card
                    .skill_id
                    .is_some_and(|id| id > 0 && !managers.catalog().skill_is_big(id))
        })
        .count()
        .min(i32::MAX as usize) as i32;
    let mut ops = Vec::new();
    for item in items {
        if item.item_id == 8120821 && round % 2 == 0 {
            for enemy in &pool.defender_main {
                if managers.hp.current(enemy.uid) > 0 {
                    ops.push(RuleOp::Command(BattleCommand::Buff(
                        BuffCommand::ConsumeCoalesced(BuffConsume {
                            origin: ORIGIN,
                            target_uid: enemy.uid,
                            selector: BuffSelector::ExactId(90071),
                            amount: 2,
                            depleted: DepletedBuff::Remove,
                        }),
                    )));
                }
            }
            continue;
        }
        // Random recipients use the battle's replayed RNG and are restricted
        // to living characters actually on the field.
        if matches!(item.item_id, 8120034 | 8120104 | 8130203) {
            let allies = live_main(managers, pool);
            if let Some(uid) = determinism
                .lua_random_index(allies.len())
                .map(|index| allies[index])
            {
                match item.item_id {
                    8120034 => ops.push(give(uid, 90011, 1)),
                    8130203 => ops.push(give(uid, 90091, 10)),
                    8120104 => ops.push(RuleOp::Command(BattleCommand::ExPoint(
                        ExPointCommand::Change(ExPointChange {
                            origin: ORIGIN,
                            source_uid: uid,
                            target_uid: uid,
                            delta: 1,
                            config_effect: 0,
                            effect_type: EffectType::Expointchange as i32,
                        }),
                    ))),
                    _ => unreachable!(),
                }
            }
            continue;
        }
        for uid in live_main(managers, pool) {
            match item.item_id {
                8130217 => ops.push(give(uid, 90091, 3)),
                8130041 if managers.buff.buff_id_amount(uid, 90011) >= 4 => {
                    ops.push(give(uid, 90011, 2))
                }
                8230028 => ops.push(give(uid, 90011, 2)),
                8330026 => ops.push(give(uid, 90011, 4)),
                8330710 => ops.push(give(uid, 90051, 5)),
                8130803 => ops.push(give(uid, 90071, 1)),
                8130804 => ops.push(give(uid, 90071, 2)),
                8120821 if round % 2 == 1 => ops.push(give(uid, 90071, 2)),
                8110307 | 8210308 => {
                    let divisor = if item.item_id == 8110307 { 3 } else { 2 };
                    let stacks = (card_count / divisor)
                        .saturating_mul(2_i32.saturating_add(item.count2.max(0)))
                        .min(100);
                    if stacks > 0 {
                        ops.push(give(uid, 90301, stacks));
                    }
                }
                _ => {}
            }
        }
    }
    ops
}

/// At an allied action's actual completion, never at queue construction or
/// damage preview. Each successful hit's crit count contributes separately.
pub(super) fn action(
    managers: &BattleManagers,
    pool: &TargetPool,
    action: &ActionEvent,
    aid: bool,
    determinism: &mut RoundDeterminism,
) -> Vec<RuleOp> {
    let Some(context) = managers.rouge.context() else {
        return vec![];
    };
    if !pool
        .attacker_main
        .iter()
        .any(|entity| entity.uid == action.source_uid)
        || managers.hp.current(action.source_uid) <= 0
    {
        return vec![];
    }
    let high_rank = action.card_index > 0
        && matches!(action.rank, 2 | 3)
        && !managers.catalog().skill_is_big(action.skill_id);
    let mut ops = Vec::new();
    for item in &context.artefacts {
        match item.item_id {
            8110213 => ops.push(give(action.source_uid, 90091, 5)),
            8130303 if high_rank => ops.push(give(action.source_uid, 90301, 4)),
            8230304 if high_rank => ops.push(give(action.source_uid, 90301, 6)),
            8130103
                if aid
                    && action.card_index > 0
                    && !managers.catalog().skill_is_big(action.skill_id) =>
            {
                for uid in live_main(managers, pool) {
                    ops.push(give(uid, 90091, 2));
                }
            }
            8310309
                if high_rank
                    && determinism.roll_permille(
                        500_i32
                            .saturating_add(item.count2.saturating_mul(10))
                            .clamp(0, 1000),
                    ) =>
            {
                for uid in live_main(managers, pool) {
                    ops.push(give(uid, 90301, 4));
                }
            }
            8210314 | 8310315 if action.crit_count > 0 => {
                let per_crit = if item.item_id == 8210314 {
                    1_i32
                } else {
                    2_i32
                };
                ops.push(give(
                    action.source_uid,
                    90011,
                    action.crit_count.saturating_mul(per_crit).min(50),
                ));
            }
            _ => {}
        }
    }
    ops
}

/// Apply the boxing set after the attacker-side round settlement. Multiple
/// copies add normally; the same round cannot be committed twice.
pub(super) fn round_end(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    round: i32,
) -> Vec<RuleOp> {
    let Some(state) = managers.rouge.active.as_mut() else {
        return vec![];
    };
    if round <= 0
        || state
            .turn_effects
            .last_end_round
            .is_some_and(|seen| seen >= round)
    {
        return vec![];
    }
    state.turn_effects.last_end_round = Some(round);
    let items = state.context.artefacts.clone();
    let mut ops = Vec::new();
    for item in items {
        let stacks = match item.item_id {
            8230027 => 1,
            8330026 => 2,
            _ => 0,
        };
        if stacks > 0 {
            for uid in live_main(managers, pool) {
                ops.push(give(uid, 90001, stacks));
            }
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        manager::card::CardManager,
        runtime::{determinism::RoundDeterminism, drain},
        skill::{effect::SkillEffectCatalog, target::TargetContext},
    };
    use sonettobuf::{CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

    fn setup(items: &[(i32, i32)]) -> (BattleManagers, TargetPool) {
        crate::test_support::init_config();
        let entity = |uid, team| FightEntityInfo {
            uid: Some(uid),
            team_type: Some(team),
            current_hp: Some(1000),
            attr: Some(HeroAttribute {
                hp: Some(1000),
                attack: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![entity(10, 1), entity(11, 1)],
                sub_entitys: vec![entity(12, 1)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![entity(-1, 2)],
                ..Default::default()
            }),
            version: Some(7),
            ..Default::default()
        };
        let mut managers = BattleManagers::seeded(&fight);
        configure(
            &mut managers,
            RougeContext {
                artefacts: items
                    .iter()
                    .enumerate()
                    .map(|(index, (item_id, count2))| Artefact {
                        uid: index as i64 + 100,
                        item_id: *item_id,
                        count2: *count2,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
        (managers, TargetPool::from_fight(&fight))
    }

    fn execute(managers: &mut BattleManagers, pool: &TargetPool, ops: Vec<RuleOp>) {
        let mut rng = RoundDeterminism::with_seed(42);
        drain::run_command_group(
            managers,
            pool,
            &SkillEffectCatalog::default(),
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            ops,
        )
        .unwrap();
    }

    #[test]
    fn round_effects_use_real_buffs_hand_and_alive_active_allies_once_per_round() {
        let (mut managers, pool) = setup(&[
            (8110307, 0),
            (8210308, 1),
            (8130041, 0),
            (8230028, 0),
            (8230027, 0),
            (8330026, 0),
        ]);
        let cards = (0..5)
            .map(|_| CardInfo {
                uid: Some(10),
                skill_id: Some(30030111),
                ..Default::default()
            })
            .chain(std::iter::once(CardInfo {
                uid: Some(10),
                skill_id: Some(30030131),
                card_type: Some(1),
                ..Default::default()
            }))
            .collect();
        managers.card = CardManager::new(cards);
        execute(&mut managers, &pool, vec![give(10, 90011, 4)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 12);
        assert_eq!(managers.buff.buff_id_amount(11, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 8);
        assert_eq!(
            managers.buff.buff_id_amount(12, 90011),
            0,
            "reserve heroes are not on the field"
        );
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
        let ops = round_end(&mut managers, &pool, 1);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90001), 3);
        assert_eq!(managers.buff.buff_id_amount(11, 90001), 3);
        assert!(round_end(&mut managers, &pool, 1).is_empty());
    }

    #[test]
    fn actual_action_results_award_only_the_actor_and_stack_by_rank_and_crits() {
        let (mut managers, pool) = setup(&[
            (8110213, 0),
            (8130303, 0),
            (8230304, 0),
            (8210314, 0),
            (8310315, 0),
        ]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            crit_count: 2,
            is_attack: true,
            ..Default::default()
        };
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = action(&managers, &pool, &event, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90091), 5);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 10);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(11, 90091), 0);
        let weak = ActionEvent {
            rank: 1,
            crit_count: 0,
            ..event
        };
        let ops = action(&managers, &pool, &weak, false, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90091), 10);
        assert_eq!(managers.buff.buff_id_amount(10, 90301), 10);
    }

    #[test]
    fn support_scarf_and_forging_hammer_use_actual_casts_and_replayed_probability() {
        let (mut managers, pool) = setup(&[(8130103, 0), (8310309, 0)]);
        let event = ActionEvent {
            source_uid: 10,
            skill_id: 30030112,
            card_index: 1,
            rank: 2,
            ..Default::default()
        };
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 999]);
        let ops = action(&managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90091), 2);
            assert_eq!(managers.buff.buff_id_amount(uid, 90301), 4);
        }
        let ops = action(&managers, &pool, &event, true, &mut rng);
        execute(&mut managers, &pool, ops);
        for uid in [10, 11] {
            assert_eq!(managers.buff.buff_id_amount(uid, 90091), 4);
            assert_eq!(managers.buff.buff_id_amount(uid, 90301), 4);
        }
        let no_card = ActionEvent {
            card_index: 0,
            ..event
        };
        assert!(action(&managers, &pool, &no_card, true, &mut rng).is_empty());
    }

    #[test]
    fn slingshot_lucky_is_applied_before_only_the_selected_high_rank_card() {
        let (mut managers, pool) = setup(&[(8210314, 0), (8310315, 0)]);
        assert!(before_card(&managers, &pool, 10, 30030112, 0, false).is_empty());
        assert!(before_card(&managers, &pool, 10, 30030111, 1, false).is_empty());
        assert!(before_card(&managers, &pool, 10, 30030112, 1, true).is_empty());
        let ops = before_card(&managers, &pool, 10, 30030112, 1, false);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90011), 6);
        assert_eq!(managers.buff.buff_id_amount(11, 90011), 0);
    }

    #[test]
    fn clockwork_and_random_round_gifts_stay_on_living_main_characters() {
        let (mut managers, pool) = setup(&[
            (8120034, 0),
            (8120104, 0),
            (8130203, 0),
            (8130803, 0),
            (8130804, 0),
        ]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 3);
        assert_eq!(managers.buff.buff_id_amount(11, 90071), 3);
        assert_eq!(managers.buff.buff_id_amount(12, 90071), 0);
        assert_eq!(
            managers.buff.buff_id_amount(10, 90011) + managers.buff.buff_id_amount(11, 90011),
            1
        );
        assert_eq!(
            managers.buff.buff_id_amount(10, 90091) + managers.buff.buff_id_amount(11, 90091),
            10
        );
        assert_eq!(managers.ex_point.get(10) + managers.ex_point.get(11), 1);
        assert!(round_start(&mut managers, &pool, 1, &mut rng).is_empty());
    }

    #[test]
    fn catalyst_hand_limit_stacks_with_actual_placed_powder_boxes() {
        let (mut managers, _) = setup(&[]);
        for (style_id, expected) in [(1, 3), (4, 5), (5, 6), (6, 6)] {
            configure(
                &mut managers,
                RougeContext {
                    catalyst: Some(Catalyst {
                        style_id,
                        ..Default::default()
                    }),
                    artefacts: vec![
                        Artefact {
                            uid: 1,
                            item_id: 8230305,
                            ..Default::default()
                        },
                        Artefact {
                            uid: 2,
                            item_id: 8330306,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            );
            assert_eq!(hand_limit_bonus(&managers), expected);
        }
    }

    #[test]
    fn cardboard_hat_grants_odd_round_dynamo_and_consumes_only_enemy_dynamo_on_even_rounds() {
        let (mut managers, pool) = setup(&[(8120821, 0), (8120821, 0)]);
        let mut rng = RoundDeterminism::with_seed(42);
        let ops = round_start(&mut managers, &pool, 1, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 4);
        execute(&mut managers, &pool, vec![give(-1, 90071, 3)]);
        let ops = round_start(&mut managers, &pool, 2, &mut rng);
        execute(&mut managers, &pool, ops);
        assert_eq!(managers.buff.buff_id_amount(-1, 90071), 0);
        assert_eq!(managers.buff.buff_id_amount(10, 90071), 4);
        assert!(round_start(&mut managers, &pool, 2, &mut rng).is_empty());
    }
}
