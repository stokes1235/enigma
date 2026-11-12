use super::*;
use sonettobuf::{CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

use crate::engine::{
    event::payload::EntityDiedEvent,
    manager::card::{CardCommand, CardManager, CardPlay},
    runtime::{determinism::RoundDeterminism, drain, schedule},
    skill::{
        action::{
            ActionEvent, SkillInvocation, SkillLifecycle, SkillModifiers, SkillRequest, SkillTarget,
        },
        effect::{ParsedBehavior, ParsedSkillEffect, SkillEffectCatalog, SkillEffectSlot},
        target::{TargetContext, TargetRequest},
    },
};

fn fight() -> Fight {
    crate::test_support::init_config();
    let entity = |uid, hp| FightEntityInfo {
        uid: Some(uid),
        current_hp: Some(hp),
        attr: Some(HeroAttribute {
            hp: Some(hp),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1000), entity(11, 1000)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 5000), entity(-2, 5000)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    }
}

fn fixture(
    context: Option<RougeContext>,
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    let fight = fight();
    let mut managers = BattleManagers::seeded(&fight);
    if let Some(context) = context {
        configure(&mut managers, context);
    }
    (
        managers,
        TargetPool::from_fight(&fight),
        SkillEffectCatalog::default(),
        RoundDeterminism::with_seed(42),
    )
}

fn context(items: &[i32]) -> RougeContext {
    RougeContext {
        artefacts: items
            .iter()
            .enumerate()
            .map(|(index, id)| Artefact {
                uid: index as i64 + 100,
                item_id: *id,
                ..Default::default()
            })
            .collect(),
        first_slot_uid: Some(10),
        ..Default::default()
    }
}

fn run(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    random: &mut RoundDeterminism,
    round: i32,
    ops: Vec<RuleOp>,
) -> drain::DrainResult {
    drain::run_command_group(
        managers,
        pool,
        catalog,
        random,
        TargetContext {
            current_round: round,
            ..Default::default()
        },
        ops,
    )
    .unwrap()
}

#[test]
fn server_only_genesis_attribute_changes_real_damage_and_is_serde_stable() {
    fn damage(context: Option<RougeContext>) -> i32 {
        let (mut managers, pool, mut catalog, mut random) = fixture(context);
        catalog.insert(ParsedSkillEffect {
            skill_id: 100,
            slots: vec![SkillEffectSlot::new(
                ParsedBehavior::new(10006, "Damage", vec![1, 101, 100]),
                TargetRequest {
                    code: 0,
                    raw: vec![],
                },
            )],
        });
        let mut invocation: SkillInvocation = SkillRequest {
            source_uid: 10,
            skill_id: 100,
        }
        .into();
        invocation.target = SkillTarget::Explicit(-1);
        drain::run_skill(
            &mut managers,
            &pool,
            &catalog,
            &mut random,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            invocation,
            SkillModifiers::default(),
        )
        .unwrap();
        5000 - managers.hp.current(-1)
    }
    let mut initial = context(&[]);
    initial.extra_attributes = vec![(10, 217, 1000)];
    let restored: RougeContext =
        serde_json::from_str(&serde_json::to_string(&initial).unwrap()).unwrap();
    assert_eq!(damage(None), 500);
    assert_eq!(damage(Some(initial)), 1000);
    assert_eq!(damage(Some(restored)), 1000);
}

#[test]
fn bottle_enters_once_and_rank_two_cast_emits_a_real_taunt() {
    let (mut managers, pool, catalog, mut random) = fixture(Some(context(&[8130315])));
    let ops = opening(&mut managers, &pool);
    let entry = run(&mut managers, &pool, &catalog, &mut random, 1, ops);
    assert!(managers.buff.has_buff_id(10, 5042));
    assert!(!entry.frames.is_empty());
    assert!(opening(&mut managers, &pool).is_empty());
    managers
        .execute_buff(BuffCommand::Remove(
            crate::engine::manager::buff::BuffRemove {
                origin: ORIGIN,
                target_uid: 10,
                selector: crate::engine::manager::buff::BuffRemoveSelector::ExactId(5042),
            },
        ))
        .unwrap();
    let action = ActionEvent {
        source_uid: 11,
        skill_id: 30030112,
        rank: 2,
        card_index: 1,
        ..Default::default()
    };
    let cast = run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        1,
        vec![RuleOp::SkillLifecycle(SkillLifecycle::ActionCompleted(
            action,
        ))],
    );
    assert!(managers.buff.has_buff_id(10, 5041));
    assert!(!cast.frames.is_empty());
    assert!(!managers.buff.has_buff_id(11, 5041));
}

