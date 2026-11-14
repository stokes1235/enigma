use super::*;
use sonettobuf::{FightTeam, HeroAttribute};

fn tables() -> &'static config::GameDB {
    super::super::original_tests::init_config();
    config::configs::get()
}

fn enemy(uid: i64) -> FightEntityInfo {
    let attr = HeroAttribute {
        hp: Some(1000),
        attack: Some(1000),
        defense: Some(500),
        mdefense: Some(600),
        ..Default::default()
    };
    FightEntityInfo {
        uid: Some(uid),
        model_id: Some(801110301),
        entity_type: Some(2),
        current_hp: Some(750),
        attr: Some(attr),
        base_attr: Some(attr),
        ..Default::default()
    }
}

#[test]
fn explicit_noise_thresholds_accumulate_without_guessing_hp_or_step_formula() {
    let tables = tables();
    let one = profile(tables, 1, Encounter::MinorObstacle).unwrap();
    assert!(one.enemy_attributes.is_empty());
    assert_eq!(one.dynamic, DynamicRules::default());
    let seven = profile(tables, 7, Encounter::MinorObstacle).unwrap();
    assert_eq!(
        seven.enemy_attributes,
        BTreeMap::from([(102, 150), (205, 350), (206, 150), (207, 100)])
    );
    let twenty = profile(tables, 20, Encounter::FinalBoss).unwrap();
    assert_eq!(
        twenty.enemy_attributes,
        BTreeMap::from([
            (102, 550),
            (103, 400),
            (104, 400),
            (205, 350),
            (206, 350),
            (207, 500),
            (208, 500)
        ])
    );
    assert!(!twenty.enemy_attributes.contains_key(&101));
    assert_eq!(twenty.dynamic.attack_growth_start_round, 6);
    assert_eq!(twenty.dynamic.attack_growth_per_round, 50);
    assert_eq!(twenty.dynamic.scalding_cards, 2);
    assert_eq!(twenty.dynamic.stronger_afflatus, 300);
    assert!(profile(tables, 0, Encounter::Other).is_err());
    assert!(profile(tables, 21, Encounter::Other).is_err());
    // Raw data is exposed for future evidence work, never treated as a ready
    // multiplier. The irregular11th-difficulty HP series must remain intact.
    assert_eq!(tables.rouge_layer_step_attr(1, 4), Some("hp#359|addDmg#59"));
    assert_eq!(
        tables.rouge_layer_step_attr(11, 1),
        Some("hp#290|addDmg#33")
    );
    assert_eq!(tables.rouge_layer_step_attr(20, 6), None);
}

#[test]
fn extra_ap_is_specific_to_the_documented_encounter_types() {
    let tables = tables();
    for encounter in [Encounter::MinorObstacle, Encounter::PresentDanger] {
        assert_eq!(
            profile(tables, 16, encounter)
                .unwrap()
                .dynamic
                .extra_enemy_ap,
            0
        );
        assert_eq!(
            profile(tables, 17, encounter)
                .unwrap()
                .dynamic
                .extra_enemy_ap,
            1
        );
        assert_eq!(
            profile(tables, 20, encounter)
                .unwrap()
                .dynamic
                .extra_enemy_ap,
            1
        );
    }
    for encounter in [Encounter::Crisis, Encounter::FinalBoss] {
        assert_eq!(
            profile(tables, 18, encounter)
                .unwrap()
                .dynamic
                .extra_enemy_ap,
            0
        );
        assert_eq!(
            profile(tables, 19, encounter)
                .unwrap()
                .dynamic
                .extra_enemy_ap,
            1
        );
    }
    assert_eq!(
        profile(tables, 20, Encounter::Other)
            .unwrap()
            .dynamic
            .extra_enemy_ap,
        0
    );
}

