use super::*;
use crate::{
    catalog::BattleCatalog,
    engine::runtime::{BattleRuntime, determinism::RoundDeterminism},
};
use sonettobuf::{
    BeginRoundOper, BeginRoundRequest, BuffInfo, Fight, FightEntityInfo, FightRound, FightStep,
    FightTeam, HeroAttribute,
};

fn fight(buff: Option<i32>) -> Fight {
    crate::test_support::init_config();
    let tables = crate::test_support::game_data();
    let allies = [3003, 3005, 3006, 3007]
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
            let (group1, group2, ex_skill) = crate::hero::active_skills(tables, model, 3, 1);
            FightEntityInfo {
                uid: Some(index as i64 + 10),
                model_id: Some(model),
                level: Some(1),
                team_type: Some(1),
                current_hp: Some(10000),
                attr: Some(HeroAttribute {
                    hp: Some(10000),
                    attack: Some(1000),
                    ..Default::default()
                }),
                skill_group1: group1,
                skill_group2: group2,
                ex_skill: Some(ex_skill),
                buffs: if index == 0 {
                    buff.into_iter()
                        .map(|id| BuffInfo {
                            uid: Some(900),
                            buff_id: Some(id),
                            from_uid: Some(10),
                            layer: Some(1),
                            duration: Some(1),
                            ..Default::default()
                        })
                        .collect()
                } else {
                    vec![]
                },
                ..Default::default()
            }
        })
        .collect();
    let enemy = |uid, hp| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(2),
        level: Some(1),
        current_hp: Some(hp),
        attr: Some(HeroAttribute {
            hp: Some(hp),
            attack: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: allies,
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy(-1, 100000), enemy(-2, 100000)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        is_record: Some(false),
        ..Default::default()
    }
}

fn context(items: &[i32], mapping: bool) -> RougeContext {
    let tables = crate::test_support::game_data();
    RougeContext {
        artefacts: items
            .iter()
            .enumerate()
            .map(|(i, id)| Artefact {
                uid: i as i64 + 100,
                item_id: *id,
                ..Default::default()
            })
            .collect(),
        support_skills: if mapping {
            vec![SupportSkill {
                owner_uid: 10,
                hero_id: 3004,
                skill_ids: crate::hero::active_skills(tables, 3004, 3, 1).0,
            }]
        } else {
            vec![]
        },
        ..Default::default()
    }
}

fn start(fight: Fight, context: Option<RougeContext>, seed: u64) -> (BattleRuntime, FightRound) {
    let mut runtime =
        BattleRuntime::new(BattleCatalog::new(crate::test_support::game_data()), fight);
    if let Some(context) = context {
        runtime.configure_rouge(context);
    }
    let opening = runtime
        .start_round_with_determinism(RoundDeterminism::with_seed(seed))
        .unwrap();
    (runtime, opening)
}