#[test]
fn aid_cast_drives_round_end_marionette_shield_once_and_preserves_old_shield() {
    let mut initial = context(&[8320109]);
    initial.artefacts[0].count2 = 5;
    initial.artefacts[0].only_support_below = true;
    let (mut managers, pool, catalog, mut random) = fixture(Some(initial));
    managers.card = CardManager::new(vec![CardInfo {
        uid: Some(10),
        skill_id: Some(30030111),
        card_type: Some(2),
        ..Default::default()
    }]);
    managers.hp.set_shield(10, 400);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        1,
        vec![
            RuleOp::Command(BattleCommand::Card(CardCommand::Play(CardPlay {
                origin: ORIGIN,
                hand_index: 0,
                target_uid: Some(-1),
                chosen_skill_id: None,
                choice: None,
                recorded_skill: None,
            }))),
            RuleOp::SkillLifecycle(SkillLifecycle::ActionCompleted(ActionEvent {
                source_uid: 10,
                skill_id: 30030111,
                card_index: 1,
                rank: 1,
                ..Default::default()
            })),
        ],
    );
    let end = schedule::run_attacker_round_end(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(managers.hp.shield(10), 700); // Old 400 + (15+5+10)% of 1000.
    assert_eq!(managers.hp.shield(11), 300);
    assert!(managers.buff.has_buff_id(10, 30740121));
    assert!(!end.frames.is_empty());
    assert!(round_end(&mut managers, &pool, 1).is_empty());
    assert!(round_end(&mut managers, &pool, 2).is_empty());
}

#[test]
fn unsupported_serum_kill_cards_are_not_added_to_stall_the_next_round() {
    let (mut managers, pool, catalog, mut random) = fixture(Some(context(&[8210045])));
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        1,
        vec![RuleOp::Publish(BattleEvent::EntityDied(EntityDiedEvent {
            source_uid: 10,
            target_uid: -1,
        }))],
    );
    assert!(!managers.buff.has_buff_id(10, 90331));
    assert!(!managers.buff.has_buff_id(10, 90082));
    assert!(!managers.buff.has_buff_id(11, 90331));
    assert!(!managers.buff.has_buff_id(-1, 90082));
}

#[test]
fn identical_seed_context_and_operations_replay_all_event_state() {
    fn played(initial: RougeContext) -> (BattleManagers, drain::DrainResult) {
        let (mut managers, pool, catalog, mut random) = fixture(Some(initial));
        let ops = opening(&mut managers, &pool);
        let mut result = run(&mut managers, &pool, &catalog, &mut random, 1, ops);
        let death = run(
            &mut managers,
            &pool,
            &catalog,
            &mut random,
            1,
            vec![RuleOp::Publish(BattleEvent::EntityDied(EntityDiedEvent {
                source_uid: 10,
                target_uid: -1,
            }))],
        );
        result.frames.extend(death.frames);
        (managers, result)
    }
    let initial = context(&[8130315, 8320109]);
    let (left, left_frames) = played(initial.clone());
    let restored = serde_json::from_str(&serde_json::to_string(&initial).unwrap()).unwrap();
    let (right, right_frames) = played(restored);
    assert_eq!(
        format!("{:?}", left_frames.frames),
        format!("{:?}", right_frames.frames)
    );
    let mut left_fight = fight();
    left.sync_entities(&mut left_fight);
    let mut right_fight = fight();
    right.sync_entities(&mut right_fight);
    assert_eq!(left_fight, right_fight);
    assert_eq!(left.rouge.context(), right.rouge.context());
}

#[test]
fn non_rouge_events_create_no_artefact_effects() {
    let (mut managers, pool, catalog, mut random) = fixture(None);
    assert!(opening(&mut managers, &pool).is_empty());
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        1,
        vec![RuleOp::Publish(BattleEvent::EntityDied(EntityDiedEvent {
            source_uid: 10,
            target_uid: -1,
        }))],
    );
    assert!(!managers.buff.has_buff_id(10, 90082));
    assert!(round_end(&mut managers, &pool, 1).is_empty());
    assert!(managers.rouge.context().is_none());
}

#[test]
fn conditional_damage_belongs_only_to_the_qualifying_skill_execution() {
    let mut initial = context(&[]);
    initial.conditional_modifiers = vec![ConditionalModifier {
        source_uid: 100,
        item_id: 8110312,
        predicate: AttackPredicate::All(vec![
            AttackPredicate::RankAtLeast(2),
            AttackPredicate::NonUltimate,
        ]),
        attr_id: 205,
        value: 500,
    }];
    let (mut managers, pool, mut catalog, mut random) = fixture(Some(initial));
    for skill_id in [30030111, 30030112] {
        catalog.insert(ParsedSkillEffect {
            skill_id,
            slots: vec![],
        });
        catalog.insert_damage_rate(skill_id, 1000);
        catalog.insert_logic_target(skill_id, 1);
        catalog.insert_effect_tag(skill_id, 1);
    }
    let mut damages = Vec::new();
    for skill_id in [30030112, 30030111, 30030112] {
        let before = managers.hp.current(-1);
        let mut invocation: SkillInvocation = SkillRequest {
            source_uid: 10,
            skill_id,
        }
        .into();
        invocation.mode = crate::engine::skill::action::SkillExecutionMode::Active;
        invocation.target = SkillTarget::Explicit(-1);
        drain::run_skill(
            &mut managers,
            &pool,
            &catalog,
            &mut random,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            invocation,
            SkillModifiers::default(),
        )
        .unwrap();
        damages.push(before - managers.hp.current(-1));
        assert_eq!(managers.attribute.get(10, AttrId::DmgBonus), 0);
    }
    assert_eq!(damages, [150, 100, 150]);
}