#[test]
fn only_defender_monsters_scale_and_existing_damage_rates_are_preserved() {
    let tables = tables();
    let ally = enemy(10);
    let mut fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![ally.clone()],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy(-1)],
            sub_entitys: vec![enemy(-2)],
            sp_entitys: vec![enemy(-3)],
            sp_fight_entities: vec![enemy(-4)],
            player_entity: Some(FightEntityInfo {
                uid: Some(-99999),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut ex = vec![(
        -1,
        HeroExAttribute {
            add_dmg: Some(600),
            ..Default::default()
        },
    )];
    let mut sp = vec![(
        -1,
        HeroSpAttribute {
            final_add_dmg: Some(80),
            ..Default::default()
        },
    )];
    let profile = profile(tables, 20, Encounter::FinalBoss).unwrap();
    assert_eq!(
        apply_to_defenders(tables, &mut fight, &mut ex, &mut sp, &profile),
        4
    );
    assert_eq!(fight.attacker.as_ref().unwrap().entitys[0], ally);
    for entity in fight
        .defender
        .as_ref()
        .unwrap()
        .entitys
        .iter()
        .chain(&fight.defender.as_ref().unwrap().sub_entitys)
        .chain(&fight.defender.as_ref().unwrap().sp_entitys)
        .chain(&fight.defender.as_ref().unwrap().sp_fight_entities)
    {
        let attr = entity.attr.as_ref().unwrap();
        assert_eq!(
            (attr.attack, attr.defense, attr.mdefense),
            (Some(1550), Some(700), Some(840))
        );
        assert_eq!((attr.hp, entity.current_hp), (Some(1000), Some(750)));
        assert_eq!(entity.base_attr.as_ref().unwrap().attack, Some(1550));
    }
    assert_eq!(
        ex.iter().find(|(uid, _)| *uid == -1).unwrap().1.add_dmg,
        Some(950)
    );
    let base = tables
        .monster_template
        .iter()
        .find(|row| row.template == 801110301)
        .unwrap();
    assert_eq!(
        ex.iter().find(|(uid, _)| *uid == -2).unwrap().1.add_dmg,
        Some(base.add_dmg + 350)
    );
    assert_eq!(
        sp.iter()
            .find(|(uid, _)| *uid == -1)
            .unwrap()
            .1
            .final_add_dmg,
        Some(580)
    );
    let context = runtime_context(&profile, &fight).unwrap();
    assert_eq!(context.initial_enemy_uids, vec![-1, -2, -3, -4]);
    assert_eq!(
        context.enemy_attributes,
        profile.enemy_attributes.into_iter().collect::<Vec<_>>()
    );
}

#[test]
fn difficulty_one_is_a_deep_no_op_for_fight_and_extra_attributes() {
    let tables = tables();
    let mut fight = Fight {
        defender: Some(FightTeam {
            entitys: vec![enemy(-1)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut ex = vec![(
        -1,
        HeroExAttribute {
            add_dmg: Some(123),
            ..Default::default()
        },
    )];
    let mut sp = vec![(
        -1,
        HeroSpAttribute {
            final_drop_dmg: Some(77),
            ..Default::default()
        },
    )];
    let before = (fight.clone(), ex.clone(), sp.clone());
    let profile = profile(tables, 1, Encounter::MinorObstacle).unwrap();
    assert_eq!(
        apply_to_defenders(tables, &mut fight, &mut ex, &mut sp, &profile),
        0
    );
    assert_eq!((fight.clone(), ex, sp), before);
    assert!(runtime_context(&profile, &fight).is_none());
}

#[test]
fn local_progression_is_bounded_monotonic_and_preserves_zero_step_baseline() {
    let tables = tables();
    let mut first = profile(tables, 1, Encounter::MinorObstacle).unwrap();
    apply_local_progression(&mut first, 0);
    assert!(first.enemy_attributes.is_empty());
    let mut midway = profile(tables, 7, Encounter::MinorObstacle).unwrap();
    apply_local_progression(&mut midway, 10);
    assert_eq!(midway.enemy_attributes[&101], 250);
    assert_eq!(midway.enemy_attributes[&102], 150); // configured difficulty only
    let mut capped = profile(tables, 20, Encounter::FinalBoss).unwrap();
    apply_local_progression(&mut capped, i32::MAX);
    assert_eq!(capped.enemy_attributes[&101], 775);
    assert_eq!(capped.enemy_attributes[&102], 550);
    let mut negative = profile(tables, 1, Encounter::MinorObstacle).unwrap();
    apply_local_progression(&mut negative, i32::MIN);
    assert!(negative.enemy_attributes.is_empty());
}

#[test]
fn completed_nodes_change_the_local_hp_approximation_without_extra_attack_growth() {
    let tables = tables();
    let mut first = profile(tables, 6, Encounter::MinorObstacle).unwrap();
    let mut later = first.clone();
    apply_local_progression(&mut first, 0);
    apply_local_progression(&mut later, 10);
    let fight = Fight {
        defender: Some(FightTeam {
            entitys: vec![enemy(-1)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut early_fight = fight.clone();
    let mut later_fight = fight;
    apply_to_defenders(tables, &mut early_fight, &mut vec![], &mut vec![], &first);
    apply_to_defenders(tables, &mut later_fight, &mut vec![], &mut vec![], &later);
    let early = early_fight.defender.unwrap().entitys.remove(0);
    let late = later_fight.defender.unwrap().entitys.remove(0);
    assert_eq!(early.attr.as_ref().unwrap().attack, Some(1150));
    assert_eq!(late.attr.as_ref().unwrap().attack, Some(1150));
    assert_eq!(early.attr.unwrap().hp, Some(1125));
    assert_eq!(late.attr.unwrap().hp, Some(1225));
}

#[test]
fn inferred_health_scales_current_max_and_base_only_for_defenders() {
    let tables = tables();
    let mut fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![enemy(1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy(-1)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut profile = profile(tables, 7, Encounter::MinorObstacle).unwrap();
    apply_local_progression(&mut profile, 10);
    apply_to_defenders(tables, &mut fight, &mut vec![], &mut vec![], &profile);
    let defender = &fight.defender.as_ref().unwrap().entitys[0];
    assert_eq!(defender.attr.as_ref().unwrap().hp, Some(1250));
    assert_eq!(defender.base_attr.as_ref().unwrap().hp, Some(1250));
    assert_eq!(defender.current_hp, Some(937));
    assert_eq!(
        fight.attacker.as_ref().unwrap().entitys[0].current_hp,
        Some(750)
    );
    let context = runtime_context(&profile, &fight).unwrap();
    assert!(context.enemy_attributes.contains(&(101, 250)));
    assert_eq!(context.initial_enemy_uids, vec![-1]);
}

#[test]
fn saturated_health_scaling_preserves_wounded_fraction() {
    let mut entity = enemy(-1);
    entity.attr.as_mut().unwrap().hp = Some(i32::MAX);
    entity.base_attr.as_mut().unwrap().hp = Some(i32::MAX);
    entity.current_hp = Some(i32::MAX / 2);
    let old = entity.attr.as_ref().unwrap().hp;
    scale(&mut entity.attr.as_mut().unwrap().hp, 775);
    scale_current_hp(&mut entity, old, 775);
    assert_eq!(entity.attr.as_ref().unwrap().hp, Some(i32::MAX));
    assert_eq!(entity.current_hp, Some(i32::MAX / 2));
}

#[test]
fn run_progress_uses_archived_and_current_resolved_nodes_once() {
    let graph = super::super::map::LayerMap {
        layer_id: 111,
        stages: 4,
        nodes: (1..=4)
            .map(|id| super::super::map::MapNode {
                stage: id - 1,
                node_id: id,
                last: vec![],
                next: vec![],
                event_id: id,
                node_type: 1,
            })
            .collect(),
    };
    let start = RougeStart {
        user_id: 1,
        season: 1,
        state: 1,
        difficulty: 7,
        style: 1,
        hero_ids: String::new(),
        assist_hero_uid: 0,
        point_remainder: 0,
        genius_remainder: 0,
        rewards: String::new(),
    };
    let mut run = RougeRun {
        user_id: 1,
        season: 1,
        layer_index: 0,
        layer_id: 111,
        stage: 3,
        node_id: 4,
        seed: 1,
        graph: serde_json::to_string(&graph).unwrap(),
        coin: 0,
        power: 0,
        updated_at: 0,
        map_type: 1,
        middle_layer_id: 0,
        position_index: 0,
        choices: r#"{"1":1,"2":1}"#.into(),
        finished: "[2,3]".into(),
        bag: String::new(),
        shop: String::new(),
        spent_coin: 0,
        retry_num: 0,
        terminal_state: 0,
        minor_count: 0,
        danger_count: 0,
        crisis_count: 0,
        step_count: 7,
        team: String::new(),
        power_limit: 0,
        interactive_index: 0,
        interactive_refresh: 0,
        battle_interactive: String::new(),
        team_exp: 0,
        middle_state: String::new(),
        details: String::new(),
    };
    let result = for_run(tables(), &start, &run).unwrap();
    assert_eq!(result.enemy_attributes[&101], 250); // 7 archived + 3 unique nodes
    assert_eq!(result.enemy_attributes[&102], 150);
    assert_eq!(hp_rate_for_run(tables(), &start, &run).unwrap(), 250);
    run.map_type = 2; // final boss retains the current layer's graph
    let final_boss = for_run(tables(), &start, &run).unwrap();
    assert_eq!(final_boss.enemy_attributes[&101], 250);
    run.graph = "{}".into(); // migration 099 default; node accounting survives
    let legacy_final = for_run(tables(), &start, &run).unwrap();
    assert_eq!(legacy_final.enemy_attributes, final_boss.enemy_attributes);
    run.map_type = 1; // ordinary encounters still require their real map node
    assert!(for_run(tables(), &start, &run).is_err());
    // The monster preview is still useful before a normal node is selected.
    assert_eq!(hp_rate_for_run(tables(), &start, &run).unwrap(), 250);
    let mut unselected = start.clone();
    unselected.difficulty = 0;
    assert_eq!(hp_rate_for_run(tables(), &unselected, &run).unwrap(), 0);
}
