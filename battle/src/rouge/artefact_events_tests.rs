use super::*;
use crate::engine::{
    manager::hp::{DamageEffectKind, HpCommand, HpDamage, HurtDamageFromType, HurtInfoData},
    runtime::{determinism::RoundDeterminism, drain},
    skill::{effect::SkillEffectCatalog, target::TargetContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

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
fn real_hits_and_death_award_their_recipient_only_once() {
    crate::test_support::init_config();
    let entity = |uid, team| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(1000),
        ex_point: Some(0),
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
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: vec![
                Artefact {
                    uid: 1,
                    item_id: 8120208,
                    count2: 2,
                    ..Default::default()
                },
                Artefact {
                    uid: 2,
                    item_id: 8120317,
                    ..Default::default()
                },
                Artefact {
                    uid: 3,
                    item_id: 8230037,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    let catalog = SkillEffectCatalog::default();
    let mut rng = RoundDeterminism::with_seed(77);
    let mut run = |managers: &mut BattleManagers, op| {
        drain::run_command_group(
            managers,
            &pool,
            &catalog,
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            vec![op],
        )
        .unwrap();
    };
    run(&mut managers, damage(-1, 10, 10));
    assert_eq!(managers.buff.buff_id_amount(10, 90091), 7);
    assert_eq!(managers.buff.buff_id_amount(10, 90301), 3);
    assert_eq!(managers.buff.buff_id_amount(11, 90091), 0);
    let before = managers.ex_point.get(10);
    run(&mut managers, damage(10, -1, 2000));
    assert_eq!(managers.ex_point.get(10), before + 1);
    run(&mut managers, damage(10, -1, 2000));
    assert_eq!(managers.ex_point.get(10), before + 1);
}

fn attack_fixture(
    item_id: i32,
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
        current_hp: Some(10_000),
        attr: Some(HeroAttribute {
            hp: Some(10_000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2), entity(-2, 2)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: vec![Artefact {
                uid: 1,
                item_id,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let mut catalog = SkillEffectCatalog::default();
    catalog.insert(crate::engine::skill::effect::ParsedSkillEffect {
        skill_id: 30030111,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030111, 1000);
    catalog.insert_logic_target(30030111, 1);
    catalog.insert_effect_tag(30030111, 1);
    (managers, pool, catalog, RoundDeterminism::with_seed(77))
}

fn attack(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    target_uid: i64,
) {
    attack_at_round(managers, pool, catalog, rng, target_uid, 1);
}

fn attack_at_round(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    target_uid: i64,
    round: i32,
) {
    attack_skill(managers, pool, catalog, rng, 30030111, target_uid, round);
}

fn attack_skill(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    skill_id: i32,
    target_uid: i64,
    round: i32,
) {
    use crate::engine::skill::action::{
        SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget,
    };
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: 10,
        skill_id,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.target = SkillTarget::Explicit(target_uid);
    drain::run_skill(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: round,
            ..Default::default()
        },
        invocation,
        SkillModifiers::default(),
    )
    .unwrap();
}

#[test]
fn bone_arrow_single_attack_hits_one_random_living_enemy_once_and_replays() {
    fn run() -> (i32, i32) {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8110029);
        let before = [managers.hp.current(-1), managers.hp.current(-2)];
        attack(&mut managers, &pool, &catalog, &mut rng, -1);
        (
            before[0] - managers.hp.current(-1),
            before[1] - managers.hp.current(-2),
        )
    }
    let damage = run();
    assert!(damage == (200, 0) || damage == (100, 100));
    assert_eq!(
        damage.0 + damage.1,
        200,
        "extra genesis cannot recursively trigger itself"
    );
    assert_eq!(run(), damage);
}

#[test]
fn bone_arrow_does_not_trigger_for_real_two_target_attack() {
    use crate::engine::runtime::determinism::SkillTargetChoice;
    let (mut managers, pool, mut catalog, mut rng) = attack_fixture(8110029);
    catalog.insert_logic_target(30030111, 202);
    rng.enqueue_skill_target_choices([SkillTargetChoice {
        skill_id: 30030111,
        source_uid: 10,
        target_code: 202,
        targets: vec![-1, -2],
        additional_targets: vec![],
        crit_targets: vec![],
        additional_crit_targets: vec![],
    }]);
    let before = [managers.hp.current(-1), managers.hp.current(-2)];
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert_eq!(before[0] - managers.hp.current(-1), 100);
    assert_eq!(before[1] - managers.hp.current(-2), 100);
}

#[test]
fn bone_arrow_can_finish_a_battle_and_records_clipped_damage_and_kills() {
    use crate::engine::round::outcome::BattleOutcome;
    use crate::engine::skill::action::{
        SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget,
    };
    fn run() -> (BattleSummary, Option<BattleOutcome>, Vec<String>) {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8110029);
        // The main attack defeats -1; only -2 survives for the follow-up.
        managers.hp.resync_current(-1, 100);
        managers.hp.resync_current(-2, 50);
        let mut invocation: SkillInvocation = SkillRequest {
            source_uid: 10,
            skill_id: 30030111,
        }
        .into();
        invocation.mode = SkillExecutionMode::Active;
        invocation.target = SkillTarget::Explicit(-1);
        let result = drain::run_skill(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            TargetContext {
                current_round: 1,
                battle_id: i32::MAX,
                ..Default::default()
            },
            invocation,
            SkillModifiers::default(),
        )
        .unwrap();
        assert_eq!(managers.hp.current(-1), 0);
        assert_eq!(managers.hp.current(-2), 0);
        let summary = managers.rouge.summary().unwrap();
        assert_eq!(summary.damage_dealt, 150); // clipped at the two remaining HP values
        assert_eq!(summary.enemy_kills, 2);
        assert_eq!(summary.max_single_damage, 100);
        // The secondary damage occurs after ActionCompleted. This is the
        // current local badge accounting; official treatment is not confirmed.
        assert_eq!(summary.max_action_damage, 100);
        assert_eq!(managers.terminal_outcome(), Some(BattleOutcome::Victory));
        (
            summary,
            managers.terminal_outcome(),
            result.frames.iter().map(|f| format!("{f:?}")).collect(),
        )
    }
    assert_eq!(run(), run());
}

#[test]
fn jade_ring_low_health_entry_loss_can_win_without_character_damage() {
    use crate::engine::round::outcome::BattleOutcome;
    crate::test_support::init_config();
    let entity = |uid, team, current| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(current),
        attr: Some(HeroAttribute {
            hp: Some(1000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1000)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 100)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    let entry = EntryContext::default();
    super::super::configure(
        &mut managers,
        RougeContext {
            present_danger: true,
            entry: Some(entry.clone()),
            artefacts: vec![Artefact {
                uid: 1,
                item_id: 8330413,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let ops = super::super::artefact_entry::opening(&managers, &pool, &entry);
    let result = drain::run_command_group(
        &mut managers,
        &pool,
        &SkillEffectCatalog::default(),
        &mut RoundDeterminism::with_seed(77),
        TargetContext {
            current_round: 1,
            battle_id: i32::MAX,
            ..Default::default()
        },
        ops,
    )
    .unwrap();
    assert_eq!(managers.hp.current(-1), 0);
    assert!(result.events.iter().any(|event| matches!(event,
        BattleEvent::EntityDied(death) if death.source_uid == -1 && death.target_uid == -1)));
    let summary = managers.rouge.summary().unwrap();
    assert_eq!(summary.damage_dealt, 0);
    assert_eq!(summary.enemy_kills, 0);
    assert_eq!(managers.terminal_outcome(), Some(BattleOutcome::Victory));
}

#[test]
fn bent_shotgun_rolls_two_debuffs_on_single_ultimate_not_normal_attack() {
    let (mut managers, pool, mut catalog, mut rng) = attack_fixture(8110507);
    managers.rouge.active.as_mut().unwrap().context.artefacts[0].count2 = 50; // 100% each
    catalog.insert(crate::engine::skill::effect::ParsedSkillEffect {
        skill_id: 30030131,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030131, 1000);
    catalog.insert_logic_target(30030131, 1);
    catalog.insert_effect_tag(30030131, 1);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert_eq!(managers.buff.buff_id_amount(-1, 3091), 0);
    assert_eq!(managers.buff.buff_id_amount(-1, 3101), 0);
    attack_skill(&mut managers, &pool, &catalog, &mut rng, 30030131, -1, 1);
    for id in [3091, 3101] {
        let buff = managers
            .buff
            .active_for(-1)
            .find(|buff| buff.buff_id == Some(id))
            .unwrap();
        assert_eq!(buff.duration, Some(1));
        assert_eq!(managers.buff.buff_id_amount(-2, id), 0);
    }
}

#[test]
fn wool_shears_uses_current_coin_and_caps_genesis_without_recursive_hits() {
    fn one(coin: i32) -> i32 {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8110616);
        let mut context = managers.rouge.context().unwrap().clone();
        context.artefacts[0].count2 = 50; // 100% proc chance
        context.catalyst = Some(Catalyst {
            coin,
            power_limit: 100,
            ..Default::default()
        });
        super::super::configure(&mut managers, context);
        let before = managers.hp.current(-1);
        attack(&mut managers, &pool, &catalog, &mut rng, -1);
        let dealt = before - managers.hp.current(-1);
        assert_eq!(managers.rouge.resources().unwrap().coin, coin);
        dealt
    }
    assert_eq!(one(12), 124); // 100 ordinary + 24 genesis
    assert_eq!(one(1400), 2100); // 100 ordinary + capped 2000 genesis
}

#[test]
fn golden_shears_real_hit_uses_inlays_base_cap_and_genesis_modifiers() {
    fn one(coin: i32, count2: i32, genesis: i32) -> (i32, BattleSummary) {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8210508);
        let mut context = managers.rouge.context().unwrap().clone();
        context.artefacts[0].count2 = count2;
        context.catalyst = Some(Catalyst {
            coin,
            power_limit: 100,
            ..Default::default()
        });
        super::super::configure(&mut managers, context);
        managers.attribute.add(10, AttrId::GenesisDmgBonus, genesis);
        let before = managers.hp.current(-1);
        attack(&mut managers, &pool, &catalog, &mut rng, -1);
        assert_eq!(managers.rouge.resources().unwrap().coin, coin);
        (
            before - managers.hp.current(-1),
            managers.rouge.summary().unwrap(),
        )
    }
    assert_eq!(one(0, 0, 0).0, 100);
    assert_eq!(one(12, 0, 0).0, 124);
    assert_eq!(one(2000, 0, 0).0, 2100);
    assert_eq!(one(1000, 3, 0).0, 3600); // min(1000*5, 3500) + 100
    assert_eq!(one(12, 0, 1000).0, 148); // Genesis +100% after base cap
    let extreme = one(i32::MAX, i32::MAX, 0);
    assert_eq!(extreme.0, 10_000); // clipped to the target's actual HP
    assert_eq!(extreme.1.enemy_kills, 1);
    assert_eq!(one(12, 0, 0), one(12, 0, 0));
}

#[test]
fn golden_shears_only_triggers_on_real_allied_hits_per_target() {
    use crate::engine::runtime::determinism::SkillTargetChoice;
    let (mut managers, pool, mut catalog, mut rng) = attack_fixture(8210508);
    let mut context = managers.rouge.context().unwrap().clone();
    context.catalyst = Some(Catalyst {
        coin: 12,
        power_limit: 100,
        ..Default::default()
    });
    super::super::configure(&mut managers, context);
    // A direct HP command with an indirect origin is not a main attack.
    let before = managers.hp.current(-1);
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![damage(10, -1, 10)],
    )
    .unwrap();
    assert_eq!(before - managers.hp.current(-1), 10);

    catalog.insert_logic_target(30030111, 202);
    rng.enqueue_skill_target_choices([SkillTargetChoice {
        skill_id: 30030111,
        source_uid: 10,
        target_code: 202,
        targets: vec![-1, -2],
        additional_targets: vec![],
        crit_targets: vec![],
        additional_crit_targets: vec![],
    }]);
    let before = [managers.hp.current(-1), managers.hp.current(-2)];
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert_eq!(before[0] - managers.hp.current(-1), 124);
    assert_eq!(before[1] - managers.hp.current(-2), 124);

    // Ordinary fights have no Rouge context and retain normal damage.
    let (mut ordinary, pool, catalog, mut rng) = attack_fixture(8210508);
    ordinary.rouge.active = None;
    let before = ordinary.hp.current(-1);
    attack(&mut ordinary, &pool, &catalog, &mut rng, -1);
    assert_eq!(before - ordinary.hp.current(-1), 100);
}

fn quill_kill_fixture(
    support: bool,
    below: bool,
    power: i32,
    multi: bool,
) -> (BattleManagers, Vec<BattleEvent>) {
    use crate::engine::manager::card::{CardCommand, CardManager, CardPlay};
    use crate::engine::runtime::determinism::SkillTargetChoice;
    use crate::engine::skill::action::{
        SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget,
    };
    use sonettobuf::CardInfo;
    let (mut managers, pool, mut catalog, mut rng) = attack_fixture(8310108);
    let mut context = managers.rouge.context().unwrap().clone();
    context.artefacts[0].count2 = 3;
    context.artefacts[0].only_support_below = below;
    context.catalyst = Some(Catalyst {
        power,
        power_limit: 50,
        ..Default::default()
    });
    super::super::configure(&mut managers, context);
    managers.hp.resync_current(-1, 100);
    if multi {
        managers.hp.resync_current(-2, 100);
    }
    managers.card = CardManager::new(vec![CardInfo {
        uid: Some(10),
        skill_id: Some(30030111),
        card_type: Some(if support { 2 } else { 0 }),
        ..Default::default()
    }]);
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![RuleOp::Command(BattleCommand::Card(CardCommand::Play(
            CardPlay {
                origin: ORIGIN,
                hand_index: 0,
                target_uid: Some(-1),
                chosen_skill_id: None,
                choice: None,
                recorded_skill: None,
            },
        )))],
    )
    .unwrap();
    if multi {
        catalog.insert_logic_target(30030111, 202);
        rng.enqueue_skill_target_choices([SkillTargetChoice {
            skill_id: 30030111,
            source_uid: 10,
            target_code: 202,
            targets: vec![-1, -2],
            additional_targets: vec![],
            crit_targets: vec![],
            additional_crit_targets: vec![],
        }]);
    }
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: 10,
        skill_id: 30030111,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.card_index = 1;
    invocation.target = SkillTarget::Explicit(-1);
    let result = drain::run_skill(
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
    (managers, result.events)
}

#[test]
fn quill_recovers_for_real_aid_kills_with_caps_and_independent_layout_bonus() {
    let (one, events) = quill_kill_fixture(true, false, 0, false);
    assert_eq!(one.rouge.resources().unwrap().power, 18);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::AllyAction(a) if a.enemy_kill_count == 1))
    );
    let (multi, events) = quill_kill_fixture(true, false, 0, true);
    assert_eq!(multi.rouge.resources().unwrap().power, 36);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::AllyAction(a) if a.enemy_kill_count == 2))
    );
    assert_eq!(
        quill_kill_fixture(false, false, 0, false)
            .0
            .rouge
            .resources()
            .unwrap()
            .power,
        0
    );
    assert_eq!(
        quill_kill_fixture(false, true, 0, false)
            .0
            .rouge
            .resources()
            .unwrap()
            .power,
        15
    );
    assert_eq!(
        quill_kill_fixture(true, true, 45, false)
            .0
            .rouge
            .resources()
            .unwrap()
            .power,
        50
    );
    assert_eq!(
        quill_kill_fixture(true, false, 0, false)
            .0
            .rouge
            .resources(),
        one.rouge.resources()
    );
}

#[test]
fn quill_does_not_count_unattributed_or_friendly_deaths_as_enemy_kills() {
    use crate::engine::skill::action::{ActionEvent, SkillLifecycle};
    let (mut managers, pool, catalog, mut rng) = attack_fixture(8310108);
    let mut context = managers.rouge.context().unwrap().clone();
    context.artefacts[0].only_support_below = true;
    context.catalyst = Some(Catalyst {
        power_limit: 50,
        ..Default::default()
    });
    super::super::configure(&mut managers, context);
    let action = ActionEvent {
        source_uid: 10,
        skill_id: 30030111,
        kill_count: 1,
        enemy_kill_count: 0,
        ..Default::default()
    };
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![RuleOp::SkillLifecycle(SkillLifecycle::ActionCompleted(
            action,
        ))],
    )
    .unwrap();
    assert_eq!(managers.rouge.resources().unwrap().power, 0);
    let (mut ordinary, pool, catalog, mut rng) = attack_fixture(8310108);
    ordinary.rouge.active = None;
    ordinary.hp.resync_current(-1, 100);
    attack(&mut ordinary, &pool, &catalog, &mut rng, -1);
    assert_eq!(ordinary.hp.current(-1), 0);
    assert!(ordinary.rouge.resources().is_none());
}

fn give_target_status(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    id: i32,
) {
    let op = RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid: 10,
        target_uid: -1,
        buff_id: id,
        amount: Some(1),
        occurrences: 1,
        child_uid_reservations: 0,
    })));
    drain::run_command_group(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![op],
    )
    .unwrap();
}

