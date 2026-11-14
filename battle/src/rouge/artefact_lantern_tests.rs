use super::*;
use crate::engine::{
    manager::hp::{DamageEffectKind, HpCommand, HpDamage, HurtDamageFromType, HurtInfoData},
    runtime::{determinism::RoundDeterminism, drain},
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn setup(items: &[i32]) -> (BattleManagers, TargetPool, SkillEffectCatalog, RoundDeterminism) {
    crate::test_support::init_config();
    let entity = |uid, team, hp| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(hp),
        attr: Some(HeroAttribute {
            hp: Some(1000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1000), entity(11, 1, 1000), entity(12, 1, 0)],
            sub_entitys: vec![entity(13, 1, 1000)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1000)],
            ..Default::default()
        }),
        version: Some(7),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    let context = RougeContext {
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
    };
    // Reconstruct the initial context the same way as a resumed battle.
    let serialized = serde_json::to_string(&context).unwrap();
    configure(&mut managers, serde_json::from_str(&serialized).unwrap());
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
    rng: &mut RoundDeterminism,
    ops: Vec<RuleOp>,
) {
    drain::run_command_group(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        ops,
    )
    .unwrap();
}

#[test]
fn lantern_blocks_the_four_configured_status_families_on_deployed_allies() {
    let (mut managers, pool, catalog, mut rng) = setup(&[8320514, 8320514]);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        vec![grant(-1, 10, 90151), grant(-1, 10, 4150001)],
    );
    assert!(managers.buff.has_buff_id(10, 90151));
    assert!(managers.buff.has_buff_id(10, 4150001));

    let ops = opening(&mut managers, &pool);
    run(&mut managers, &pool, &catalog, &mut rng, ops);
    for uid in [10, 11] {
        assert!(managers.buff.has_buff_id(uid, 832051401));
    }
    for uid in [12, 13, -1] {
        assert!(!managers.buff.has_buff_id(uid, 832051401));
    }
    assert!(opening(&mut managers, &pool).is_empty());
    assert!(!managers.buff.has_buff_id(10, 90151));
    assert!(!managers.buff.has_buff_id(10, 4150001));

    // Alternative duration IDs exercise exclusion by family, not exact ID.
    for uid in [10, 11] {
        let denied = [90233, 90154, 90223, 4150001];
        let ops = denied.iter().map(|id| grant(-1, uid, *id)).collect();
        run(&mut managers, &pool, &catalog, &mut rng, ops);
        for id in denied {
            assert!(!managers.buff.has_buff_id(uid, id), "{uid} received {id}");
        }
        assert!(managers.buff.has_buff_id(uid, 832051401));
    }
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        vec![grant(-1, 10, 90061), grant(-1, 10, 90161)],
    );
    assert!(managers.buff.has_buff_id(10, 90061));
    assert!(managers.buff.has_buff_id(10, 90161));
}

#[test]
fn no_lantern_means_no_immunity_for_enemy_or_unprotected_allies() {
    let (mut managers, pool, catalog, mut rng) = setup(&[]);
    let ops = opening(&mut managers, &pool);
    run(&mut managers, &pool, &catalog, &mut rng, ops);
    run(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        vec![grant(-1, 10, 90151), grant(10, -1, 90151)],
    );
    assert!(managers.buff.has_buff_id(10, 90151));
    assert!(managers.buff.has_buff_id(-1, 90151));
}

#[test]
fn rose_and_slough_applies_its_configured_genesis_reduction_once() {
    fn received(items: &[i32]) -> i32 {
        let (mut managers, pool, catalog, mut rng) = setup(items);
        let ops = opening(&mut managers, &pool);
        run(&mut managers, &pool, &catalog, &mut rng, ops);
        assert_eq!(
            managers.buff.has_buff_id(10, 833031101),
            !items.is_empty()
        );
        assert!(!managers.buff.has_buff_id(12, 833031101));
        assert!(!managers.buff.has_buff_id(13, 833031101));
        assert_eq!(
            crate::engine::damage::modifiers::genesis_multiplier(&managers, 10, -1),
            1000
        );
        let multiplier =
            crate::engine::damage::modifiers::genesis_multiplier(&managers, -1, 10);
        let amount = crate::engine::damage::calculate(
            crate::engine::damage::DamageFormulaInput::genesis(100, 1000, multiplier),
        );
        run(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            vec![RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(HpDamage {
                origin: ORIGIN,
                source_uid: -1,
                target_uid: 10,
                amount,
                config_effect: 1,
                effect_kind: DamageEffectKind::Genesis,
                assassinate: false,
                ignore_riposte: false,
                hurt: HurtInfoData {
                    from_uid: -1,
                    is_crit: false,
                    career_restraint: false,
                    reduce_hp: amount,
                    effect_id: 1,
                    skill_id: 30030111,
                    damage_from: HurtDamageFromType::SkillEffect,
                    buff_act_id: 0,
                    buff_uid: 0,
                    hurt_effect_type: 0,
                    display_amount: None,
                },
            })))],
        );
        1000 - managers.hp.current(10)
    }

    assert_eq!(received(&[]), 100);
    assert_eq!(received(&[8330311]), 50);
    assert_eq!(received(&[8330311, 8330311]), 50);
}
