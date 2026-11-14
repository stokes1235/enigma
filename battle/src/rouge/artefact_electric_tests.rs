use super::*;
use crate::engine::{
    manager::buff::{BuffConsume, BuffSelector, DepletedBuff},
    runtime::drain,
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn fight() -> Fight {
    let entity = |uid, team| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(1000),
        attr: Some(HeroAttribute {
            hp: Some(1000),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1), entity(11, 1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2)],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn consume(uid: i64, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Buff(BuffCommand::ConsumeCoalesced(
        BuffConsume {
            origin: ORIGIN,
            target_uid: uid,
            selector: BuffSelector::ExactId(90071),
            amount,
            depleted: DepletedBuff::Remove,
        },
    )))
}

#[test]
fn committed_losses_preserve_remainders_per_actor_and_restoration_does_not_recurse() {
    crate::test_support::init_config();
    let fight = fight();
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: [8120808, 8220813, 8320812]
                .into_iter()
                .enumerate()
                .map(|(i, id)| Artefact {
                    uid: i as i64 + 1,
                    item_id: id,
                    count2: 8,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
    let catalog = SkillEffectCatalog::default();
    let mut rng = RoundDeterminism::with_seed(42);
    let run = |managers: &mut BattleManagers, rng: &mut RoundDeterminism, ops| {
        drain::run_command_group(
            managers,
            &pool,
            &catalog,
            rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            ops,
        )
        .unwrap();
    };
    run(
        &mut managers,
        &mut rng,
        vec![grant(10, 90071, 6), grant(11, 90071, 6)],
    );
    let buff_uid = managers.buff.buff_id_or_type_uid(10, 90071).unwrap();
    run(
        &mut managers,
        &mut rng,
        vec![consume(10, 3), consume(11, 1)],
    );
    assert_eq!(managers.buff.buff_id_amount(10, 90071), 5);
    assert_eq!(managers.buff.buff_id_amount(11, 90071), 5);
    assert_eq!(managers.buff.buff_id_amount(10, 90281), 1);
    assert_eq!(managers.buff.buff_id_amount(10, 90041), 3);
    assert_eq!(managers.buff.buff_id_amount(10, 90201), 1);
    assert_eq!(managers.buff.buff_id_amount(11, 90281), 0);
    assert_eq!(managers.buff.buff_id_amount(11, 90041), 1);
    assert_eq!(managers.buff.buff_id_amount(11, 90201), 0);
    let duplicate = BattleEvent::BuffChanged(crate::engine::event::payload::BuffChangeEvent {
        source_uid: 10,
        target_uid: 10,
        buff_uid,
        buff_id: 90071,
        before_amount: 6,
        after_amount: 3,
        act_id: 0,
        act_value: 0,
        duration_expired: false,
    });
    assert!(observe(&mut managers, &pool, &duplicate, 1, &mut rng).is_empty());
    run(&mut managers, &mut rng, vec![consume(10, 2)]);
    assert_eq!(
        managers.buff.buff_id_amount(10, 90071),
        5,
        "regain must not itself recurse"
    );
    assert!(managers.buff.buff_id_amount(10, 90281) >= 2);
    assert!(managers.buff.buff_id_amount(10, 90041) >= 5);
    assert_eq!(managers.buff.buff_id_amount(10, 90201), 2);
    let permanent = [90001, 90011, 90041, 90021, 90031, 90281];
    assert_eq!(
        permanent
            .iter()
            .map(|id| managers.buff.buff_id_amount(10, *id))
            .sum::<i32>(),
        8
    );
}

fn omega_fight() -> Fight {
    let actor = |uid, team, position, attack| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(1001),
        team_type: Some(team),
        position: Some(position),
        current_hp: Some(1_000),
        attr: Some(HeroAttribute {
            hp: Some(1_000),
            attack: Some(attack),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![actor(10, 1, 1, 100), actor(11, 1, 2, 250)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![actor(-1, 2, 1, 100), actor(-2, 2, 2, 100)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    }
}

fn omega_setup(
    items: Vec<Artefact>,
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    crate::test_support::init_config();
    let fight = omega_fight();
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: items,
            ..Default::default()
        },
    );
    let mut catalog = SkillEffectCatalog::default();
    crate::catalog::BattleCatalog::new(crate::test_support::game_data()).extend_skill_roots(
        &mut catalog,
        [811080592],
        [],
    );
    (managers, pool, catalog, RoundDeterminism::with_seed(42))
}

fn omega() -> Artefact {
    Artefact {
        uid: 100,
        item_id: 8310815,
        ..Default::default()
    }
}

fn execute_for_round(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    round: i32,
    ops: Vec<RuleOp>,
) {
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
    .unwrap();
}

fn settle(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    round: i32,
) {
    crate::engine::runtime::schedule::run_attacker_round_end(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: round,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn omega_uses_team_gains_and_the_highest_living_attack_at_actual_round_end() {
    // One actor providing the entire five layers and two actors contributing
    // three plus two both satisfy the configured team-wide threshold.
    for (first, second) in [(5, 0), (3, 2)] {
        let (mut managers, pool, catalog, mut rng) = omega_setup(vec![omega()]);
        let mut ops = vec![grant(10, 90071, first)];
        if second > 0 {
            ops.push(grant(11, 90071, second));
        }
        execute_for_round(&mut managers, &pool, &catalog, &mut rng, 1, ops);
        settle(&mut managers, &pool, &catalog, &mut rng, 1);
        assert_eq!(managers.hp.current(-1), 750);
        assert_eq!(
            managers.hp.current(-2),
            750,
            "the configured Genesis skill hits all enemies"
        );
        settle(&mut managers, &pool, &catalog, &mut rng, 1);
        assert_eq!(
            managers.hp.current(-1),
            750,
            "repeating settlement must not fire twice"
        );
    }
}

#[test]
fn omega_counts_only_real_post_cap_gain_and_deduplicates_notifications() {
    let (mut managers, pool, catalog, mut rng) = omega_setup(vec![omega()]);
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![grant(10, 90071, 28), grant(11, 90071, 29)],
    );
    let uid = managers.buff.buff_id_or_type_uid(10, 90071).unwrap();
    let duplicate = BattleEvent::BuffAdded(crate::engine::event::payload::BuffChangeEvent {
        source_uid: 10,
        target_uid: 10,
        buff_uid: uid,
        buff_id: 90071,
        before_amount: 0,
        after_amount: 28,
        act_id: 0,
        act_value: 0,
        duration_expired: false,
    });
    assert!(observe(&mut managers, &pool, &duplicate, 1, &mut rng).is_empty());
    assert_eq!(
        managers.rouge.active.as_ref().unwrap().electric_round_gains[&(1, 10)],
        28
    );
    settle(&mut managers, &pool, &catalog, &mut rng, 1);
    assert_eq!(managers.hp.current(-1), 750);
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        2,
        vec![grant(10, 90071, 5), grant(11, 90071, 5)],
    );
    assert_eq!(managers.buff.buff_id_amount(10, 90071), 30);
    assert_eq!(managers.buff.buff_id_amount(11, 90071), 30);
    let gains = &managers.rouge.active.as_ref().unwrap().electric_round_gains;
    assert_eq!(gains[&(2, 10)], 2);
    assert_eq!(gains[&(2, 11)], 1);
    settle(&mut managers, &pool, &catalog, &mut rng, 2);
    assert_eq!(
        managers.hp.current(-1),
        750,
        "three applied layers are below the threshold"
    );
}

#[test]
fn omega_ignores_dead_contributors_and_assigns_enemy_phase_gains_to_next_settlement() {
    use crate::engine::manager::hp::{HpCommand, HpLoss};
    let (mut managers, pool, catalog, mut rng) = omega_setup(vec![omega()]);
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![
            grant(10, 90071, 5),
            RuleOp::Command(BattleCommand::Hp(HpCommand::Lose(HpLoss {
                origin: ORIGIN,
                source_uid: 10,
                target_uid: 10,
                amount: 1_000,
                config_effect: 0,
                hurt: None,
            }))),
        ],
    );
    assert_eq!(managers.hp.current(10), 0);
    settle(&mut managers, &pool, &catalog, &mut rng, 1);
    assert_eq!(
        managers.hp.current(-1),
        1_000,
        "only living actors contribute at settlement"
    );
    // Attacker settlement precedes the enemy phase in the runtime. A gain
    // committed after it belongs to the next attacker's settlement.
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![grant(11, 90071, 2)],
    );
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        2,
        vec![grant(11, 90071, 3)],
    );
    settle(&mut managers, &pool, &catalog, &mut rng, 2);
    assert_eq!(managers.hp.current(-1), 750);
}

#[test]
fn omega_uses_effective_attack_and_multiple_copies_are_independent() {
    let mut second = omega();
    second.uid = 101;
    let (mut managers, pool, catalog, mut rng) = omega_setup(vec![omega(), second]);
    managers.attribute.add(10, AttrId::Attack, 2_000); // 100 * (1 + 2) = 300
    execute_for_round(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        1,
        vec![grant(11, 90071, 5)],
    );
    settle(&mut managers, &pool, &catalog, &mut rng, 1);
    assert_eq!(
        managers.hp.current(-1),
        400,
        "two copies use the highest effective 300 attack"
    );
    assert_eq!(managers.hp.current(-2), 400);
}

#[test]
fn omega_is_disabled_for_ordinary_battles_and_replays_identically() {
    let run =
        |enabled: bool| {
            crate::test_support::init_config();
            let fight = omega_fight();
            let pool = TargetPool::from_fight(&fight);
            let mut managers = BattleManagers::seeded(&fight);
            if enabled {
                super::super::configure(
                    &mut managers,
                    RougeContext {
                        artefacts: vec![omega()],
                        ..Default::default()
                    },
                );
            }
            let mut catalog = SkillEffectCatalog::default();
            crate::catalog::BattleCatalog::new(crate::test_support::game_data())
                .extend_skill_roots(&mut catalog, [811080592], []);
            let mut rng = RoundDeterminism::with_seed(42);
            execute_for_round(
                &mut managers,
                &pool,
                &catalog,
                &mut rng,
                1,
                vec![grant(10, 90071, 3), grant(11, 90071, 2)],
            );
            let result = crate::engine::runtime::schedule::run_attacker_round_end(
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
            (
                managers.hp.current(-1),
                managers.hp.current(-2),
                format!("{:?}", result.frames),
            )
        };
    let ordinary = run(false);
    assert_eq!((ordinary.0, ordinary.1), (1_000, 1_000));
    let one = run(true);
    assert_eq!((one.0, one.1), (750, 750));
    assert_eq!(one, run(true));
}
