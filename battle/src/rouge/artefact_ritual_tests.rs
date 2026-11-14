use super::*;
use crate::engine::{
    event::payload::BuffChangeEvent,
    manager::{
        buff::{BuffConsume, BuffSelector, DepletedBuff},
        hp::{DamageEffectKind, HpCommand, HpDamage, HurtInfoData},
    },
    runtime::{determinism::RoundDeterminism, drain, schedule},
    skill::{
        action::{SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
        effect::{ParsedSkillEffect, SkillEffectCatalog},
        target::TargetContext,
    },
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn fight() -> Fight {
    let entity = |uid, team, hp| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(hp),
        ex_skill: (uid == 10).then_some(30030131),
        skill_group1: if uid == 10 { vec![30030111] } else { vec![] },
        attr: Some(HeroAttribute {
            hp: Some(1_000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1_000), entity(11, 1, 1_000), entity(12, 1, 0)],
            sub_entitys: vec![entity(13, 1, 1_000)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1_000)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    }
}

fn setup(
    items: Option<Vec<Artefact>>,
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    crate::test_support::init_config();
    let fight = fight();
    let mut managers = BattleManagers::seeded(&fight);
    if let Some(items) = items {
        super::super::configure(
            &mut managers,
            RougeContext {
                artefacts: items,
                ..Default::default()
            },
        );
    }
    (
        managers,
        TargetPool::from_fight(&fight),
        SkillEffectCatalog::default(),
        RoundDeterminism::with_seed(42),
    )
}

fn item(uid: i64, count2: i32, in_range: i32) -> Artefact {
    Artefact {
        uid,
        item_id: 8320207,
        count2,
        ritual_in_range_count: in_range,
        ..Default::default()
    }
}

fn consume(uid: i64, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::ConsumeCoalesced(
        BuffConsume {
            origin: ORIGIN,
            target_uid: uid,
            selector: BuffSelector::ExactId(90091),
            amount,
            depleted: DepletedBuff::Remove,
        },
    )))
}

fn run(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    round: i32,
    ops: Vec<RuleOp>,
) -> Vec<String> {
    drain::run_command_group(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: round,
            ..Default::default()
        },
        ops,
    )
    .unwrap()
    .frames
    .into_iter()
    .map(|frame| format!("{frame:?}"))
    .collect()
}

fn damage(source: i64, target: i64, amount: i32, from: HurtDamageFromType) -> RuleOp {
    RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(HpDamage {
        origin: ORIGIN,
        source_uid: source,
        target_uid: target,
        amount,
        config_effect: 1,
        effect_kind: DamageEffectKind::Normal,
        assassinate: false,
        ignore_riposte: false,
        hurt: HurtInfoData {
            from_uid: source,
            is_crit: false,
            career_restraint: false,
            reduce_hp: amount,
            effect_id: 1,
            skill_id: 30030111,
            damage_from: from,
            buff_act_id: 0,
            buff_uid: 0,
            hurt_effect_type: 0,
            display_amount: None,
        },
    })))
}

#[test]
fn committed_enemy_hits_grant_ritual_to_only_the_living_hit_actor() {
    let (mut managers, pool, catalog, mut rng) = setup(Some(vec![item(1, 0, 0)]));
    managers.hp.set_shield(10, 100);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![damage(-1, 10, 50, HurtDamageFromType::Skill)],
    );
    assert_eq!(
        managers.hp.current(10),
        1_000,
        "a shielded hit is still an attack"
    );
    assert_eq!(managers.buff.buff_id_amount(10, 90091), 5);
    assert_eq!(managers.buff.buff_id_amount(11, 90091), 0);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![damage(-1, 11, 100, HurtDamageFromType::Skill)],
    );
    assert_eq!(managers.buff.buff_id_amount(11, 90091), 5);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![
            damage(11, 10, 10, HurtDamageFromType::Skill),
            damage(-1, 10, 10, HurtDamageFromType::Buff),
            damage(-1, 12, 10, HurtDamageFromType::Skill),
        ],
    );
    assert_eq!(
        managers.buff.buff_id_amount(10, 90091),
        5,
        "friendly damage and status ticks do not count as enemy attacks"
    );
    assert_eq!(managers.buff.buff_id_amount(12, 90091), 0);
    assert_eq!(
        managers.buff.buff_id_amount(13, 90091),
        0,
        "reserve allies do not receive field effects"
    );
    let (mut plain, pool, catalog, mut rng) = setup(None);
    run(
        &mut plain,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![damage(-1, 10, 10, HurtDamageFromType::Skill)],
    );
    assert_eq!(plain.buff.buff_id_amount(10, 90091), 0);
}

