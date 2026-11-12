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