#[test]
fn biting_crossbow_leech_uses_committed_damage_on_real_status_targets() {
    for status in [5052, 90061, 90261] {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8310114);
        managers.hp.resync_current(10, 9000);
        give_target_status(&mut managers, &pool, &catalog, &mut rng, status);
        attack(&mut managers, &pool, &catalog, &mut rng, -1);
        assert_eq!(managers.hp.current(10), 9010, "status {status}");
    }
    // An unrelated negative status is not Gash/Bleed.
    let (mut managers, pool, catalog, mut rng) = attack_fixture(8310114);
    managers.hp.resync_current(10, 9000);
    give_target_status(&mut managers, &pool, &catalog, &mut rng, 90152);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert_eq!(managers.hp.current(10), 9000);
}

#[test]
fn biting_crossbow_handles_shields_lethal_clipping_and_ordinary_fights() {
    let (mut shielded, pool, catalog, mut rng) = attack_fixture(8310114);
    shielded.hp.resync_current(10, 9000);
    give_target_status(&mut shielded, &pool, &catalog, &mut rng, 5052);
    shielded.hp.set_shield(-1, 100);
    attack(&mut shielded, &pool, &catalog, &mut rng, -1);
    assert_eq!(shielded.hp.current(-1), 10_000);
    assert_eq!(shielded.hp.current(10), 9010);

    let (mut lethal, pool, catalog, mut rng) = attack_fixture(8310114);
    lethal.hp.resync_current(10, 9000);
    lethal.hp.resync_current(-1, 50);
    give_target_status(&mut lethal, &pool, &catalog, &mut rng, 5052);
    attack(&mut lethal, &pool, &catalog, &mut rng, -1);
    assert_eq!(lethal.hp.current(-1), 0);
    assert_eq!(lethal.hp.current(10), 9005);

    let (mut capped, pool, catalog, mut rng) = attack_fixture(8310114);
    capped.hp.resync_current(10, 9995);
    give_target_status(&mut capped, &pool, &catalog, &mut rng, 5052);
    attack(&mut capped, &pool, &catalog, &mut rng, -1);
    assert_eq!(capped.hp.current(10), 10_000);

    let (mut indirect, pool, catalog, mut rng) = attack_fixture(8310114);
    indirect.hp.resync_current(10, 9000);
    give_target_status(&mut indirect, &pool, &catalog, &mut rng, 5052);
    drain::run_command_group(
        &mut indirect,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![damage(10, -1, 100)],
    )
    .unwrap();
    assert_eq!(indirect.hp.current(10), 9000);

    let (mut ordinary, pool, catalog, mut rng) = attack_fixture(8310114);
    ordinary.rouge.active = None;
    ordinary.hp.resync_current(10, 9000);
    give_target_status(&mut ordinary, &pool, &catalog, &mut rng, 5052);
    attack(&mut ordinary, &pool, &catalog, &mut rng, -1);
    assert_eq!(ordinary.hp.current(10), 9000);
}

