use super::*;
use crate::{
    Battle,
    dungeon::BuiltFight,
    engine::{
        manager::{BattleManagers, card::CardSetup},
        runtime::{determinism::RoundDeterminism, schedule},
        skill::{
            effect::SkillEffectCatalog,
            target::{TargetContext, TargetPool},
        },
    },
    rouge::Artefact,
};
use sonettobuf::{CardInfo, FightEntityInfo, FightStep, FightTeam, HeroAttribute};

fn projected_skill_ids(steps: &[FightStep]) -> Vec<i32> {
    fn visit(step: &FightStep, ids: &mut Vec<i32>) {
        if let Some(id) = step.act_id.filter(|id| *id > 0) {
            ids.push(id);
        }
        for effect in &step.act_effect {
            if let Some(child) = effect.fight_step.as_ref() {
                visit(child, ids);
            }
        }
    }
    let mut ids = Vec::new();
    for step in steps {
        visit(step, &mut ids);
    }
    ids
}

fn context(ids: &[i32]) -> RougeContext {
    RougeContext {
        artefacts: ids
            .iter()
            .enumerate()
            .map(|(index, item_id)| Artefact {
                uid: index as i64 + 100,
                item_id: *item_id,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn fixture(battle_id: i32) -> Fight {
    let entity = |uid, team, current_hp, position| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(if team == 1 { 3003 } else { 1001 }),
        team_type: Some(team),
        position: Some(position),
        current_hp: Some(current_hp),
        attr: Some(HeroAttribute {
            hp: Some(1000),
            attack: Some(100),
            ..Default::default()
        }),
        skill_group1: vec![30030111, 30030112, 30030113],
        skill_group2: vec![30030121, 30030122, 30030123],
        // The real dungeon builder attaches this region skill to attackers.
        // The defender copy emulates a separately sourced identical effect.
        passive_skill: vec![STORM_SLOW_RULE],
        ..Default::default()
    };
    Fight {
        battle_id: Some(battle_id),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 500, 1), entity(11, 1, 800, 2)],
            sub_entitys: vec![entity(13, 1, 700, 3)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1000, 1)],
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn only_configured_attacker_region_receives_placed_contract_skills() {
    crate::test_support::init_config();
    let catalog = BattleCatalog::new(crate::test_support::game_data());
    let mut fight = fixture(8018101);
    prepare(catalog, &mut fight, &context(&[8330615, 8330710, 8330710]));
    prepare(catalog, &mut fight, &context(&[8330615, 8330710]));
    let allies = fight.attacker.as_ref().unwrap();
    for entity in allies.entitys.iter().chain(&allies.sub_entitys) {
        assert_eq!(
            entity
                .passive_skill
                .iter()
                .filter(|id| **id == 8018105)
                .count(),
            1
        );
        assert_eq!(
            entity
                .passive_skill
                .iter()
                .filter(|id| **id == 8018106)
                .count(),
            1
        );
    }
    let enemy = &fight.defender.as_ref().unwrap().entitys[0];
    assert_eq!(enemy.passive_skill, vec![STORM_SLOW_RULE]);

    let mut ordinary = fixture(1211);
    let before = ordinary.clone();
    prepare(catalog, &mut ordinary, &context(&[8330615, 8330710]));
    assert_eq!(ordinary, before);
    let mut empty_layout = fixture(8018101);
    let before = empty_layout.clone();
    prepare(catalog, &mut empty_layout, &context(&[]));
    assert_eq!(empty_layout, before);

    // Some battles have only the area rewrite. A protection contract cannot
    // conjure a separate slow rule where one was not configured.
    let mut rewrite_only = fixture(8024101);
    prepare(catalog, &mut rewrite_only, &context(&[8330615, 8330710]));
    let skills = &rewrite_only.attacker.as_ref().unwrap().entitys[0].passive_skill;
    assert!(skills.contains(&HEAL_REPLACEMENT));
    assert!(!skills.contains(&PROTECTION_REPLACEMENT));
}

fn run_opening(
    ids: &[i32],
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    crate::test_support::init_config();
    let battle_catalog = BattleCatalog::new(crate::test_support::game_data());
    let mut fight = fixture(8018101);
    let context = context(ids);
    prepare(battle_catalog, &mut fight, &context);
    let pool = TargetPool::from_fight(&fight);
    let catalog = SkillEffectCatalog::from_fight(crate::test_support::game_data(), &fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(&mut managers, context);
    let mut rng = RoundDeterminism::with_seed(42);
    let cards = CardSetup {
        hand: vec![
            CardInfo {
                uid: Some(10),
                skill_id: Some(30030111),
                ..Default::default()
            },
            CardInfo {
                uid: Some(11),
                skill_id: Some(30030121),
                ..Default::default()
            },
        ],
        draw_pile: vec![],
        deck_num: 0,
    };
    schedule::run_start(
        battle_catalog,
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            battle_id: 8018101,
            current_round: 1,
            ..Default::default()
        },
        cards,
        2,
    )
    .unwrap();
    (managers, pool, catalog, rng)
}

#[test]
fn actual_region_rules_apply_independent_replacements_and_preserve_enemy_skill() {
    let (mut both, pool, catalog, mut rng) = run_opening(&[8330615, 8330710]);
    assert_eq!(both.buff.buff_id_amount(10, 90281), 3);
    assert_eq!(both.buff.buff_id_amount(11, 90281), 3);
    assert_eq!(both.buff.buff_id_amount(10, 90291), 0);
    assert_eq!(both.buff.buff_id_amount(11, 90291), 0);
    assert_eq!(both.hp.current(10), 550);
    assert_eq!(both.hp.current(11), 820);
    assert_eq!(
        both.buff.buff_id_amount(13, 90281),
        0,
        "reserve is not in the active team"
    );
    assert!(
        both.entity_snapshot(13)
            .unwrap()
            .passive_skill
            .contains(&8018106)
    );
    assert!(
        both.buff
            .has_active_buff_id(ATTACKER_SIDE_UID, HEAL_REPLACEMENT)
    );
    assert!(!both.buff.has_active_buff_id(-1, HEAL_REPLACEMENT));
    assert!(
        both.entity_snapshot(-1)
            .unwrap()
            .passive_skill
            .contains(&STORM_SLOW_RULE)
    );

    schedule::run_round_start_split(
        &mut both,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            battle_id: 8018101,
            current_round: 2,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    assert_eq!(both.buff.buff_id_amount(10, 90281), 6);
    assert_eq!(both.buff.buff_id_amount(11, 90281), 6);
    assert_eq!(both.buff.buff_id_amount(10, 90291), 0);
    assert!(
        both.buff.buff_id_amount(-1, 90291) >= 1,
        "the defender's own identical skill must still apply its slow"
    );
    assert_eq!(both.hp.current(10), 595);
    assert_eq!(both.hp.current(11), 838);
    assert!(
        both.card
            .hand()
            .iter()
            .all(|card| card.enchants.iter().all(|e| e.enchant_id != Some(10003)))
    );

    let (mut untouched, pool, catalog, mut rng) = run_opening(&[]);
    assert_eq!(untouched.buff.buff_id_amount(10, 90291), 1);
    assert_eq!(untouched.buff.buff_id_amount(10, 90281), 0);
    assert_eq!(untouched.hp.current(10), 500);
    schedule::run_round_start_split(
        &mut untouched,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            battle_id: 8018101,
            current_round: 2,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    assert_eq!(
        untouched
            .card
            .hand()
            .iter()
            .filter(|card| card.enchants.iter().any(|e| e.enchant_id == Some(10003)))
            .count(),
        2
    );
}

#[test]
fn contracts_do_not_disable_each_others_region_rule() {
    let (mut protection, pool, catalog, mut rng) = run_opening(&[8330615]);
    assert_eq!(protection.buff.buff_id_amount(10, 90281), 3);
    assert_eq!(protection.buff.buff_id_amount(10, 90291), 0);
    assert_eq!(protection.hp.current(10), 500);
    let (round_start, _) = schedule::run_round_start_split(
        &mut protection,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            battle_id: 8018101,
            current_round: 2,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    assert_eq!(
        protection
            .card
            .hand()
            .iter()
            .filter(|card| card.enchants.iter().any(|e| e.enchant_id == Some(10003)))
            .count(),
        2
    );
    let projected =
        crate::engine::packet::timeline::project_for_version(&round_start.frames, 7).unwrap();
    let ids = projected_skill_ids(&projected);
    assert_eq!(ids.iter().filter(|id| **id == 8018104).count(), 1);
    let protection_end = ids
        .iter()
        .rposition(|id| *id == 8018106)
        .expect("Protection skill");
    let rewrite = ids
        .iter()
        .position(|id| *id == 8018104)
        .expect("Storm rewrite");
    assert!(
        protection_end < rewrite,
        "Protection precedes Storm rewrite: {ids:?}"
    );

    let (mut healing, pool, catalog, mut rng) = run_opening(&[8330710]);
    assert_eq!(healing.buff.buff_id_amount(10, 90291), 1);
    assert_eq!(healing.buff.buff_id_amount(10, 90281), 0);
    assert_eq!(healing.hp.current(10), 550);
    schedule::run_round_start_split(
        &mut healing,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            battle_id: 8018101,
            current_round: 2,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    assert!(
        healing
            .card
            .hand()
            .iter()
            .all(|card| card.enchants.iter().all(|e| e.enchant_id != Some(10003)))
    );
}

#[test]
fn non_rouge_battlefield_rules_run_once_on_their_own_team_boundary() {
    use crate::engine::{
        fight::rules::{DEFENDER_SIDE_UID, OwnedBattleSkill},
        skill::{
            behavior::classify::BehaviorSpec,
            condition::parse_conditions,
            effect::{ParsedBehavior, ParsedSkillEffect, SkillEffectSlot},
            rule::route::ConditionRoute,
            target::TargetRequest,
        },
    };

    crate::test_support::init_config();
    let mut fight = fixture(8018101);
    fight.battle_id = None;
    for team in [&mut fight.attacker, &mut fight.defender]
        .into_iter()
        .flatten()
    {
        for entity in &mut team.entitys {
            entity.passive_skill.clear();
        }
    }
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    managers.battle_rule.extend_owned_skills([
        OwnedBattleSkill {
            owner_uid: ATTACKER_SIDE_UID,
            skill_id: 99000001,
        },
        OwnedBattleSkill {
            owner_uid: DEFENDER_SIDE_UID,
            skill_id: 99000001,
        },
    ]);
    let mut slot = SkillEffectSlot::new(
        ParsedBehavior::from_spec(BehaviorSpec::new(1, "AddBuff"), vec![90291], Vec::new()),
        TargetRequest {
            code: 103,
            raw: Vec::new(),
        },
    );
    slot.condition_target = TargetRequest {
        code: 103,
        raw: Vec::new(),
    };
    slot.conditions = parse_conditions(crate::test_support::game_data(), "57104#8018105");
    slot.compiled_route = ConditionRoute::compile(&slot.conditions);
    let mut catalog = SkillEffectCatalog::default();
    catalog.insert(ParsedSkillEffect {
        skill_id: 99000001,
        slots: vec![slot],
    });
    let mut rng = RoundDeterminism::with_seed(42);
    let target = TargetContext {
        current_round: 2,
        ..Default::default()
    };

    schedule::run_before_ai_round_start(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        target,
        2,
        &[],
        None,
    )
    .unwrap();
    assert_eq!(managers.buff.buff_id_amount(DEFENDER_SIDE_UID, 90291), 1);
    assert_eq!(managers.buff.buff_id_amount(ATTACKER_SIDE_UID, 90291), 0);
    schedule::run_round_start_after_ai_split(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        target,
        &[],
        0,
    )
    .unwrap();
    assert_eq!(managers.buff.buff_id_amount(DEFENDER_SIDE_UID, 90291), 1);
    assert_eq!(managers.buff.buff_id_amount(ATTACKER_SIDE_UID, 90291), 1);
}

#[test]
fn real_battle_start_rebuilds_same_storm_skills_without_extra_rule_version() {
    crate::test_support::init_config();
    let catalog = BattleCatalog::new(crate::test_support::game_data());
    let built = || BuiltFight {
        fight: fixture(8018101),
        ex_attributes: vec![],
        sp_attributes: vec![],
        rouge: Some(context(&[8330615, 8330710])),
        battle_rule_skills: vec![],
    };
    let first = Battle::start(catalog, built(), 42).unwrap();
    let replayed = Battle::start(catalog, built(), 42).unwrap();
    assert_eq!(first.reconnect_state(), replayed.reconnect_state());
    for uid in [10, 11, 13] {
        let entity = first.entity_info(uid).unwrap();
        assert!(entity.passive_skill.contains(&HEAL_REPLACEMENT));
        assert!(entity.passive_skill.contains(&PROTECTION_REPLACEMENT));
    }

    let ordinary_layout = BuiltFight {
        fight: fixture(8018101),
        ex_attributes: vec![],
        sp_attributes: vec![],
        rouge: Some(context(&[])),
        battle_rule_skills: vec![],
    };
    let opening = Battle::start(catalog, ordinary_layout, 42).unwrap();
    let round = opening.reconnect_state().1.unwrap();
    assert!(
        !projected_skill_ids(&round.fight_step).contains(&8018104),
        "the opening empty hand must not publish a card rewrite skill"
    );
}
