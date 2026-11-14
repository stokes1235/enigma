use super::super::{Artefact, AttackPredicate, ConditionalModifier, RougeContext, configure};
use super::*;
use crate::engine::{
    manager::BattleManagers,
    runtime::{determinism::RoundDeterminism, drain},
    skill::{
        action::{SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
        effect::{ParsedSkillEffect, SkillEffectCatalog},
        target::{TargetContext, TargetPool},
    },
};
use sonettobuf::{BuffInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn fowling(uid: i64, count2: i32, telescopes: i32) -> Artefact {
    Artefact {
        uid,
        item_id: 8210711,
        count2,
        extra_action_chance_doublers: telescopes,
        ..Default::default()
    }
}

fn fixture(
    items: Vec<Artefact>,
    bullet_uid: Option<i64>,
    enable_rouge: bool,
    area: bool,
) -> (BattleManagers, TargetPool, SkillEffectCatalog) {
    crate::test_support::init_config();
    let actor = |uid, team| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(10_000),
        attr: Some(HeroAttribute {
            hp: Some(10_000),
            attack: Some(100),
            ..Default::default()
        }),
        buffs: if bullet_uid == Some(uid) {
            vec![BuffInfo {
                uid: Some(900),
                buff_id: Some(90501),
                from_uid: Some(uid),
                layer: Some(1),
                ..Default::default()
            }]
        } else {
            vec![]
        },
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![actor(10, 1), actor(11, 1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![actor(-1, 2), actor(-2, 2)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    if enable_rouge {
        let conditional_modifiers = items
            .iter()
            .map(|item| ConditionalModifier {
                source_uid: item.uid,
                item_id: item.item_id,
                predicate: AttackPredicate::HasBullet,
                attr_id: 205,
                value: 600,
            })
            .collect();
        configure(
            &mut managers,
            RougeContext {
                artefacts: items,
                conditional_modifiers,
                ..Default::default()
            },
        );
    }
    let mut catalog = SkillEffectCatalog::default();
    catalog.insert(ParsedSkillEffect {
        skill_id: 30030111,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030111, 1000);
    catalog.insert_logic_target(30030111, if area { 202 } else { 1 });
    catalog.insert_effect_tag(30030111, 1);
    (managers, pool, catalog)
}

fn cast(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    random: &mut RoundDeterminism,
    source_uid: i64,
    extra: bool,
    area: bool,
) -> (i32, i32) {
    let target = if source_uid > 0 { -1 } else { 10 };
    let before = managers.hp.current(target);
    let other_before = managers.hp.current(-2);
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid,
        skill_id: 30030111,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.card_index = if extra { 0 } else { 1 };
    invocation.target = if area {
        SkillTarget::Configured
    } else {
        SkillTarget::Explicit(target)
    };
    drain::run_skill(
        managers,
        pool,
        catalog,
        random,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        invocation,
        SkillModifiers::default(),
    )
    .unwrap();
    assert_eq!(
        managers.attribute.get(source_uid, AttrId::DmgBonus),
        0,
        "a one-action proc must not leak into the actor's permanent attributes"
    );
    (
        before - managers.hp.current(target),
        other_before - managers.hp.current(-2),
    )
}

fn run_one(item: Artefact, bullet: Option<i64>, extra: bool, roll: i32) -> i32 {
    let (mut managers, pool, catalog) = fixture(vec![item], bullet, true, false);
    let mut rng = RoundDeterminism::with_seed(42);
    rng.enqueue_permille_rolls([roll]);
    cast(&mut managers, &pool, &catalog, &mut rng, 10, extra, false).0
}

#[test]
fn independent_bullet_and_extra_action_damage_use_the_actual_cast() {
    assert_eq!(run_one(fowling(1, 0, 0), None, true, 0), 160);
    assert_eq!(run_one(fowling(1, 0, 0), None, true, 500), 100);
    assert_eq!(run_one(fowling(1, 0, 0), Some(10), true, 0), 220);
    assert_eq!(run_one(fowling(1, 0, 0), Some(10), true, 500), 160);
    assert_eq!(
        run_one(fowling(1, 0, 0), Some(11), true, 0),
        160,
        "a different ally's Bullet must not supply the cast owner's fixed bonus"
    );
    assert_eq!(run_one(fowling(1, 0, 0), None, false, 0), 100);
    assert_eq!(run_one(fowling(1, 0, 0), Some(10), false, 0), 160);
}

#[test]
fn copies_roll_independently_in_uid_order_and_telescope_only_changes_their_chance() {
    let items = vec![fowling(2, 50, 0), fowling(1, 0, 0)];
    let (mut managers, pool, catalog) = fixture(items, Some(10), true, false);
    let mut rng = RoundDeterminism::with_seed(42);
    rng.enqueue_permille_rolls([600, 100]);
    assert_eq!(
        cast(&mut managers, &pool, &catalog, &mut rng, 10, true, false).0,
        280,
        "two held-Bullet bonuses plus the successful second UID proc"
    );
    assert_eq!(run_one(fowling(1, 0, 1), None, true, 999), 160);
    assert_eq!(run_one(fowling(1, 0, 0), None, true, 999), 100);
    assert_eq!(run_one(fowling(1, 20, 0), None, true, 699), 160);
    assert_eq!(run_one(fowling(1, 20, 0), None, true, 700), 100);
}

#[test]
fn one_roll_is_shared_by_every_target_of_the_action_and_replays() {
    let run = || {
        let (mut managers, pool, catalog) = fixture(vec![fowling(1, 0, 0)], None, true, true);
        let mut rng = RoundDeterminism::with_seed(42);
        rng.enqueue_permille_rolls([0, 999]);
        let first = cast(&mut managers, &pool, &catalog, &mut rng, 10, true, true);
        let second = cast(&mut managers, &pool, &catalog, &mut rng, 10, true, true);
        (first, second)
    };
    let one = run();
    assert_eq!(one.0, (160, 160));
    assert_eq!(one.1, (100, 100));
    assert_eq!(one, run());
}

#[test]
fn ordinary_actions_do_not_roll_and_enemy_or_non_rouge_attacks_do_not_benefit() {
    let (mut managers, pool, catalog) = fixture(vec![fowling(1, 0, 0)], None, true, false);
    let mut rng = RoundDeterminism::with_seed(42);
    rng.enqueue_permille_rolls([0, 999]);
    assert_eq!(
        cast(&mut managers, &pool, &catalog, &mut rng, 10, false, false).0,
        100
    );
    assert_eq!(
        cast(&mut managers, &pool, &catalog, &mut rng, 10, true, false).0,
        160
    );
    assert_eq!(
        cast(&mut managers, &pool, &catalog, &mut rng, -1, true, false).0,
        100
    );
    let (mut plain, pool, catalog) = fixture(vec![fowling(1, 0, 0)], Some(10), false, false);
    let mut rng = RoundDeterminism::with_seed(42);
    rng.enqueue_permille_rolls([0]);
    assert_eq!(
        cast(&mut plain, &pool, &catalog, &mut rng, 10, true, false).0,
        100
    );
}