#[test]
fn biting_crossbow_heals_only_for_qualifying_target_in_a_multi_target_attack() {
    use crate::engine::runtime::determinism::SkillTargetChoice;
    let (mut managers, pool, mut catalog, mut rng) = attack_fixture(8310114);
    managers.hp.resync_current(10, 9000);
    give_target_status(&mut managers, &pool, &catalog, &mut rng, 5052);
    catalog.insert_logic_target(30030111, 202);
    rng.enqueue_skill_target_choices([SkillTargetChoice {
        skill_id: 30030111,
        source_uid: 10,
        target_code: 202,
        targets: vec![-1, -2],
        additional_targets: vec![],
        crit_targets: vec![],
        additional_crit_targets: vec![],
    }]);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert_eq!(managers.hp.current(10), 9010);
}

#[test]
fn bear_claw_first_real_attack_is_once_per_actor_and_round() {
    let (mut managers, pool, catalog, mut rng) = attack_fixture(8110716);
    // Eliminate the separate extra-action 50% branch so this test isolates
    // the first-attack ledger. This is a synthetic boundary fixture.
    managers.rouge.active.as_mut().unwrap().context.artefacts[0].count2 = -50;
    attack_at_round(&mut managers, &pool, &catalog, &mut rng, -1, 1);
    assert_eq!(managers.buff.buff_id_amount(-1, 90062), 1);
    attack_at_round(&mut managers, &pool, &catalog, &mut rng, -2, 1);
    assert_eq!(managers.buff.buff_id_amount(-2, 90062), 0);
    attack_at_round(&mut managers, &pool, &catalog, &mut rng, -2, 2);
    assert_eq!(managers.buff.buff_id_amount(-2, 90062), 1);
}

