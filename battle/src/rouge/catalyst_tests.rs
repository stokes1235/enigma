//! Exercise the public catalyst operation and real card queue, including replay.
use super::*;
use crate::{
    catalog::BattleCatalog,
    engine::runtime::{BattleRuntime, determinism::RoundDeterminism},
};
use sonettobuf::{
    BeginRoundOper, BeginRoundRequest, Fight, FightEntityInfo, FightStep, FightTeam, HeroAttribute,
    UseClothSkillReply, UseClothSkillRequest,
};

fn fight(enemy_hp: i32) -> Fight {
    crate::test_support::init_config();
    let tables = crate::test_support::game_data();
    let allies = [3003, 3005, 3006, 3007]
        .into_iter()
        .enumerate()
        .map(|(index, model)| {
            let (group1, group2, ultimate) = crate::hero::active_skills(tables, model, 3, 1);
            FightEntityInfo {
                uid: Some(10 + index as i64),
                model_id: Some(model),
                team_type: Some(1),
                level: Some(1),
                position: Some(index as i32 + 1),
                current_hp: Some(10000),
                ex_point: Some(0),
                attr: Some(HeroAttribute {
                    hp: Some(10000),
                    attack: Some(1000),
                    ..Default::default()
                }),
                skill_group1: group1,
                skill_group2: group2,
                ex_skill: Some(ultimate),
                ..Default::default()
            }
        })
        .collect();
    let enemy = |uid| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(2),
        level: Some(1),
        current_hp: Some(enemy_hp),
        attr: Some(HeroAttribute {
            hp: Some(100000),
            attack: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: allies,
            power: Some(15),
            cloth_id: Some(1),
            player_entity: Some(FightEntityInfo {
                uid: Some(0),
                current_hp: Some(1),
                ..Default::default()
            }),
            ex_team_str: Some("1000#100#100#{\"preserved\":true}".into()),
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy(-1), enemy(-2)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(30),
        is_record: Some(false),
        ..Default::default()
    }
}

fn context(style: i32, coin: i32, power: i32) -> RougeContext {
    let tables = crate::test_support::game_data();
    RougeContext {
        catalyst: Some(Catalyst {
            style_id: style,
            coin,
            power,
            power_limit: 100,
            slots: (10..14)
                .enumerate()
                .map(|(slot, uid)| (slot as i32 + 1, uid))
                .collect(),
            disk_count: 5,
            epic_count: 3,
        }),
        support_skills: (10..14)
            .map(|uid| SupportSkill {
                owner_uid: uid,
                hero_id: 3004,
                skill_ids: crate::hero::active_skills(tables, 3004, 3, 1).0,
            })
            .collect(),
        support_ultimates: (10..14)
            .map(|uid| SupportUltimate {
                owner_uid: uid,
                hero_id: 3004,
                skill_id: crate::hero::active_skills(tables, 3004, 3, 1).2,
            })
            .collect(),
        incantation_rules: Some(IncantationRules::default()),
        ..Default::default()
    }
}

fn start(mut initial: Fight, frozen: RougeContext) -> BattleRuntime {
    let tables = crate::test_support::game_data();
    if let Some(style) = frozen
        .catalyst
        .as_ref()
        .and_then(|catalyst| tables.rouge_style.get(catalyst.style_id))
    {
        initial.attacker.as_mut().unwrap().skill_infos = style
            .active_skills
            .split('#')
            .rev()
            .filter_map(|id| id.parse::<i32>().ok())
            .filter_map(|id| tables.rouge_active_skill.get(id))
            .map(|skill| sonettobuf::PlayerSkillInfo {
                skill_id: Some(skill.id),
                cd: Some(0),
                need_power: Some(skill.power_cost),
                r#type: Some(1),
            })
            .collect();
    }
    let mut runtime = BattleRuntime::new(
        BattleCatalog::new(crate::test_support::game_data()),
        initial,
    );
    runtime.configure_rouge(frozen);
    runtime
        .start_round_with_determinism(RoundDeterminism::with_seed(42))
        .unwrap();
    runtime
}

fn request(skill: i32, target: i64) -> UseClothSkillRequest {
    UseClothSkillRequest {
        skill_id: Some(skill),
        from_id: Some(0),
        to_id: Some(target),
        r#type: Some(2),
    }
}

fn cast(runtime: &mut BattleRuntime, skill: i32, target: i64) -> UseClothSkillReply {
    runtime
        .use_cloth_skill(request(skill, target))
        .unwrap_or_else(|| panic!("catalyst {skill} rejected"))
}

#[test]
fn captured_syntony_catalyst_projects_the_boost_and_both_temporary_basics() {
    let mut runtime = start(fight(100000), context(5, 1000, 100));
    let before = runtime.card_info_push().card_group.len();
    let generated = cast(&mut runtime, 800040001, 10).round.unwrap();
    let added = generated
        .fight_step
        .iter()
        .flat_map(|step| &step.act_effect)
        .filter(|effect| effect.effect_type == Some(197))
        .collect::<Vec<_>>();
    assert_eq!(added.len(), 2);
    for effect in added {
        let card = effect.card_info.as_ref().expect("real generated card");
        assert_eq!(card.uid, Some(10));
        assert_eq!(card.card_type.unwrap_or_default(), 0);
        assert_eq!(card.temp_card, Some(true));
        assert_eq!(card.hero_id, Some(3003));
    }
    assert_eq!(runtime.card_info_push().card_group.len(), before + 2);
    assert_eq!(runtime.rouge_resources().unwrap().power, 95);

    let boosted = cast(&mut runtime, 800040002, 0).round.unwrap();
    let markers = boosted
        .fight_step
        .iter()
        .flat_map(|step| &step.act_effect)
        .filter(|effect| effect.effect_type == Some(177))
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].target_id.unwrap_or_default(), 0);
    assert_eq!(markers[0].effect_num, Some(1));
    assert_eq!(runtime.rouge_resources().unwrap().power, 86);

    let skill = crate::hero::active_skills(crate::test_support::game_data(), 3003, 3, 1).0[0];
    let hand = runtime.card_info_push().card_group;
    let index = hand.iter().position(|card| {
        card.uid == Some(10) && card.skill_id == Some(skill) && card.temp_card == Some(true)
    }).unwrap();
    let request = BeginRoundRequest {
        opers: vec![BeginRoundOper {
            oper_type: Some(2),
            param1: Some(index as i32 + 1),
            to_id: Some(-1),
            ..Default::default()
        }],
        ..Default::default()
    };
    let round = runtime.advance_round(request.clone()).unwrap();
    let action = round.fight_step.iter().position(|step| step.act_id == Some(skill)).unwrap();
    assert!(round.fight_step[..action].iter().rev().any(|step| {
        step.act_effect.iter().any(|effect| effect.effect_type == Some(111)
            && effect.target_id == Some(10) && effect.effect_num == Some(1))
    }), "captured temporary basic grants its owner one Moxie before acting");

    let mut replay = start(fight(100000), context(5, 1000, 100));
    cast(&mut replay, 800040001, 10);
    cast(&mut replay, 800040002, 0);
    assert_eq!(replay.advance_round(request).unwrap(), round);
    assert_eq!(replay.reconnect_state(), runtime.reconnect_state());
}

