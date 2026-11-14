use super::*;
use crate::engine::{
    runtime::{determinism::RoundDeterminism, drain},
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn fixture() -> (BattleManagers, TargetPool, RoundDeterminism) {
    crate::test_support::init_config();
    let hero = |uid, hp| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(1),
        current_hp: Some(hp),
        attr: Some(HeroAttribute {
            hp: Some(1000),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![hero(10, 500), hero(11, 0)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                team_type: Some(2),
                current_hp: Some(1000),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    let mut context = RougeContext {
        catalyst: Some(Catalyst {
            style_id: 7,
            coin: 100,
            power: 0,
            power_limit: 100,
            ..Default::default()
        }),
        ..Default::default()
    };
    for (uid, item_id) in [(1, 8310608), (2, 8320617)] {
        context.artefacts.push(Artefact {
            uid,
            item_id,
            ..Default::default()
        });
        context.growth_sources.push(GrowthSource {
            uid,
            item_id,
            actor_uids: vec![10, 11],
            ..Default::default()
        });
        context
            .permanent_growth
            .insert(uid, BTreeMap::from([(KEY.to_owned(), 98)]));
    }
    super::super::configure(&mut managers, context);
    (
        managers,
        TargetPool::from_fight(&fight),
        RoundDeterminism::with_seed(42),
    )
}

#[test]
fn actual_spending_tracks_remainders_cap_income_and_dead_hp_without_revival() {
    let run = || {
        let (mut managers, pool, mut rng) = fixture();
        let catalog = SkillEffectCatalog::default();
        let mut change = |managers: &mut BattleManagers, coin, power| {
            let ops = super::super::catalyst::try_change_resources(managers, coin, power).unwrap();
            drain::run_command_group(
                managers,
                &pool,
                &catalog,
                &mut rng,
                TargetContext {
                    current_round: 1,
                    ..Default::default()
                },
                ops,
            )
            .unwrap();
        };
        change(&mut managers, -6, 0);
        change(&mut managers, 20, 50); // income and power recovery are not spending.
        assert!(managers.rouge.counters().is_empty());
        change(&mut managers, -14, 0); // two whole ten-coin thresholds, 98% -> 100%.
        assert_eq!(managers.rouge.resources().unwrap().spent_coin, 20);
        assert_eq!(managers.attribute.get(10, AttrId::DmgBonus), 20);
        assert_eq!((managers.hp.current(10), managers.hp.max(10)), (520, 1020));
        assert_eq!((managers.hp.current(11), managers.hp.max(11)), (0, 1020));
        assert_eq!(managers.rouge.counters()[&1][KEY], 2);
        assert_eq!(managers.rouge.counters()[&2][KEY], 2);
        change(&mut managers, -10, 0); // capped lifetime growth cannot exceed 100%.
        assert_eq!(managers.rouge.counters()[&1][KEY], 2);
        assert_eq!(managers.hp.max(10), 1020);
        assert!(super::super::catalyst::try_change_resources(&mut managers, -10000, 0).is_none());
        assert_eq!(managers.rouge.resources().unwrap().spent_coin, 30);
        (
            managers.rouge.resources().unwrap(),
            managers.rouge.counters(),
        )
    };
    assert_eq!(run(), run());
}

fn hp_growth_fixture(
    base_hp: i32,
    entry_max_hp: i32,
    sources: &[(i64, Vec<i64>)],
    previous_growth: i32,
) -> (BattleManagers, TargetPool, RoundDeterminism) {
    crate::test_support::init_config();
    let hero = |uid, current_hp| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(1),
        current_hp: Some(current_hp),
        attr: Some(HeroAttribute {
            hp: Some(entry_max_hp),
            ..Default::default()
        }),
        base_attr: Some(HeroAttribute {
            hp: Some(base_hp),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![hero(10, entry_max_hp - 30), hero(11, 0)],
            ..Default::default()
        }),
        defender: Some(FightTeam::default()),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    let mut context = RougeContext {
        catalyst: Some(Catalyst {
            style_id: 1,
            coin: 100,
            power_limit: 100,
            ..Default::default()
        }),
        ..Default::default()
    };
    for (uid, actors) in sources {
        context.artefacts.push(Artefact {
            uid: *uid,
            item_id: 8320617,
            ..Default::default()
        });
        context.growth_sources.push(GrowthSource {
            uid: *uid,
            item_id: 8320617,
            actor_uids: actors.clone(),
            ..Default::default()
        });
        context
            .permanent_growth
            .insert(*uid, BTreeMap::from([(KEY.to_owned(), previous_growth)]));
    }
    super::super::configure(&mut managers, context);
    (
        managers,
        TargetPool::from_fight(&fight),
        RoundDeterminism::with_seed(42),
    )
}

fn spend_coins(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    random: &mut RoundDeterminism,
    amount: i32,
) {
    let ops = super::super::catalyst::try_change_resources(managers, -amount, 0).unwrap();
    drain::run_command_group(
        managers,
        pool,
        &SkillEffectCatalog::default(),
        random,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        ops,
    )
    .unwrap();
}

#[test]
fn hp_growth_rounds_cumulative_actor_totals_and_keeps_other_max_hp_changes() {
    let run = |split: bool| {
        let (mut managers, pool, mut random) =
            hp_growth_fixture(150, 150, &[(1, vec![10, 11]), (2, vec![10])], 0);
        // A separate temporary max-HP increase remains independent of growth.
        managers
            .execute_hp(HpCommand::AdjustMax(MaxHpAdjust {
                origin: ORIGIN,
                source_uid: 10,
                target_uid: 10,
                delta: 40,
            }))
            .unwrap();
        if split {
            spend_coins(&mut managers, &pool, &mut random, 10);
            assert_eq!((managers.hp.current(10), managers.hp.max(10)), (163, 193));
            assert_eq!((managers.hp.current(11), managers.hp.max(11)), (0, 151));
            spend_coins(&mut managers, &pool, &mut random, 10);
        } else {
            spend_coins(&mut managers, &pool, &mut random, 20);
        }
        // Actor 10 receives four points across two sources: floor(150*4%).
        // Actor 11 belongs to one source and must remain defeated.
        assert_eq!((managers.hp.current(10), managers.hp.max(10)), (166, 196));
        assert_eq!((managers.hp.current(11), managers.hp.max(11)), (0, 153));
        (managers.rouge.resources(), managers.rouge.counters())
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn hp_growth_uses_original_base_and_stays_within_one_hp_of_next_fight() {
    // The opening value is floor(1003 * (1000 + 980) / 1000), including
    // 98% previously earned growth. The next battle applies 100% afresh.
    let base = 1003_i32;
    let opening_max = (i64::from(base) * 1980 / 1000) as i32;
    let (mut managers, pool, mut random) =
        hp_growth_fixture(base, opening_max, &[(1, vec![10, 11])], 98);
    spend_coins(&mut managers, &pool, &mut random, 10);
    assert_eq!(managers.hp.max(10), opening_max + 10);
    spend_coins(&mut managers, &pool, &mut random, 10);
    assert_eq!(managers.hp.max(10), opening_max + 20);
    assert_eq!(
        (managers.hp.current(11), managers.hp.max(11)),
        (0, opening_max + 20)
    );

    // combat::apply rounds the initial HP once using the summed percentage;
    // the fight-side growth rounds its increment separately. Without adding
    // a frozen rate, their documented local approximation can differ by 1.
    let next_fight_max = (i64::from(base) * 2000 / 1000) as i32;
    let next_fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                current_hp: Some(next_fight_max),
                attr: Some(HeroAttribute {
                    hp: Some(next_fight_max),
                    ..Default::default()
                }),
                base_attr: Some(HeroAttribute {
                    hp: Some(base),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let next = BattleManagers::seeded(&next_fight);
    assert_eq!(next.hp.max(10), 2006);
    assert_eq!(managers.hp.max(10), 2005);
    assert!((next.hp.max(10) - managers.hp.max(10)).abs() <= 1);
}