#[test]
fn tooth_necklace_adds_one_round_wound_after_single_real_attack() {
    let (mut managers, pool, catalog, mut rng) = attack_fixture(8110048);
    managers.rouge.active.as_mut().unwrap().context.artefacts[0].count2 = -50;
    let before = managers.hp.current(-1);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert!(managers.hp.current(-1) < before);
    let wound = managers
        .buff
        .active_for(-1)
        .find(|buff| buff.buff_id == Some(90061))
        .unwrap();
    assert_eq!(wound.duration, Some(1));
    assert_eq!(managers.buff.buff_id_amount(-2, 90061), 0);
}

fn cloak_fixture(
    moxie: i32,
    enhancement: i32,
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    crate::test_support::init_config();
    let actor = FightEntityInfo {
        uid: Some(10),
        team_type: Some(1),
        current_hp: Some(10_000),
        ex_point: Some(moxie),
        ex_point_max: Some(5),
        ex_skill: Some(30030131),
        attr: Some(HeroAttribute {
            hp: Some(10_000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    let enemy = FightEntityInfo {
        uid: Some(-1),
        team_type: Some(2),
        current_hp: Some(10_000),
        attr: Some(HeroAttribute {
            hp: Some(10_000),
            attack: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![actor],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy],
            ..Default::default()
        }),
        version: Some(7),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: vec![Artefact {
                uid: 22,
                item_id: 8220214,
                count2: enhancement,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let mut catalog = SkillEffectCatalog::default();
    for skill_id in [30030111, 30030131] {
        catalog.insert(crate::engine::skill::effect::ParsedSkillEffect {
            skill_id,
            slots: vec![],
        });
        catalog.insert_damage_rate(skill_id, 1000);
        catalog.insert_logic_target(skill_id, 1);
        catalog.insert_effect_tag(skill_id, 1);
    }
    (managers, pool, catalog, RoundDeterminism::with_seed(77))
}

fn cloak_cast(
    managers: &mut BattleManagers,
    pool: &TargetPool,
    catalog: &SkillEffectCatalog,
    rng: &mut RoundDeterminism,
    source_uid: i64,
    target_uid: i64,
) -> drain::DrainResult {
    use crate::engine::skill::action::{
        SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget,
    };
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid,
        skill_id: 30030111,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.target = SkillTarget::Explicit(target_uid);
    drain::run_skill(
        managers,
        pool,
        catalog,
        rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        invocation,
        SkillModifiers::default(),
    )
    .unwrap()
}

#[test]
fn magic_cloak_real_hit_spends_moxie_before_direct_ultimate_and_replays() {
    fn run() -> (i32, i32, i32, Vec<String>) {
        let (mut managers, pool, catalog, mut rng) = cloak_fixture(5, 75); // 100%
        let cards_before = managers.card.played().len();
        let result = cloak_cast(&mut managers, &pool, &catalog, &mut rng, -1, 10);
        assert_eq!(managers.hp.current(10), 9900);
        assert_eq!(managers.hp.current(-1), 9900);
        assert_eq!(managers.ex_point.get(10), 0);
        assert_eq!(managers.rouge.summary().unwrap().ultimate_casts, 1);
        assert_eq!(
            managers.card.played().len(),
            cards_before,
            "generated ritual does not play a card or spend card AP"
        );
        fn order(steps: &[sonettobuf::FightStep], found: &mut Vec<&'static str>) {
            for step in steps {
                if step.act_id == Some(30030131) {
                    found.push("ultimate");
                }
                for effect in &step.act_effect {
                    if effect.target_id == Some(10)
                        && effect.effect_type == Some(EffectType::Expointchange as i32)
                        && effect.effect_num == Some(-5)
                    {
                        found.push("cost");
                    }
                    if let Some(nested) = &effect.fight_step {
                        order(std::slice::from_ref(nested), found);
                    }
                }
            }
        }
        let steps = crate::engine::packet::timeline::project(&result.frames).unwrap();
        let mut seen = vec![];
        order(&steps, &mut seen);
        assert!(
            seen.iter().position(|x| *x == "cost").unwrap()
                < seen.iter().position(|x| *x == "ultimate").unwrap()
        );
        (
            managers.hp.current(10),
            managers.hp.current(-1),
            managers.ex_point.get(10),
            result.frames.iter().map(|f| format!("{f:?}")).collect(),
        )
    }
    assert_eq!(run(), run());
}

#[test]
fn magic_cloak_rejects_failed_roll_low_moxie_and_own_attacks() {
    for (moxie, enhancement, actor, target) in [(4, 75, -1, 10), (5, -25, -1, 10), (5, 75, 10, -1)]
    {
        let (mut managers, pool, catalog, mut rng) = cloak_fixture(moxie, enhancement);
        cloak_cast(&mut managers, &pool, &catalog, &mut rng, actor, target);
        assert_eq!(managers.ex_point.get(10), moxie);
        assert_eq!(managers.rouge.summary().unwrap().ultimate_casts, 0);
    }
}

#[test]
fn magic_cloak_reserves_one_cast_for_batched_hits() {
    let (mut managers, pool, catalog, mut rng) = cloak_fixture(5, 75);
    let primary_hit = || {
        let RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(mut damage))) = damage(-1, 10, 10)
        else {
            unreachable!()
        };
        damage.origin = CommandOrigin {
            domain: RuleDomain::Skill,
            key: DefinitionKey::new(30030111, "SkillDamage"),
        };
        RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(damage)))
    };
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![primary_hit(), primary_hit()],
    )
    .unwrap();
    assert_eq!(managers.hp.current(10), 9980);
    assert_eq!(managers.ex_point.get(10), 0);
    assert_eq!(managers.rouge.summary().unwrap().ultimate_casts, 1);
    assert_eq!(managers.hp.current(-1), 9900);
}

#[test]
fn magic_cloak_staged_failure_leaves_original_snapshot_unchanged() {
    let (original, pool, catalog, mut rng) = cloak_fixture(5, 75);
    let mut staged = original.clone();
    let RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(mut hit))) = damage(-1, 10, 10) else {
        unreachable!()
    };
    hit.origin = CommandOrigin {
        domain: RuleDomain::Skill,
        key: DefinitionKey::new(30030111, "SkillDamage"),
    };
    let result = drain::run_command_group(
        &mut staged,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![
            RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(hit))),
            damage(-1, 0, 10),
        ],
    );
    assert!(result.is_err());
    // The production round path stages the whole BattleRuntime on a clone and
    // publishes only success. This checks the underlying state is clone-safe.
    assert_eq!(original.ex_point.get(10), 5);
    assert_eq!(original.hp.current(10), 10_000);
    assert_eq!(original.hp.current(-1), 10_000);
    assert_eq!(original.rouge.summary().unwrap().ultimate_casts, 0);
}

