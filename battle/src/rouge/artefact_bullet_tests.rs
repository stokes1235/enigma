use super::*;
use crate::engine::{
    event::payload::BuffChangeEvent,
    manager::buff::{BuffConsume, BuffSelector, DepletedBuff},
    runtime::drain,
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn setup(copies: usize) -> (BattleManagers, TargetPool) {
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
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    configure(
        &mut managers,
        RougeContext {
            artefacts: (0..copies)
                .map(|i| Artefact {
                    uid: i as i64 + 100,
                    item_id: 8210713,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
    (managers, TargetPool::from_fight(&fight))
}

fn settle(managers: &mut BattleManagers, pool: &TargetPool, ops: Vec<RuleOp>) {
    drain::run_command_group(
        managers,
        pool,
        &SkillEffectCatalog::default(),
        &mut RoundDeterminism::with_seed(42),
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        ops,
    )
    .unwrap();
}

fn change(
    buff_id: i32,
    target_uid: i64,
    buff_uid: i64,
    before: i32,
    after: i32,
) -> BuffChangeEvent {
    BuffChangeEvent {
        source_uid: target_uid,
        target_uid,
        buff_uid,
        buff_id,
        before_amount: before,
        after_amount: after,
        act_id: 0,
        act_value: 0,
        duration_expired: false,
    }
}

#[test]
fn flintlock_uses_real_bullet_marker_and_deduplicates_loss_transitions() {
    let (mut managers, pool) = setup(1);
    let mut rng = RoundDeterminism::with_seed(42);
    let added = change(91130032, 10, 123, 0, 2);
    assert!(
        observe(
            &mut managers,
            &pool,
            &BattleEvent::BuffAdded(added),
            &mut rng
        )
        .is_empty()
    );
    let decreased = change(91130032, 10, 123, 2, 0);
    let first = observe(
        &mut managers,
        &pool,
        &BattleEvent::BuffChanged(decreased),
        &mut rng,
    );
    assert_eq!(
        first.len(),
        2,
        "one type transition draws one bullet and grants Preignition"
    );
    assert!(
        observe(
            &mut managers,
            &pool,
            &BattleEvent::BuffRemoved(decreased),
            &mut rng
        )
        .is_empty()
    );
    settle(&mut managers, &pool, first);
    assert_eq!(managers.buff.buff_id_amount(10, 4150002), 10);
    assert_eq!(
        BULLETS
            .iter()
            .map(|id| managers.buff.buff_id_amount(10, *id))
            .sum::<i32>(),
        1
    );
    assert_eq!(managers.buff.buff_id_amount(11, 4150002), 0);
}

#[test]
fn flintlock_ignores_non_bullets_enemy_losses_and_gains() {
    let (mut managers, pool) = setup(1);
    let mut rng = RoundDeterminism::with_seed(42);
    for event in [
        BattleEvent::BuffChanged(change(90071, 10, 100, 2, 1)),
        BattleEvent::BuffChanged(change(91130031, -1, 101, 1, 0)),
        BattleEvent::BuffAdded(change(91130031, 11, 102, 0, 1)),
        BattleEvent::BuffChanged(change(91130031, 11, 102, 1, 2)),
    ] {
        assert!(observe(&mut managers, &pool, &event, &mut rng).is_empty());
    }
}

#[test]
fn flintlock_replays_random_draws_and_stacks_independent_copies() {
    let run = || {
        let (mut managers, pool) = setup(2);
        let mut rng = RoundDeterminism::with_seed(73);
        let ops = observe(
            &mut managers,
            &pool,
            &BattleEvent::BuffRemoved(change(90501, 10, 77, 1, 0)),
            &mut rng,
        );
        assert_eq!(ops.len(), 4);
        settle(&mut managers, &pool, ops.clone());
        assert_eq!(managers.buff.buff_id_amount(10, 4150002), 20);
        ops
    };
    assert_eq!(run(), run());
}

#[test]
fn actual_buff_consume_triggers_flintlock_without_recursive_rewards() {
    let (mut managers, pool) = setup(1);
    settle(&mut managers, &pool, vec![grant(10, 10, 91130032)]);
    let bullet_uid = managers
        .buff
        .active_for(10)
        .find(|buff| buff.buff_id == Some(91130032))
        .and_then(|buff| buff.uid)
        .unwrap();
    settle(
        &mut managers,
        &pool,
        vec![RuleOp::Command(BattleCommand::Buff(
            BuffCommand::ConsumeCoalesced(BuffConsume {
                origin: ORIGIN,
                target_uid: 10,
                selector: BuffSelector::ExactId(91130032),
                amount: 1,
                depleted: DepletedBuff::Remove,
            }),
        ))],
    );
    assert_eq!(managers.buff.buff_id_amount(10, 4150002), 10);
    assert_eq!(
        BULLETS
            .iter()
            .map(|id| managers.buff.buff_id_amount(10, *id))
            .sum::<i32>(),
        1
    );
    assert!(
        managers
            .rouge
            .active
            .as_ref()
            .unwrap()
            .bullet_buff_amounts
            .contains_key(&bullet_uid)
    );
}
