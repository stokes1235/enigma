use super::*;
use crate::engine::{
    manager::hp::{DamageEffectKind, HpCommand, HpDamage, HurtDamageFromType, HurtInfoData},
    runtime::{determinism::RoundDeterminism, drain, schedule},
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn fight() -> Fight {
    let entity = |uid, team, current_hp, max_hp, attack| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(current_hp),
        attr: Some(HeroAttribute {
            hp: Some(max_hp),
            attack: Some(attack),
            ..Default::default()
        }),
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![
                entity(10, 1, 1_000, 1_000, 100),
                entity(11, 1, 2_000, 2_000, 300),
                entity(12, 1, 0, 1_500, 500),
            ],
            sub_entitys: vec![entity(13, 1, 1_000, 1_000, 100)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 100, 100, 10), entity(-2, 2, 100, 100, 10)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    }
}

fn context(ids: &[i32], final_battle: bool) -> RougeContext {
    RougeContext {
        final_battle,
        artefacts: ids
            .iter()
            .enumerate()
            .map(|(index, id)| Artefact {
                uid: index as i64 + 100,
                item_id: *id,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn setup(
    context: Option<RougeContext>,
) -> (Fight, BattleManagers, TargetPool, SkillEffectCatalog, RoundDeterminism) {
    crate::test_support::init_config();
    let fight = fight();
    let mut managers = BattleManagers::seeded(&fight);
    if let Some(context) = context {
        super::super::configure(&mut managers, context);
    }
    (
        fight.clone(),
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

fn damage(source: i64, target: i64, amount: i32) -> RuleOp {
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
            damage_from: HurtDamageFromType::Skill,
            buff_act_id: 0,
            buff_uid: 0,
            hurt_effect_type: 0,
            display_amount: None,
        },
    })))
}

#[test]
fn conch_uses_real_attack_and_one_round_shields_on_living_deployed_allies() {
    let (_, mut managers, pool, catalog, mut rng) = setup(Some(context(&[8180501], false)));
    managers.attribute.add(10, AttrId::Attack, 500); // 100 base + 50%
    managers.hp.set_shield(10, 300);
    let ops = super::super::round_start(&mut managers, &pool, 1, &mut rng);
    run(&mut managers, &pool, &catalog, &mut rng, 1, ops);
    assert_eq!(managers.hp.shield(10), 600);
    assert_eq!(managers.hp.shield(11), 600);
    assert_eq!(managers.hp.shield(12), 0);
    assert_eq!(managers.hp.shield(13), 0);
    assert!(managers.buff.has_buff_id(10, 81805010));
    assert!(super::round_start(&mut managers, &pool, 1).is_empty());

    // The one-round carrier expires through the ordinary round transition;
    // its next trigger creates fresh shields instead of indefinitely stacking.
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
    assert_eq!(managers.hp.shield(10), 300);
    assert_eq!(managers.hp.shield(11), 600);
}

#[test]
fn echo_action_points_only_apply_in_the_final_battle_and_survive_context_replay() {
    let original = context(&[8180502, 8330417], true);
    let encoded = serde_json::to_string(&original).unwrap();
    let restored: RougeContext = serde_json::from_str(&encoded).unwrap();
    let (fight, managers, pool, catalog, mut rng) = setup(Some(restored));
    let ap = |managers: &BattleManagers, rng: &mut RoundDeterminism, round| {
        crate::engine::round::state::next_action_points(
            &fight,
            &pool,
            managers,
            &catalog,
            rng,
            TargetContext {
                current_round: round,
                ..Default::default()
            },
        )
    };
    assert_eq!(ap(&managers, &mut rng, 1), 3); // two living actors + one AP
    assert_eq!(ap(&managers, &mut rng, 2), 3); // persistent in subsequent rounds
    let (_, ordinary, _, _, _) = setup(None);
    assert_eq!(ap(&ordinary, &mut rng, 1), 2);
    let (_, non_final, _, _, _) = setup(Some(context(&[8180502], false)));
    assert_eq!(ap(&non_final, &mut rng, 1), 2);
    let mut danger = context(&[8180502, 8330417], false);
    danger.danger_or_crisis = true;
    let (_, danger, _, _, _) = setup(Some(danger));
    assert_eq!(ap(&danger, &mut rng, 1), 3);
    let legacy: RougeContext = serde_json::from_str("{}").unwrap();
    assert!(!legacy.final_battle);
}

#[test]
fn greetings_uses_committed_enemy_deaths_and_each_living_allies_max_hp() {
    let (_, mut managers, pool, catalog, mut rng) = setup(Some(context(&[8180503], true)));
    managers.hp.set_shield(10, 50);
    run(&mut managers, &pool, &catalog, &mut rng, 1, vec![damage(10, -1, 100)]);
    assert_eq!(managers.hp.shield(10), 250);
    assert_eq!(managers.hp.shield(11), 400);
    assert_eq!(managers.hp.shield(12), 0);
    assert_eq!(managers.hp.shield(13), 0);
    assert!(managers.buff.has_buff_id(10, 81805030));
    run(&mut managers, &pool, &catalog, &mut rng, 1, vec![damage(10, -1, 100)]);
    assert_eq!(managers.hp.shield(10), 250, "an already dead target has no second death");
    run(&mut managers, &pool, &catalog, &mut rng, 1, vec![damage(11, -2, 100)]);
    assert_eq!(managers.hp.shield(10), 450);
    assert_eq!(managers.hp.shield(11), 800);
}

#[test]
fn greetings_ignores_nonfinal_and_enemy_attributed_deaths() {
    for case in [None, Some(context(&[8180503], false)), Some(context(&[], true))] {
        let (_, mut managers, pool, catalog, mut rng) = setup(case);
        run(&mut managers, &pool, &catalog, &mut rng, 1, vec![damage(10, -1, 100)]);
        assert_eq!(managers.hp.shield(10), 0);
        assert_eq!(managers.hp.shield(11), 0);
    }
    let (_, mut managers, pool, catalog, mut rng) = setup(Some(context(&[8180503], true)));
    run(&mut managers, &pool, &catalog, &mut rng, 1, vec![damage(-2, -1, 100)]);
    assert_eq!(managers.hp.shield(10), 0);
    assert_eq!(managers.hp.shield(11), 0);
}