#[test]
fn pitchfork_applies_two_round_bleed_only_on_real_attacks_against_negative_status() {
    let (mut managers, pool, catalog, mut rng) = attack_fixture(8110030);
    let before = managers.hp.current(-1);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert!(managers.hp.current(-1) < before);
    assert_eq!(managers.buff.buff_id_amount(-1, 5052), 0);
    let poison = RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
        origin: ORIGIN,
        source_uid: 10,
        target_uid: -1,
        buff_id: 90152,
        amount: Some(1),
        occurrences: 1,
        child_uid_reservations: 0,
    })));
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![poison],
    )
    .unwrap();
    // This valid HP command is not a main attack and must not apply bleed.
    drain::run_command_group(
        &mut managers,
        &pool,
        &catalog,
        &mut rng,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        vec![damage(10, -1, 10)],
    )
    .unwrap();
    assert_eq!(managers.buff.buff_id_amount(-1, 5052), 0);
    attack(&mut managers, &pool, &catalog, &mut rng, -1);
    assert!(managers.buff.buff_id_amount(-1, 5052) > 0);
    let bleed = managers
        .buff
        .active_for(-1)
        .find(|buff| buff.buff_id == Some(5052))
        .unwrap();
    assert_eq!(bleed.duration, Some(2));
    attack(&mut managers, &pool, &catalog, &mut rng, -2);
    assert_eq!(managers.buff.buff_id_amount(-2, 5052), 0);
}

#[test]
fn lighter_rolls_real_attack_burn_deterministically_without_indirect_triggers() {
    fn run() -> (i32, i32) {
        let (mut managers, pool, catalog, mut rng) = attack_fixture(8130505);
        for _ in 0..20 {
            attack(&mut managers, &pool, &catalog, &mut rng, -1);
        }
        let burns = managers.buff.buff_id_amount(-1, 4150001);
        // Indirect damage does not roll the artefact or add extra burn.
        drain::run_command_group(
            &mut managers,
            &pool,
            &catalog,
            &mut rng,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            vec![damage(10, -1, 1)],
        )
        .unwrap();
        assert_eq!(managers.buff.buff_id_amount(-1, 4150001), burns);
        (burns, managers.buff.buff_id_amount(-2, 4150001))
    }
    let result = run();
    assert!(result.0 > 0);
    assert_eq!(result.1, 0);
    assert_eq!(run(), result);
}