fn has_step(steps: &[FightStep], source: i64, skill: i32, support: Option<i32>) -> bool {
    steps.iter().any(|step| {
        step.from_id == Some(source)
            && step.act_id == Some(skill)
            && support.is_none_or(|hero| step.support_hero_id == Some(hero))
            || step
                .act_effect
                .iter()
                .filter_map(|effect| effect.fight_step.as_ref())
                .any(|child| has_step(std::slice::from_ref(child), source, skill, support))
    })
}

#[test]
fn all_eighteen_base_actives_execute_through_the_public_operation_and_charge_exact_costs() {
    let initial = fight(100000);
    let tables = crate::test_support::game_data();
    for style in 1..=9 {
        let row = tables
            .rouge_style
            .iter()
            .find(|row| row.id == style && row.version.is_empty())
            .unwrap();
        let ids = row
            .active_skills
            .split('#')
            .map(|value| value.parse::<i32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        for skill in ids {
            let frozen = context(style, 1000, 100);
            let mut runtime = start(initial.clone(), frozen);
            let before = runtime.rouge_resources().unwrap();
            let terms = tables
                .rouge_active_skill
                .iter()
                .find(|row| row.id == skill)
                .unwrap();
            let reply = cast(&mut runtime, skill, 10);
            let after = runtime.rouge_resources().unwrap();
            assert_eq!(after.coin, before.coin - terms.coin_cost, "skill {skill}");
            assert_eq!(
                after.power,
                before.power - terms.power_cost,
                "skill {skill}"
            );
            assert_eq!(
                after.spent_coin,
                before.spent_coin + terms.coin_cost,
                "skill {skill}"
            );
            assert_eq!(
                after.spent_power,
                before.spent_power + terms.power_cost,
                "skill {skill}"
            );
            assert!(
                has_step(&reply.round.unwrap().fight_step, 0, skill, None),
                "skill {skill}"
            );
            let synchronized = runtime.reconnect_state().0.attacker.unwrap();
            assert_eq!(
                synchronized.power,
                Some(15),
                "ordinary tuning power changed for {skill}"
            );
            assert!(
                synchronized
                    .ex_team_str
                    .unwrap()
                    .ends_with("#{\"preserved\":true}")
            );
        }
    }
}

#[test]
fn invalid_targets_missing_mapping_and_insufficient_resources_leave_the_full_battle_unchanged() {
    let initial = fight(100000);
    for (style, skill, target, mapping, coin, power) in [
        (2, 800010001, -1, true, 1000, 100),
        (2, 800010001, 999, true, 1000, 100),
        (2, 800010001, 10, false, 1000, 100),
        (2, 800010001, 10, true, 1000, 4),
        (7, 800060001, 10, true, 99, 0),
        (1, 800010001, 10, true, 1000, 100),
    ] {
        let mut frozen = context(style, coin, power);
        if !mapping {
            frozen.support_skills.clear();
        }
        let mut runtime = start(initial.clone(), frozen);
        let before = runtime.reconnect_state();
        let hand = runtime.card_hand().to_vec();
        let resources = runtime.rouge_resources();
        assert!(runtime.use_cloth_skill(request(skill, target)).is_none());
        assert_eq!(runtime.reconnect_state(), before);
        assert_eq!(runtime.card_hand(), hand);
        assert_eq!(runtime.rouge_resources(), resources);
    }
}

#[test]
fn per_round_limit_is_client_visible_recovers_next_round_and_replays() {
    let initial = fight(100000);
    let frozen = context(7, 1000, 0);
    let mut first = start(initial.clone(), frozen.clone());
    let reply = cast(&mut first, 800060001, -1);
    let spent = first.rouge_resources();
    assert!(first.use_cloth_skill(request(800060001, -1)).is_none());
    assert_eq!(first.rouge_resources(), spent);
    let disabled = first.reconnect_state().0.attacker.unwrap().skill_infos;
    assert!(
        disabled
            .iter()
            .any(|skill| skill.skill_id == Some(800060001) && skill.cd.unwrap_or_default() > 0)
    );
    let next = first.advance_round(BeginRoundRequest::default()).unwrap();
    let enabled = first.reconnect_state().0.attacker.unwrap().skill_infos;
    assert!(
        enabled
            .iter()
            .any(|skill| skill.skill_id == Some(800060001) && skill.cd == Some(0))
    );
    let second = cast(&mut first, 800060001, -1);
    let restored: RougeContext =
        serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    let mut replay = start(initial, restored);
    assert_eq!(reply, cast(&mut replay, 800060001, -1));
    assert_eq!(
        next,
        replay.advance_round(BeginRoundRequest::default()).unwrap()
    );
    assert_eq!(second, cast(&mut replay, 800060001, -1));
    assert_eq!(first.reconnect_state(), replay.reconnect_state());
    assert_eq!(first.rouge_resources(), replay.rouge_resources());
}

#[test]
fn generated_support_ultimate_uses_the_foreign_skill_consumes_moxie_and_replays() {
    let initial = fight(100000);
    let frozen = context(2, 1000, 100);
    let mut first = start(initial.clone(), frozen.clone());
    let generated = cast(&mut first, 800010002, 10);
    let support_id = frozen.support_ultimates[0].skill_id;
    let before_hand = first.card_hand().to_vec();
    let support_index = before_hand
        .iter()
        .position(|card| {
            card.uid == Some(10) && card.card_type == Some(3) && card.skill_id == Some(support_id)
        })
        .unwrap();
    let own_index = before_hand
        .iter()
        .position(|card| {
            card.uid == Some(10)
                && card.card_type != Some(3)
                && card.card_type != Some(6)
                && card.skill_id == initial.attacker.as_ref().unwrap().entitys[0].ex_skill
        })
        .unwrap();
    // The real client sends an explicit -99 operation for the alternative
    // ultimate after removing the selected card from its simulated hand.
    let remaining_own = own_index - usize::from(own_index > support_index);
    let request = BeginRoundRequest {
        opers: vec![
            BeginRoundOper {
                oper_type: Some(2),
                param1: Some(support_index as i32 + 1),
                to_id: Some(-1),
                ..Default::default()
            },
            BeginRoundOper {
                oper_type: Some(-99),
                param1: Some(remaining_own as i32 + 1),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let round = first.advance_round(request.clone()).unwrap();
    assert!(has_step(&round.fight_step, 10, support_id, Some(3004)));
    assert_eq!(first.entity_info(10).unwrap().ex_point, Some(0));
    assert!(
        !first
            .card_hand()
            .iter()
            .any(|card| card.uid == Some(10) && card.card_type == Some(3))
    );
    let mut replay = start(initial, frozen);
    assert_eq!(generated, cast(&mut replay, 800010002, 10));
    assert_eq!(round, replay.advance_round(request).unwrap());
    assert_eq!(first.reconnect_state(), replay.reconnect_state());
}

#[test]
fn mentalism_support_incantation_still_grants_exactly_one_moxie() {
    let mut runtime = start(fight(100000), context(2, 1000, 100));
    cast(&mut runtime, 800010001, 10);
    let index = runtime
        .card_hand()
        .iter()
        .position(|card| {
            card.uid == Some(10) && card.card_type == Some(2) && card.skill_id == Some(30040112)
        })
        .unwrap();
    assert_eq!(runtime.entity_info(10).unwrap().ex_point, Some(0));
    runtime
        .advance_round(BeginRoundRequest {
            opers: vec![BeginRoundOper {
                oper_type: Some(2),
                param1: Some(index as i32 + 1),
                to_id: Some(-1),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(runtime.entity_info(10).unwrap().ex_point, Some(1));
}

#[test]
fn a_generated_rouge_scene_card_spends_one_of_the_auto_planners_real_action_points() {
    let mut runtime = start(fight(100000), context(8, 1000, 100));
    cast(&mut runtime, 800070002, -1);
    let index = runtime
        .card_hand()
        .iter()
        .position(|card| card.skill_id == Some(800070003))
        .unwrap();
    let capacity = runtime.card_info_push().act_point.unwrap();
    assert_eq!(capacity, 4);
    let planned = runtime.plan_auto_round(&sonettobuf::AutoRoundRequest {
        opers: vec![BeginRoundOper {
            oper_type: Some(2),
            param1: Some(index as i32 + 1),
            to_id: Some(-1),
            ..Default::default()
        }],
        to_id: Some(-1),
    });
    assert_eq!(
        planned
            .opers
            .iter()
            .filter(|oper| oper.oper_type == Some(2))
            .count(),
        (capacity - 1) as usize
    );
}

#[test]
fn golden_annihilation_is_a_real_playable_scene_card_and_resolves_its_threshold_kill() {
    let mut runtime = start(fight(20000), context(7, 1000, 0));
    cast(&mut runtime, 800060002, -1);
    let index = runtime
        .card_hand()
        .iter()
        .position(|card| card.skill_id == Some(800060003))
        .unwrap();
    let round = runtime
        .advance_round(BeginRoundRequest {
            opers: vec![BeginRoundOper {
                oper_type: Some(2),
                param1: Some(index as i32 + 1),
                to_id: Some(-1),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(has_step(&round.fight_step, 0, 800060003, None));
    assert_eq!(runtime.outcome(), crate::BattleOutcome::Victory);
    assert_eq!(runtime.rouge_resources().unwrap().spent_coin, 200);
}

#[test]
fn rhapsody_is_playable_and_uses_the_frozen_epic_count_for_current_hp_loss_behavior() {
    let mut runtime = start(fight(100000), context(8, 1000, 100));
    cast(&mut runtime, 800070002, -1);
    let index = runtime
        .card_hand()
        .iter()
        .position(|card| card.skill_id == Some(800070003))
        .unwrap();
    let round = runtime
        .advance_round(BeginRoundRequest {
            opers: vec![BeginRoundOper {
                oper_type: Some(2),
                param1: Some(index as i32 + 1),
                to_id: Some(-1),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(has_step(&round.fight_step, 0, 800070003, None));
    for uid in [-1, -2] {
        assert_eq!(runtime.entity_info(uid).unwrap().current_hp, Some(70000));
    }
    assert_eq!(runtime.rouge_resources().unwrap().spent_power, 40);
}

#[test]
fn disk_catalyst_permanent_buffs_prefer_missing_statuses_for_every_recipient() {
    let mut runtime = start(fight(100000), context(9, 1000, 100));
    let pool = [90001, 90011, 90041, 90021, 90031, 90281];
    for uid in 10..14 {
        let entity = runtime.entity_info(uid).unwrap();
        let distinct = pool
            .iter()
            .filter(|id| entity.buffs.iter().any(|buff| buff.buff_id == Some(**id)))
            .count();
        assert_eq!(
            distinct, 5,
            "five Disks must give five previously missing permanent statuses, actor {uid}"
        );
    }
    cast(&mut runtime, 800080002, 10);
    for uid in 10..14 {
        let entity = runtime.entity_info(uid).unwrap();
        assert!(
            pool.iter()
                .all(|id| entity.buffs.iter().any(|buff| buff.buff_id == Some(*id))),
            "active must grant the sixth missing status before duplicates, actor {uid}"
        );
    }
}

#[test]
fn disk_catalyst_also_accounts_for_artefact_buffs_queued_in_the_same_opening() {
    let mut frozen = context(9, 1000, 100);
    frozen.entry = Some(EntryContext::default());
    frozen.artefacts.push(Artefact {
        uid: 7,
        item_id: 8140401,
        ..Default::default()
    });
    let runtime = start(fight(100000), frozen);
    let pool = [90001, 90011, 90041, 90021, 90031, 90281];
    for uid in 10..14 {
        let entity = runtime.entity_info(uid).unwrap();
        assert!(
            pool.iter()
                .all(|id| entity.buffs.iter().any(|buff| buff.buff_id == Some(*id)))
        );
        assert_eq!(
            entity
                .buffs
                .iter()
                .find(|buff| buff.buff_id == Some(90001))
                .unwrap()
                .layer,
            Some(1)
        );
    }
}

#[test]
fn pendulum_active_projects_real_master_cards_from_both_rank_three_groups() {
    fn visit<'a>(steps: &'a [FightStep], result: &mut Vec<&'a sonettobuf::ActEffect>) {
        for step in steps {
            for effect in &step.act_effect {
                result.push(effect);
                if let Some(child) = &effect.fight_step {
                    visit(std::slice::from_ref(child), result);
                }
            }
        }
    }
    let initial = fight(100000);
    let owner = initial
        .attacker
        .as_ref()
        .unwrap()
        .entitys
        .iter()
        .find(|entity| entity.uid == Some(10))
        .unwrap();
    let hero_id = owner.model_id.unwrap();
    let catalog = BattleCatalog::new(crate::test_support::game_data());
    let expected: Vec<i32> = [&owner.skill_group1, &owner.skill_group2]
        .into_iter()
        .map(|group| {
            *group
                .iter()
                .find(|id| catalog.skill_rank(**id) == 3)
                .unwrap()
        })
        .collect();
    let frozen = context(4, 1000, 100);
    let mut first = start(initial.clone(), frozen.clone());
    let reply = cast(&mut first, 800030002, 10);
    let mut effects = Vec::new();
    visit(
        &reply
            .round
            .as_ref()
            .expect("catalyst reply includes a round")
            .fight_step,
        &mut effects,
    );
    let cards: Vec<_> = effects
        .into_iter()
        .filter(|effect| effect.effect_type == Some(197))
        .collect();
    assert_eq!(cards.len(), 2);
    for (effect, skill_id) in cards.iter().zip(&expected) {
        let card = effect
            .card_info
            .as_ref()
            .expect("197 must carry the actual card");
        assert_eq!(effect.target_id, Some(10));
        assert_eq!(effect.team_type, Some(1));
        assert_eq!(effect.effect_num.unwrap_or_default(), 0);
        assert_eq!(effect.config_effect.unwrap_or_default(), 0);
        assert_eq!(effect.reserve_id.unwrap_or_default(), 0);
        assert_eq!(
            (card.uid, card.hero_id, card.skill_id),
            (Some(10), Some(hero_id), Some(*skill_id))
        );
        assert_eq!(card.card_type.unwrap_or_default(), 0);
        assert_eq!(card.temp_card, Some(true));
        assert!(first.card_hand().iter().any(|held| held == card));
    }
    assert_eq!(first.rouge_resources().unwrap().power, 84);
    let mut replay = start(initial, frozen);
    assert_eq!(cast(&mut replay, 800030002, 10), reply);
    assert_eq!(first.reconnect_state(), replay.reconnect_state());
}

#[test]
fn pendulum_generated_normal_card_casts_as_its_owner_and_grants_moxie() {
    let mut runtime = start(fight(100000), context(4, 1000, 100));
    let before = runtime
        .entity_info(10)
        .unwrap()
        .ex_point
        .unwrap_or_default();
    let reply = cast(&mut runtime, 800030002, 10);
    fn find_card(steps: &[FightStep]) -> Option<i32> {
        for step in steps {
            for effect in &step.act_effect {
                if effect.effect_type == Some(197) {
                    return effect.card_info.as_ref().and_then(|card| card.skill_id);
                }
                if let Some(child) = &effect.fight_step {
                    if let Some(id) = find_card(std::slice::from_ref(child)) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }
    let skill_id = find_card(&reply.round.unwrap().fight_step).unwrap();
    let index = runtime
        .card_hand()
        .iter()
        .position(|card| {
            card.uid == Some(10) && card.skill_id == Some(skill_id) && card.temp_card == Some(true)
        })
        .unwrap();
    let result = runtime
        .advance_round(BeginRoundRequest {
            opers: vec![BeginRoundOper {
                oper_type: Some(2),
                param1: Some(index as i32 + 1),
                to_id: Some(-1),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(has_step(&result.fight_step, 10, skill_id, None));
    let mut sequence = Vec::new();
    fn walk(steps: &[FightStep], uid: i64, skill: i32, out: &mut Vec<&'static str>) {
        for step in steps {
            if step.from_id == Some(uid) && step.act_id == Some(skill) {
                out.push("skill");
            }
            for effect in &step.act_effect {
                if effect.target_id == Some(uid)
                    && effect.effect_type == Some(111)
                    && effect.effect_num == Some(1)
                {
                    out.push("moxie");
                }
                if let Some(child) = &effect.fight_step {
                    walk(std::slice::from_ref(child), uid, skill, out);
                }
            }
        }
    }
    walk(&result.fight_step, 10, skill_id, &mut sequence);
    assert!(
        sequence.iter().position(|x| *x == "moxie").unwrap()
            < sequence.iter().position(|x| *x == "skill").unwrap()
    );
    assert_eq!(runtime.entity_info(10).unwrap().ex_point, Some(before + 1));
}

#[test]
fn pendulum_first_active_emits_captured_universal_card() {
    let mut initial = fight(100000);
    for enemy in &mut initial.defender.as_mut().unwrap().entitys {
        enemy.model_id = Some(801320301);
    }
    let mut runtime = start(initial, context(4, 1000, 100));
    let reply = cast(&mut runtime, 800030001, -1);
    let round = reply.round.as_ref().unwrap();
    let ids: Vec<_> = round
        .ex_point_info
        .iter()
        .filter_map(|entry| entry.uid)
        .collect();
    for uid in [10, 11, 12, 13, -1, -2] {
        assert!(ids.contains(&uid), "missing combatant {uid}");
    }
    let enemy_ids: Vec<_> = round
        .hero_sp_attributes
        .iter()
        .filter_map(|entry| entry.uid)
        .collect();
    assert!(enemy_ids.contains(&-1) && enemy_ids.contains(&-2));
    assert!(!enemy_ids.contains(&10));
    fn collect<'a>(steps: &'a [FightStep], effects: &mut Vec<&'a sonettobuf::ActEffect>) {
        for step in steps {
            for effect in &step.act_effect {
                effects.push(effect);
                if let Some(nested) = &effect.fight_step {
                    collect(std::slice::from_ref(nested), effects);
                }
            }
        }
    }
    let mut effects = Vec::new();
    collect(&reply.round.as_ref().unwrap().fight_step, &mut effects);
    let universal: Vec<_> = effects
        .iter()
        .filter(|effect| effect.effect_type == Some(58))
        .collect();
    assert_eq!(universal.len(), 1);
    assert_eq!(universal[0].target_id.unwrap_or_default(), 0);
    assert_eq!(universal[0].effect_num, Some(30000001));
    assert_eq!(universal[0].config_effect, Some(60002));
    assert_eq!(universal[0].reserve_id.unwrap_or_default(), 0);
    assert_eq!(universal[0].team_type, Some(1));
    assert!(universal[0].card_info.is_none());
    assert_eq!(runtime.rouge_resources().unwrap().power, 95);
}