fn play(index: usize, target: i64) -> BeginRoundRequest {
    BeginRoundRequest {
        opers: vec![BeginRoundOper {
            oper_type: Some(2),
            param1: Some(index as i32 + 1),
            to_id: Some(target),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn has_support_step(steps: &[FightStep], hero: i32, skill: i32) -> bool {
    steps.iter().any(|step| {
        step.support_hero_id == Some(hero) && step.act_id == Some(skill)
            || step.act_effect.iter().any(|effect| {
                effect.fight_step.as_ref().is_some_and(|nested| {
                    has_support_step(std::slice::from_ref(nested), hero, skill)
                })
            })
    })
}

fn has_shield(steps: &[FightStep], target: i64, minimum: i32) -> bool {
    steps
        .iter()
        .flat_map(|step| &step.act_effect)
        .any(|effect| {
            effect.target_id == Some(target)
                && effect.effect_type
                    == Some(sonettobuf::effect_type_enum::EffectType::Shield as i32)
                && effect.effect_num.is_some_and(|amount| amount >= minimum)
                || effect
                    .fight_step
                    .as_ref()
                    .is_some_and(|step| has_shield(std::slice::from_ref(step), target, minimum))
        })
}

#[test]
fn support_anticipation_generates_a_real_foreign_card_that_casts_and_replays() {
    let mut initial = fight(Some(90331));
    initial.defender.as_mut().unwrap().entitys[0]
        .attr
        .as_mut()
        .unwrap()
        .mdefense = Some(100000);
    let frozen = context(&[8320109], true);
    let (mut first, opening) = start(initial.clone(), Some(frozen.clone()), 42);
    let cards = first.card_hand().to_vec();
    let (index, support) = cards
        .iter()
        .enumerate()
        .find(|(_, card)| card.card_type == Some(2))
        .unwrap();
    assert_eq!(
        (
            support.uid,
            support.hero_id,
            support.skill_id,
            support.temp_card
        ),
        (Some(10), Some(3004), Some(30040111), Some(true))
    );
    let request = play(index, -1);
    let round = first.advance_round(request.clone()).unwrap();
    // Druvis is Mental; Lilya's configured support incantation is Reality.
    // The very large Mental DEF must not suppress this foreign skill.
    assert!(100000 - first.entity_info(-1).unwrap().current_hp.unwrap() >= 1000);
    assert!(has_support_step(&round.fight_step, 3004, 30040111));
    assert!(has_shield(&round.fight_step, 10, 1500));
    assert!(
        !first
            .entity_info(10)
            .unwrap()
            .buffs
            .iter()
            .any(|buff| buff.buff_id == Some(90331))
    );
    let restored: RougeContext =
        serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    let (mut replay, replay_opening) = start(initial, Some(restored), 42);
    assert_eq!(opening, replay_opening);
    assert_eq!(cards, replay.card_hand());
    assert_eq!(round, replay.advance_round(request).unwrap());
    assert_eq!(first.reconnect_state(), replay.reconnect_state());
}

#[test]
fn missing_support_mapping_consumes_one_shot_without_fabricating_a_card_or_blocking() {
    let initial = fight(Some(90331));
    let (mut runtime, _) = start(initial, Some(context(&[], false)), 42);
    assert!(
        !runtime
            .card_hand()
            .iter()
            .any(|card| matches!(card.card_type, Some(2 | 3)))
    );
    assert!(
        !runtime
            .entity_info(10)
            .unwrap()
            .buffs
            .iter()
            .any(|buff| buff.buff_id == Some(90331))
    );
    runtime.advance_round(BeginRoundRequest::default()).unwrap();
}

#[test]
fn random_group_precast_uses_seeded_own_skills_and_can_be_played() {
    let initial = fight(Some(90082));
    let owner = &initial.attacker.as_ref().unwrap().entitys[0];
    let expected = owner
        .skill_group1
        .iter()
        .chain(&owner.skill_group2)
        .copied()
        .filter(|id| BattleCatalog::new(crate::test_support::game_data()).skill_rank(*id) == 1)
        .collect::<Vec<_>>();
    let (mut first, opening) = start(initial.clone(), Some(context(&[], false)), 77);
    let hand = first.card_hand().to_vec();
    let (index, card) = hand
        .iter()
        .enumerate()
        .find(|(_, card)| card.uid == Some(10) && card.temp_card == Some(true))
        .unwrap();
    assert!(card.skill_id.is_some_and(|id| expected.contains(&id)));
    assert_ne!(card.card_type, Some(2));
    let request = play(index, -1);
    let round = first.advance_round(request.clone()).unwrap();
    let (mut replay, replay_opening) = start(initial, Some(context(&[], false)), 77);
    assert_eq!(opening, replay_opening);
    assert_eq!(hand, replay.card_hand());
    assert_eq!(round, replay.advance_round(request).unwrap());
    assert!(
        !first
            .entity_info(10)
            .unwrap()
            .buffs
            .iter()
            .any(|buff| buff.buff_id == Some(90082))
    );
}

#[test]
fn compact_limits_change_actual_opening_draws_and_replay_without_leaking_to_other_fights() {
    let initial = fight(None);
    let (plain, _) = start(initial.clone(), None, 42);
    let (one, _) = start(initial.clone(), Some(context(&[8230305], false)), 42);
    let (two, _) = start(initial.clone(), Some(context(&[8330306], false)), 42);
    let (both, opening) = start(
        initial.clone(),
        Some(context(&[8230305, 8330306], false)),
        42,
    );
    let count = |runtime: &BattleRuntime| {
        runtime
            .card_hand()
            .iter()
            .filter(|card| !card.temp_card.unwrap_or(false))
            .count()
    };
    assert_eq!(count(&one), count(&plain) + 1);
    assert_eq!(count(&two), count(&plain) + 2);
    assert_eq!(count(&both), count(&plain) + 3);
    let (replay, replay_opening) = start(initial, Some(context(&[8230305, 8330306], false)), 42);
    assert_eq!(opening, replay_opening);
    assert_eq!(both.card_hand(), replay.card_hand());
}

#[test]
fn serum_kill_creates_next_round_support_and_precast_then_real_support_cast_replays() {
    let mut initial = fight(None);
    let weak = &mut initial.defender.as_mut().unwrap().entitys[0];
    weak.current_hp = Some(1);
    weak.attr.as_mut().unwrap().hp = Some(1);
    let frozen = context(&[8210045, 8320109], true);
    let (mut runtime, opening) = start(initial.clone(), Some(frozen.clone()), 42);
    let host_card = runtime
        .card_hand()
        .iter()
        .position(|card| {
            card.uid == Some(10)
                && !card.temp_card.unwrap_or(false)
                && card.skill_id.is_some_and(|id| {
                    BattleCatalog::new(crate::test_support::game_data()).skill_is_attack(id)
                })
        })
        .unwrap();
    let first_request = play(host_card, -1);
    let first_round = runtime.advance_round(first_request.clone()).unwrap();
    assert_eq!(runtime.entity_info(-1).unwrap().current_hp, Some(0));
    let hand = runtime.card_hand().to_vec();
    let support = hand
        .iter()
        .position(|card| card.uid == Some(10) && card.card_type == Some(2))
        .unwrap();
    assert!(hand.iter().any(|card| card.uid == Some(10)
        && card.temp_card == Some(true)
        && card.card_type != Some(2)));
    let second_request = play(support, -2);
    let second_round = runtime.advance_round(second_request.clone()).unwrap();
    assert!(has_support_step(&second_round.fight_step, 3004, 30040111));
    assert!(has_shield(&second_round.fight_step, 10, 1500));
    let saved = serde_json::to_string(&(initial, frozen)).unwrap();
    let (restored_fight, restored_context): (Fight, RougeContext) =
        serde_json::from_str(&saved).unwrap();
    let (mut replay, replay_opening) = start(restored_fight, Some(restored_context), 42);
    assert_eq!(opening, replay_opening);
    assert_eq!(first_round, replay.advance_round(first_request).unwrap());
    assert_eq!(hand, replay.card_hand());
    assert_eq!(second_round, replay.advance_round(second_request).unwrap());
    assert_eq!(runtime.reconnect_state(), replay.reconnect_state());
}

#[test]
fn stacked_random_anticipation_generates_one_real_precast_per_stack() {
    let mut initial = fight(Some(90082));
    initial.attacker.as_mut().unwrap().entitys[0].buffs[0].layer = Some(3);
    let owner = &initial.attacker.as_ref().unwrap().entitys[0];
    let expected = owner
        .skill_group1
        .iter()
        .chain(&owner.skill_group2)
        .copied()
        .filter(|id| BattleCatalog::new(crate::test_support::game_data()).skill_rank(*id) == 1)
        .collect::<Vec<_>>();
    let (runtime, _) = start(initial, Some(context(&[], false)), 42);
    let cards = runtime
        .card_hand()
        .iter()
        .filter(|card| card.uid == Some(10) && card.temp_card == Some(true))
        .collect::<Vec<_>>();
    assert_eq!(cards.len(), 3);
    assert!(
        cards
            .iter()
            .all(|card| card.skill_id.is_some_and(|id| expected.contains(&id))),
        "actual generated cards: {:?}",
        cards
            .iter()
            .map(|card| (card.skill_id, card.card_type))
            .collect::<Vec<_>>()
    );
}