#[test]
fn ritual_loss_accumulates_across_living_allies_and_rounds_and_ignores_duplicates() {
    let (mut managers, pool, catalog, mut rng) = setup(Some(vec![item(1, 0, 2)])); // 50 layers
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![give(10, 90091, 49), give(11, 90091, 2)],
    );
    let first_uid = managers.buff.buff_id_or_type_uid(10, 90091).unwrap();
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![consume(10, 49)],
    );
    assert_eq!(managers.buff.buff_id_amount(10, 90031), 0);
    let duplicate = BattleEvent::BuffRemoved(BuffChangeEvent {
        source_uid: 10,
        target_uid: 10,
        buff_uid: first_uid,
        buff_id: 90091,
        before_amount: 49,
        after_amount: 0,
        act_id: 0,
        act_value: 0,
        duration_expired: false,
    });
    assert!(observe(&mut managers, &pool, &duplicate).is_empty());
    schedule::run_attacker_round_end(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
    )
    .unwrap();
    schedule::run_round_start_split(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 2,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        2,
        vec![consume(11, 1)],
    );
    for uid in [10, 11] {
        assert_eq!(managers.buff.buff_id_amount(uid, 90031), 5);
        assert_eq!(managers.buff.buff_id_amount(uid, 90171), 1);
    }
    assert_eq!(managers.buff.buff_id_amount(12, 90031), 0);
    assert_eq!(managers.buff.buff_id_amount(13, 90031), 0);
    assert_eq!(
        managers.rouge.active.as_ref().unwrap().ritual_loss_progress[&1],
        0
    );
}

#[test]
fn multiple_artefacts_apply_each_threshold_and_replay_without_leaking_to_new_battles() {
    let run_case = || {
        let (mut managers, pool, catalog, mut rng) =
            setup(Some(vec![item(1, 0, 2), item(2, 0, 0)]));
        let frames = run(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            1,
            vec![give(10, 90091, 100), consume(10, 100)],
        );
        for uid in [10, 11] {
            assert_eq!(
                managers.buff.buff_id_amount(uid, 90031),
                15,
                "two crossings at 50 and one at 100"
            );
            assert_eq!(managers.buff.buff_id_amount(uid, 90171), 3);
        }
        assert!(
            managers
                .rouge
                .active
                .as_ref()
                .unwrap()
                .ritual_loss_progress
                .values()
                .all(|n| *n == 0)
        );
        (
            frames,
            managers.buff.buff_id_amount(10, 90031),
            managers.buff.buff_id_amount(11, 90171),
        )
    };
    assert_eq!(run_case(), run_case());
    let (fresh, _, _, _) = setup(Some(vec![item(1, 0, 2)]));
    assert!(
        fresh
            .rouge
            .active
            .as_ref()
            .unwrap()
            .ritual_loss_progress
            .is_empty()
    );
}

#[test]
fn enhancement_changes_threshold_and_a_single_loss_can_cross_it_multiple_times() {
    let (mut managers, pool, catalog, mut rng) = setup(Some(vec![item(7, 99, 0)]));
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![give(10, 90091, 3), consume(10, 3)],
    );
    for uid in [10, 11] {
        assert_eq!(managers.buff.buff_id_amount(uid, 90031), 15);
        assert_eq!(managers.buff.buff_id_amount(uid, 90171), 3);
    }
    assert_eq!(
        managers.rouge.active.as_ref().unwrap().ritual_loss_progress[&7],
        0
    );
}

#[test]
fn real_ultimate_expires_ritual_and_grants_the_threshold_reward() {
    let (mut managers, pool, mut catalog, mut rng) = setup(Some(vec![item(1, 51, 0)])); // 49 layers
    catalog.insert(ParsedSkillEffect {
        skill_id: 30030131,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030131, 1_000);
    catalog.insert_logic_target(30030131, 1);
    catalog.insert_effect_tag(30030131, 1);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![give(10, 90091, 49)],
    );
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: 10,
        skill_id: 30030131,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.target = SkillTarget::Explicit(-1);
    drain::run_skill(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        invocation,
        SkillModifiers::default(),
    )
    .unwrap();
    assert_eq!(
        managers.buff.buff_id_amount(10, 90091),
        0,
        "ritual is consumed by the real ultimate"
    );
    for uid in [10, 11] {
        assert_eq!(managers.buff.buff_id_amount(uid, 90031), 5);
        assert_eq!(managers.buff.buff_id_amount(uid, 90171), 1);
    }
}
