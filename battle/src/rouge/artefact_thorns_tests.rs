use super::*;
use crate::engine::{
    runtime::{determinism::RoundDeterminism, drain},
    skill::{
        action::{ActionEvent, SkillExecutionMode, SkillLifecycle},
        effect::SkillEffectCatalog,
        target::TargetContext,
    },
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn setup(
    items: &[i32],
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
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
                .map(|(index, item_id)| Artefact {
                    uid: index as i64 + 1,
                    item_id: *item_id,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
    (
        managers,
        TargetPool::from_fight(&fight),
        SkillEffectCatalog::default(),
        RoundDeterminism::with_seed(42),
    )
}

fn run(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    random: &mut RoundDeterminism,
    ops: Vec<RuleOp>,
) {
    drain::run_command_group(
        managers,
        pool,
        catalog,
        random,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        ops,
    )
    .unwrap();
}

fn complete(source_uid: i64, mode: SkillExecutionMode) -> RuleOp {
    RuleOp::SkillLifecycle(SkillLifecycle::ActionCompleted(ActionEvent {
        source_uid,
        skill_id: 30030111,
        card_index: 1,
        rank: 1,
        mode,
        ..Default::default()
    }))
}

#[test]
fn all_five_families_resolve_on_the_real_action_event_without_consuming_the_buff() {
    for (buff, expected_loss) in [
        (90231, 10), // Pollution: 20% of the source's attack.
        (90151, 15), // Poison: snapshotted 30% of the source's attack.
        (90221, 50), // Corrode: 10% of current HP, subject to its configured cap.
        (90161, 50), // Bleed: 100% of the source's attack.
        (90061, 10), // Gash: 20% of the owner's attack.
    ] {
        let (mut managers, pool, catalog, mut random) = setup(&[8330311]);
        let opening_ops = opening(&mut managers, &pool);
        run(&mut managers, &pool, &catalog, &mut random, opening_ops);
        run(
            &mut managers,
            &pool,
            &catalog,
            &mut random,
            vec![grant(-1, 10, buff)],
        );
        assert!(managers.buff.has_buff_id(10, buff));
        run(
            &mut managers,
            &pool,
            &catalog,
            &mut random,
            vec![complete(10, SkillExecutionMode::Active)],
        );
        assert_eq!(managers.hp.current(10), 1000 - expected_loss, "buff {buff}");
        assert!(
            managers.buff.has_buff_id(10, buff),
            "buff {buff} was consumed"
        );
        assert_eq!(managers.hp.current(11), 1000);
        assert_eq!(managers.hp.current(-1), 1000);
    }
}

#[test]
fn action_requires_the_placed_artefact_and_ignores_unrelated_or_enemy_statuses() {
    let (mut managers, pool, catalog, mut random) = setup(&[]);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![grant(-1, 10, 90151)],
    );
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![complete(10, SkillExecutionMode::Active)],
    );
    assert_eq!(managers.hp.current(10), 1000);

    let (mut managers, pool, catalog, mut random) = setup(&[8330311, 8330311]);
    let opening_ops = opening(&mut managers, &pool);
    run(&mut managers, &pool, &catalog, &mut random, opening_ops);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![
            grant(-1, 10, 90151),
            grant(-1, 10, 30790106),
            grant(10, -1, 90151),
        ],
    );
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![complete(-1, SkillExecutionMode::Active)],
    );
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![complete(10, SkillExecutionMode::Nested)],
    );
    assert_eq!(
        (managers.hp.current(10), managers.hp.current(-1)),
        (1000, 1000)
    );
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![complete(10, SkillExecutionMode::Active)],
    );
    assert_eq!(
        managers.hp.current(10),
        985,
        "duplicate artefacts resolve only once"
    );
    assert_eq!(managers.hp.current(-1), 1000);
}

#[test]
fn real_active_skill_produces_the_completed_action_and_early_poison_tick() {
    use crate::engine::skill::{
        action::{SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
        effect::ParsedSkillEffect,
    };

    let (mut managers, pool, mut catalog, mut random) = setup(&[8330311]);
    let opening_ops = opening(&mut managers, &pool);
    run(&mut managers, &pool, &catalog, &mut random, opening_ops);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut random,
        vec![grant(-1, 10, 90151)],
    );
    catalog.insert(ParsedSkillEffect {
        skill_id: 30030111,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030111, 1_000);
    catalog.insert_logic_target(30030111, 1);
    catalog.insert_effect_tag(30030111, 1);
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: 10,
        skill_id: 30030111,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
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
    assert_eq!(managers.hp.current(10), 985);
    assert!(managers.buff.has_buff_id(10, 90151));
}
