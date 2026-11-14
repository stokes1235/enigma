use super::*;
use crate::engine::{
    manager::{
        card::{CardCommand, CardManager, CardPlay},
        hp::{
            DamageEffectKind, HpCommand, HpDamage, HpHeal, HpHealKind, HurtDamageFromType,
            HurtInfoData,
        },
    },
    runtime::{determinism::RoundDeterminism, drain, schedule},
    skill::{
        action::{SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
        effect::{ParsedSkillEffect, SkillEffectCatalog},
        target::TargetContext,
    },
};
use sonettobuf::{CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

struct Harness {
    managers: BattleManagers,
    pool: TargetPool,
    catalog: SkillEffectCatalog,
    random: RoundDeterminism,
    frames: Vec<String>,
}

fn context(items: &[i32], count: i32) -> RougeContext {
    RougeContext {
        artefacts: items
            .iter()
            .enumerate()
            .map(|(index, &item_id)| Artefact {
                uid: index as i64 + 100,
                item_id,
                ..Default::default()
            })
            .collect(),
        growth_sources: items
            .iter()
            .enumerate()
            .map(|(index, &item_id)| GrowthSource {
                uid: index as i64 + 100,
                item_id,
                syntony_artefact_count: count,
                rare_artefact_count: count,
                actor_uids: vec![10, 11],
            })
            .collect(),
        incantation_rules: Some(IncantationRules::default()),
        ..Default::default()
    }
}

impl Harness {
    fn new(context: Option<RougeContext>) -> Self {
        crate::test_support::init_config();
        let entity = |uid, hp, team| FightEntityInfo {
            uid: Some(uid),
            team_type: Some(team),
            current_hp: Some(hp),
            skill_group1: if uid == 10 {
                vec![30030111, 30030112, 30030113]
            } else {
                vec![]
            },
            skill_group2: if uid == 10 {
                vec![30030121, 30030122, 30030123]
            } else {
                vec![]
            },
            attr: Some(HeroAttribute {
                hp: Some(hp),
                attack: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![entity(10, 1000, 1), entity(11, 1000, 1)],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![entity(-1, 5000, 2), entity(-2, 5000, 2)],
                ..Default::default()
            }),
            version: Some(7),
            cur_round: Some(1),
            max_round: Some(20),
            ..Default::default()
        };
        let mut managers = BattleManagers::seeded(&fight);
        if let Some(context) = context {
            configure(&mut managers, context);
        }
        let mut catalog = SkillEffectCatalog::default();
        // Real configured IDs retain their rank/category. Their deliberately
        // simple test body makes each actual cast's damage easy to compare.
        for skill_id in [
            30030111, 30030112, 30030113, 30030121, 30030122, 30040111, 30050121, 30090121,
        ] {
            catalog.insert(ParsedSkillEffect {
                skill_id,
                slots: vec![],
            });
            catalog.insert_damage_rate(skill_id, 1000);
            catalog.insert_logic_target(skill_id, 1);
            catalog.insert_effect_tag(skill_id, 1);
        }
        Self {
            managers,
            pool: TargetPool::from_fight(&fight),
            catalog,
            random: RoundDeterminism::with_seed(42),
            frames: vec![],
        }
    }

    fn run(&mut self, ops: Vec<RuleOp>) {
        let result = drain::run_command_group(
            &mut self.managers,
            &self.pool,
            &self.catalog,
            &mut self.random,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            ops,
        )
        .unwrap();
        self.frames.push(format!("{:?}", result.frames));
    }

    fn cast(&mut self, uid: i64, skill_id: i32, selected: bool, support: bool) -> i32 {
        self.cast_target(uid, skill_id, selected, support, SkillTarget::Explicit(-1))
    }

    fn cast_target(
        &mut self,
        uid: i64,
        skill_id: i32,
        selected: bool,
        support: bool,
        target: SkillTarget,
    ) -> i32 {
        if selected {
            self.managers.card = CardManager::new(vec![CardInfo {
                uid: Some(uid),
                skill_id: Some(skill_id),
                card_type: Some(if support { 2 } else { 0 }),
                ..Default::default()
            }]);
            self.run(vec![RuleOp::Command(BattleCommand::Card(
                CardCommand::Play(CardPlay {
                    origin: ORIGIN,
                    hand_index: 0,
                    target_uid: Some(-1),
                    chosen_skill_id: None,
                    choice: None,
                    recorded_skill: None,
                }),
            ))]);
        }
        let before = self.managers.hp.current(-1);
        let mut invocation: SkillInvocation = SkillRequest {
            source_uid: uid,
            skill_id,
        }
        .into();
        invocation.mode = SkillExecutionMode::Active;
        invocation.card_index = i32::from(selected);
        invocation.target = target;
        let result = drain::run_skill(
            &mut self.managers,
            &self.pool,
            &self.catalog,
            &mut self.random,
            TargetContext {
                current_round: 1,
                ..Default::default()
            },
            invocation,
            SkillModifiers::default(),
        )
        .unwrap();
        self.frames.push(format!("{:?}", result.frames));
        before - self.managers.hp.current(-1)
    }

    fn growth(&self, uid: i64) -> i32 {
        self.managers
            .rouge
            .counters()
            .get(&uid)
            .and_then(|counter| counter.get("growth_pct"))
            .copied()
            .unwrap_or_default()
    }
}

#[test]
fn scarab_grows_once_per_actual_actor_and_rank_and_changes_later_damage() {
    let mut initial = context(&[8320310], 10);
    initial.incantation_rules = None;
    initial.permanent_growth = BTreeMap::from([(100, BTreeMap::from([("growth_pct".into(), 8)]))]);
    let mut h = Harness::new(Some(initial));
    assert_eq!(h.managers.attribute.get(10, AttrId::IncantationMight), 0); // baseline is not applied twice
    assert_eq!(h.cast(10, 30030112, true, false), 100);
    assert_eq!(h.growth(100), 2);
    assert_eq!(h.managers.attribute.get(11, AttrId::IncantationMight), 20);
    assert!(h.cast(10, 30030112, true, false) > 100);
    assert_eq!(h.growth(100), 2);
    h.cast(10, 30030113, true, false);
    h.cast(10, 30030113, true, false);
    assert_eq!(h.growth(100), 6);
    h.cast(11, 30030112, true, false);
    h.cast(11, 30030113, true, false);
    assert_eq!(h.growth(100), 12);
    assert_eq!(h.managers.attribute.get(10, AttrId::IncantationMight), 120);
    end_incantation_round(&mut h.managers);
    h.cast(10, 30030112, true, false);
    assert_eq!(h.growth(100), 12); // per-battle guard survives a round reset
}

#[test]
fn actual_attack_cards_trigger_syntony_growth_and_round_bonus_expires_alone() {
    let mut h = Harness::new(Some(context(&[8310405, 8330418], 10)));
    for _ in 0..3 {
        assert_eq!(h.cast(10, 30030111, true, false), 100);
    }
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 50);
    assert_eq!(h.growth(100), 0);
    // Reality and Mental are the same Attack syntony category. An actual
    // SupportNormal also counts, using the deployed actor's statistics.
    assert!(h.cast(10, 30040111, true, true) > 100);
    assert_eq!((h.growth(100), h.growth(101)), (4, 4));
    h.cast(11, 30030111, true, false);
    assert_eq!((h.growth(100), h.growth(101)), (12, 12));
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 3);
    assert_eq!(h.managers.rouge.summary().unwrap().support_casts, 1);
    assert_eq!(h.managers.attribute.get(11, AttrId::Attack), 200);
    end_incantation_round(&mut h.managers);
    assert_eq!(h.managers.attribute.get(11, AttrId::Attack), 0);
    assert_eq!(h.managers.attribute.get(11, AttrId::DmgBonus), 120);
    assert_eq!(h.managers.attribute.get(11, AttrId::DmgTakenReduction), 120);
    h.cast(10, 30030111, true, false);
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 0);
    assert_eq!(h.growth(100), 12);
}

#[test]
fn queued_and_extra_actions_do_not_fabricate_syntony_or_growth() {
    let initial = context(&[8310405], 9);
    let mut h = Harness::new(Some(initial));
    h.managers.card = CardManager::new(vec![
        CardInfo {
            uid: Some(10),
            skill_id: Some(30030111),
            ..Default::default()
        };
        5
    ]);
    for _ in 0..5 {
        h.run(vec![RuleOp::Command(BattleCommand::Card(
            CardCommand::Play(CardPlay {
                origin: ORIGIN,
                hand_index: 0,
                target_uid: Some(-1),
                chosen_skill_id: None,
                choice: None,
                recorded_skill: None,
            }),
        ))]);
    }
    h.run(vec![RuleOp::Command(BattleCommand::Card(
        CardCommand::CommitActionQueue {
            team: 1,
            emitter_uid: 10,
            include_device_action: false,
        },
    ))]);
    assert_eq!(h.growth(100), 0);
    for _ in 0..5 {
        h.cast(10, 30030111, false, false);
    }
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 0);
    assert_eq!(h.managers.rouge.summary().unwrap().extra_action_casts, 5);
    for _ in 0..5 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.growth(100), 4); // layout below ten cannot unlock the extra eight
    let mut locked = context(&[8310405], 10);
    locked.incantation_rules = None;
    let mut locked = Harness::new(Some(locked));
    for _ in 0..5 {
        locked.cast(10, 30030111, true, false);
    }
    assert_eq!(locked.growth(100), 0);
}

#[test]
fn distinct_actual_card_types_trigger_resonance_and_keep_exact_temporary_ownership() {
    let mut h = Harness::new(Some(context(&[], 0)));
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    let summary = h.managers.rouge.summary().unwrap();
    assert_eq!(summary.resonance_max_level, 1);
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 40);
    assert_eq!(h.managers.attribute.get(11, AttrId::RealityDef), 40);
    h.managers.attribute.add(10, AttrId::Attack, 70);
    end_incantation_round(&mut h.managers);
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 70);
    assert_eq!(h.managers.attribute.get(11, AttrId::RealityDef), 0);
}

#[test]
fn syntony_level_three_grants_support_and_relevant_artefact_states_from_real_casts() {
    let mut frozen = context(&[8120406, 8130403, 8130404], 0);
    frozen.catalyst = Some(Catalyst {
        style_id: 5,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 2);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90281), 2);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90281), 2);
    assert_eq!(
        h.managers.buff.buff_id_amount(-1, 90101) + h.managers.buff.buff_id_amount(-2, 90101),
        18
    );
    h.cast(10, 30030111, true, false);
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 3);
    assert_eq!(
        h.managers.buff.buff_id_amount(10, 90331) + h.managers.buff.buff_id_amount(11, 90331),
        1
    );
    assert_eq!(h.managers.buff.buff_id_amount(10, 90281), 2);
    assert_eq!(
        h.managers.buff.buff_id_amount(-1, 90101) + h.managers.buff.buff_id_amount(-2, 90101),
        28
    );
    end_incantation_round(&mut h.managers);
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(
        h.managers.buff.buff_id_amount(10, 90281),
        2,
        "root is once per fight"
    );
}

#[test]
fn resonance_crown_and_catalyst_react_to_real_level_crossings() {
    let mut frozen = context(&[8320506], 0);
    frozen.catalyst = Some(Catalyst {
        style_id: 6,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    h.catalog.insert(ParsedSkillEffect {
        skill_id: 30020121,
        slots: vec![],
    });
    h.catalog.insert_damage_rate(30020121, 1000);
    h.catalog.insert_logic_target(30020121, 1);
    h.catalog.insert_effect_tag(30020121, 1);
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    assert_eq!(h.managers.rouge.summary().unwrap().resonance_max_level, 2);
    for id in [90001, 90031, 90021] {
        assert_eq!(h.managers.buff.buff_id_amount(10, id), 1);
        assert_eq!(h.managers.buff.buff_id_amount(11, id), 1);
    }
    h.cast(10, 30020121, true, false);
    assert_eq!(h.managers.rouge.summary().unwrap().resonance_max_level, 3);
    for id in [90001, 90031, 90021] {
        assert_eq!(h.managers.buff.buff_id_amount(10, id), 2);
    }
    assert_eq!(
        h.managers.buff.buff_id_amount(10, 90331) + h.managers.buff.buff_id_amount(11, 90331),
        1
    );
}

#[test]
fn level_limit_unlock_changes_real_casts_without_repeating_permanent_growth() {
    for limit in [5, 6] {
        let mut frozen = context(&[8310405], 10);
        frozen
            .incantation_rules
            .as_mut()
            .unwrap()
            .syntony_level_limit = limit;
        let mut h = Harness::new(Some(frozen));
        for _ in 0..8 {
            h.cast(10, 30030111, true, false);
        }
        assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, limit);
        assert_eq!(h.growth(100), 12);
        assert_eq!(
            h.managers.attribute.get(10, AttrId::Attack),
            if limit == 5 { 400 } else { 600 }
        );
    }
}

#[test]
fn action_damage_maximum_accumulates_actual_targets_instead_of_one_hit() {
    let mut h = Harness::new(Some(context(&[], 0)));
    h.catalog.insert_logic_target(30030111, 301); // All enemies, as a real AoE body.
    h.cast_target(10, 30030111, true, false, SkillTarget::Configured);
    let summary = h.managers.rouge.summary().unwrap();
    assert_eq!(h.managers.hp.current(-1), 4900);
    assert_eq!(h.managers.hp.current(-2), 4900);
    assert_eq!(summary.damage_dealt, 200);
    assert_eq!(summary.max_single_damage, 100);
    assert_eq!(summary.max_action_damage, 200);
    assert_eq!(summary.casts, 1);
}

#[test]
fn generic_skill3_copies_of_a_basic_skill_cannot_impersonate_739_precasts() {
    let mut h = Harness::new(Some(context(&[], 0)));
    for skill in [30030111, 30030121, 30030122] {
        h.managers.card.add_temp_card_for(10, skill, None, 0, 1);
    }
    schedule::run_player_action_queue(
        &mut h.managers,
        &h.pool,
        &h.catalog,
        &mut h.random,
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        (0..3).map(|_| CardPlay {
            origin: ORIGIN,
            hand_index: 0,
            target_uid: Some(-1),
            chosen_skill_id: None,
            choice: None,
            recorded_skill: None,
        }),
        1,
        0,
    )
    .unwrap();
    let summary = h.managers.rouge.summary().unwrap();
    assert_eq!(summary.casts, 3);
    assert_eq!(summary.syntony_max_level, 0);
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 0);
}

fn damage(source_uid: i64, target_uid: i64, amount: i32) -> RuleOp {
    RuleOp::Command(BattleCommand::Hp(HpCommand::Damage(HpDamage {
        origin: ORIGIN,
        source_uid,
        target_uid,
        amount,
        config_effect: 1,
        effect_kind: DamageEffectKind::Normal,
        assassinate: false,
        ignore_riposte: false,
        hurt: HurtInfoData {
            from_uid: source_uid,
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
fn summary_uses_clipped_committed_damage_healing_and_real_zero_to_positive_revives() {
    let mut h = Harness::new(Some(context(&[], 0)));
    h.managers.hp.set_shield(-1, 100);
    h.run(vec![damage(10, -1, 10000)]);
    h.run(vec![damage(-2, 10, 10000)]);
    for _ in 0..2 {
        h.run(vec![RuleOp::Command(BattleCommand::Hp(HpCommand::Heal(
            HpHeal {
                origin: ORIGIN,
                source_uid: 11,
                target_uid: 10,
                amount: 10000,
                config_effect: 1,
                kind: HpHealKind::Revive,
            },
        )))]);
    }
    let summary = h.managers.rouge.summary().unwrap();
    assert_eq!(
        (summary.damage_dealt, summary.max_single_damage),
        (5100, 5100)
    );
    assert_eq!(summary.damage_taken, 1000);
    assert_eq!(summary.healing_done, 1000);
    assert_eq!(
        (
            summary.enemy_kills,
            summary.ally_deaths,
            summary.ally_revives
        ),
        (1, 1, 1)
    );
    assert_eq!(summary.casts, 0); // damage commands are not invented casts
}

#[test]
fn frozen_growth_context_and_real_cast_operations_replay_counters_attributes_and_statistics() {
    let frozen = context(&[8310405, 8330418, 8320310], 10);
    let restored = serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    let mut first = Harness::new(Some(frozen));
    let mut replay = Harness::new(Some(restored));
    for h in [&mut first, &mut replay] {
        for (uid, skill) in [
            (10, 30030112),
            (10, 30030112),
            (11, 30030112),
            (10, 30040111),
            (11, 30030113),
        ] {
            h.cast(uid, skill, true, false);
        }
        end_incantation_round(&mut h.managers);
        h.cast(11, 30030113, true, false);
    }
    assert_eq!(first.frames, replay.frames);
    assert_eq!(
        first.managers.rouge.counters(),
        replay.managers.rouge.counters()
    );
    assert_eq!(
        first.managers.rouge.summary(),
        replay.managers.rouge.summary()
    );
    assert_eq!(
        first.managers.hp.current(-1),
        replay.managers.hp.current(-1)
    );
    assert_eq!(
        first.managers.attribute.get(11, AttrId::DmgBonus),
        replay.managers.attribute.get(11, AttrId::DmgBonus)
    );
    let summary = first.managers.rouge.summary().unwrap();
    assert_eq!(summary.casts, 6);
    assert_eq!(
        (
            summary.rank1_casts,
            summary.rank2_casts,
            summary.rank3_casts
        ),
        (1, 3, 2)
    );
    assert!(summary.max_action_damage >= 100);
    let mut ordinary = Harness::new(None);
    ordinary.cast(10, 30030112, true, false);
    assert!(ordinary.managers.rouge.summary().is_none());
    assert!(ordinary.managers.rouge.counters().is_empty());
}

#[test]
fn resource_artefacts_credit_actual_spells_once_and_replay_deterministically() {
    let mut frozen = context(&[8230614, 8310510], 0);
    frozen.artefacts[1].count2 = 3;
    frozen.artefacts[1].non_resonance_in_range = true;
    frozen.catalyst = Some(Catalyst {
        style_id: 1,
        coin: 100,
        power_limit: 100,
        ..Default::default()
    });
    let run = |frozen: RougeContext| {
        let mut h = Harness::new(Some(frozen));
        for skill in [30030112, 30030112, 30030111, 30030111, 30050121, 30090121] {
            h.cast(10, skill, true, false);
        }
        assert_eq!(h.managers.rouge.resources().unwrap().coin, 156);
        assert_eq!(h.managers.buff.buff_id_amount(10, 90021), 6);
        end_incantation_round(&mut h.managers);
        for skill in [30030111, 30030111, 30030111] {
            h.cast(10, skill, true, false);
        }
        assert_eq!(
            h.managers.rouge.resources().unwrap().coin,
            176,
            "first syntony awards the bomb once"
        );
        end_incantation_round(&mut h.managers);
        for skill in [30030111, 30030111, 30030111] {
            h.cast(10, skill, true, false);
        }
        assert_eq!(
            h.managers.rouge.resources().unwrap().coin,
            176,
            "bomb syntony is once per battle"
        );
        (h.managers.rouge.resources().unwrap().clone(), h.frames)
    };
    assert_eq!(run(frozen.clone()), run(frozen));
}

#[test]
fn claw_rank_spell_works_before_syntony_unlock() {
    let mut frozen = context(&[8310510], 0);
    frozen.incantation_rules = None;
    frozen.catalyst = Some(Catalyst {
        style_id: 1,
        coin: 10,
        power_limit: 100,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    h.cast(10, 30030112, true, false);
    assert_eq!(h.managers.rouge.resources().unwrap().coin, 15);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90021), 2);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90021), 0);
    h.cast(10, 30030112, false, false);
    assert_eq!(h.managers.rouge.resources().unwrap().coin, 15);
}

#[test]
fn syntony_and_resonance_artefacts_affect_the_correct_teams() {
    let mut h = Harness::new(Some(context(&[8120416, 8130505, 8330513], 0)));
    for skill in [30030111, 30030111, 30030111] {
        h.cast(10, skill, true, false);
    }
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90031), 5);
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90032), 5);
    }
    h.cast(10, 30030112, true, false);
    h.cast(10, 30050121, true, false);
    // Resonance adds 10 to both enemies. The directly hit enemy can also
    // receive additional stacks from the same artefact's 50% on-hit effect.
    assert_eq!(h.managers.buff.buff_id_amount(-2, 4150001), 10);
    assert!(h.managers.buff.buff_id_amount(-1, 4150001) >= 10);
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 2);
    }
    h.run(vec![
        grant(10, -1, 90021),
        grant(10, -1, 90021),
        grant(10, -1, 90021),
    ]);
    h.cast(10, 30090121, true, false);
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90021), 1);
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 6);
    }
}

#[test]
fn wyrm_toe_rolls_once_per_syntony_and_targets_each_living_enemy() {
    let mut frozen = context(&[8110410], 0);
    frozen.artefacts[0].count2 = 25; // one base Syntony Disk: 75%
    let mut h = Harness::new(Some(frozen));
    h.cast(10, 30030111, true, false);
    h.cast(10, 30030111, true, false);
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90061), 0);
    h.random.enqueue_permille_rolls([749, 750]);
    h.cast(10, 30030111, true, false);
    for uid in [-1, -2] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90061), 2);
    }
    h.cast(10, 30030111, true, false);
    for uid in [-1, -2] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90061), 2);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 90061), 0);
}

#[test]
fn timid_caltrop_rolls_once_per_resonance_and_uses_the_real_thorns_status() {
    let mut frozen = context(&[8120512], 0);
    frozen.artefacts[0].count2 = 25; // one base Resonance Disk: 75%
    let mut h = Harness::new(Some(frozen));
    h.random.enqueue_permille_rolls([749, 750]);
    h.cast(10, 30030111, true, false);
    h.cast(10, 30050121, true, false);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90021), 0);
    h.cast(10, 30090121, true, false);
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 2);
    }
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90021), 0);
    end_incantation_round(&mut h.managers);
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 2);
    }
}

#[test]
fn wyrm_toe_uses_telescope_probability_and_does_not_trigger_when_absent() {
    let mut frozen = context(&[8110410], 0);
    frozen.artefacts[0].extra_action_chance_doublers = 1;
    let mut h = Harness::new(Some(frozen));
    h.random.enqueue_permille_rolls([999]);
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90061), 2);
    let mut without = Harness::new(Some(context(&[], 0)));
    for _ in 0..3 {
        without.cast(10, 30030111, true, false);
    }
    assert_eq!(without.managers.buff.buff_id_amount(-1, 90061), 0);
}

#[test]
fn syntony_eye_thresholds_are_independent_once_per_fight_and_use_frozen_range() {
    for nearby in [false, true] {
        let mut frozen = context(&[8210411], 0);
        frozen.artefacts[0].count2 = 2;
        frozen.artefacts[0].syntony_in_range = nearby;
        let mut h = Harness::new(Some(frozen));
        for _ in 0..3 {
            h.cast(10, 30030111, true, false);
        }
        assert_eq!(h.managers.buff.buff_id_amount(10, 90001), 0);
        h.cast(10, 30030111, true, false);
        assert_eq!(
            h.managers.buff.buff_id_amount(10, 90001),
            if nearby { 6 } else { 0 }
        );
        h.cast(10, 30030111, true, false);
        let expected = if nearby { 17 } else { 11 };
        assert_eq!(h.managers.buff.buff_id_amount(10, 90001), expected);
        assert_eq!(h.managers.buff.buff_id_amount(11, 90001), expected);
        end_incantation_round(&mut h.managers);
        for _ in 0..5 {
            h.cast(10, 30030111, true, false);
        }
        assert_eq!(h.managers.buff.buff_id_amount(10, 90001), expected);
    }
}

#[test]
fn syntony_eye_level_jump_crosses_both_thresholds_once() {
    let mut frozen = context(&[8210411], 0);
    frozen.artefacts[0].syntony_in_range = true;
    frozen.catalyst = Some(Catalyst {
        style_id: 5,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    // The style and its active boost raise the first trigger directly to 3.
    h.managers
        .rouge
        .active
        .as_mut()
        .unwrap()
        .catalyst
        .as_mut()
        .unwrap()
        .syntony_boost = 1;
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 90001), 15);
    h.cast(10, 30030111, true, false);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90001), 15);
}

#[test]
fn resonance_artefacts_preserve_every_draw_and_prioritize_missing_buffs() {
    let mut frozen = context(&[8130503, 8130504, 8220516, 8320511], 0);
    frozen.artefacts[2].count2 = 2;
    frozen.artefacts[2].non_resonance_in_range = true;
    frozen.artefacts[3].count2 = 2;
    frozen.artefacts[3].resonance_artefact_count = 3;
    frozen.catalyst = Some(Catalyst {
        style_id: 6,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    // 1 + 1 + (1+2+1) + (1+2+3) = 12 rolls per ally.
    for uid in [10, 11] {
        let ids = [90001, 90011, 90041, 90021, 90031, 90281];
        assert!(
            ids.iter()
                .all(|id| h.managers.buff.buff_id_amount(uid, *id) > 0)
        );
        assert_eq!(
            ids.iter()
                .map(|id| h.managers.buff.buff_id_amount(uid, *id))
                .sum::<i32>(),
            12
        );
    }
    end_incantation_round(&mut h.managers);
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    for uid in [10, 11] {
        let ids = [90001, 90011, 90041, 90021, 90031, 90281];
        assert_eq!(
            ids.iter()
                .map(|id| h.managers.buff.buff_id_amount(uid, *id))
                .sum::<i32>(),
            24
        );
    }
}

#[test]
fn compass_uses_effective_attack_and_grants_actual_status_immunity() {
    let mut h = Harness::new(Some(context(&[8330412], 0)));
    h.managers.attribute.add(11, AttrId::Attack, 1000);
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 500212), 0);
    assert!(h.managers.buff.buff_id_amount(11, 500212) > 0);
    h.run(vec![RuleOp::Command(BattleCommand::Buff(
        BuffCommand::Grant(BuffGrant {
            origin: ORIGIN,
            source_uid: -1,
            target_uid: 11,
            buff_id: 90152,
            amount: Some(1),
            occurrences: 1,
            child_uid_reservations: 0,
        }),
    ))]);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90152), 0);
    h.cast(10, 30030111, true, false);
    h.cast(10, 30030111, true, false);
    assert!(h.managers.buff.buff_id_amount(10, 500212) > 0);
    assert!(h.managers.buff.buff_id_amount(11, 500212) > 0);
}

#[test]
fn lanterns_prioritize_distinct_negative_statuses_and_replay_deterministically() {
    let mut frozen = context(&[8320514, 8320514], 0);
    for item in &mut frozen.artefacts {
        item.count2 = 3;
        item.non_resonance_in_range = true;
    }
    let run = |frozen: RougeContext| {
        let mut h = Harness::new(Some(frozen));
        for skill in [30030111, 30050121, 30090121] {
            h.cast(10, skill, true, false);
        }
        let ids = [90063, 90233, 90154, 90223, 90163];
        let result: Vec<Vec<i32>> = [-1, -2]
            .into_iter()
            .map(|uid| {
                ids.iter()
                    .map(|id| h.managers.buff.buff_id_amount(uid, *id))
                    .collect()
            })
            .collect();
        for amounts in &result {
            assert_eq!(amounts.iter().filter(|&&n| n > 0).count(), 2);
        }
        result
    };
    assert_eq!(run(frozen.clone()), run(frozen));
}

#[test]
fn lantern_recognizes_existing_negative_statuses_by_type_not_exact_id() {
    let mut h = Harness::new(Some(context(&[8320514], 0)));
    // These are duration or skill-specific IDs, but share the same semantic
    // status types as the lantern's own Poison, Bleed, Pollution and Corrode.
    for buff_id in [5051, 21231, 90231, 90221] {
        h.run(vec![RuleOp::Command(BattleCommand::Buff(
            BuffCommand::Grant(BuffGrant {
                origin: ORIGIN,
                source_uid: 10,
                target_uid: -1,
                buff_id,
                amount: Some(1),
                occurrences: 1,
                child_uid_reservations: 0,
            }),
        ))]);
    }
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    // The only missing status family is Gash; a random exact-ID-only check
    // could mistakenly choose Bleed 90161 even though 5051 was already active.
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90061), 1);
    assert_eq!(h.managers.buff.buff_id_amount(-1, 90161), 0);
    assert!(h.managers.buff.has_active_buff_id_or_type(-1, 6002));
}

#[test]
fn lantern_recognizes_pending_alternative_status_ids_by_type_and_target() {
    let h = Harness::new(Some(context(&[8320514], 0)));
    let pending = vec![RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(
        BuffGrant {
            origin: ORIGIN,
            source_uid: 10,
            target_uid: -1,
            buff_id: 5052,
            amount: Some(1),
            occurrences: 1,
            child_uid_reservations: 0,
        },
    )))];
    assert!(super::growth::lantern_status_present(
        &h.managers,
        -1,
        6002,
        &pending
    ));
    assert!(!super::growth::lantern_status_present(
        &h.managers,
        -2,
        6002,
        &pending
    ));
    assert!(!super::growth::lantern_status_present(
        &h.managers,
        -1,
        6003,
        &pending
    ));
}

#[test]
fn resonance_crossbow_targets_one_live_ally_with_frozen_range_tier() {
    for nearby in [false, true] {
        let mut frozen = context(&[8210105], 0);
        frozen.artefacts[0].count2 = 50; // 100% makes target selection test deterministic.
        frozen.artefacts[0].non_support_in_range = nearby;
        let mut h = Harness::new(Some(frozen));
        for skill in [30030111, 30050121, 30090121] {
            h.cast(10, skill, true, false);
        }
        let (empower, support) = if nearby { (5024, 90332) } else { (5021, 90331) };
        let owners: Vec<_> = [10, 11]
            .into_iter()
            .filter(|uid| h.managers.buff.buff_id_amount(*uid, empower) > 0)
            .collect();
        assert_eq!(owners.len(), 1);
        assert!(h.managers.buff.buff_id_amount(owners[0], support) > 0);
    }
}

#[test]
fn glasses_queue_independent_team_action_points_for_the_next_round() {
    let mut h = Harness::new(Some(context(&[8230414, 8230509, 8330417], 0)));
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(super::pending_action_point_bonus(&h.managers), 2);
    for skill in [30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    assert_eq!(super::pending_action_point_bonus(&h.managers), 4);
    end_incantation_round(&mut h.managers);
    assert_eq!(super::pending_action_point_bonus(&h.managers), 4);
    super::consume_pending_action_points(&mut h.managers);
    assert_eq!(super::pending_action_point_bonus(&h.managers), 0);
    assert_eq!(super::persistent_action_point_bonus(&h.managers), 0);
}

#[test]
fn aid_artefacts_queue_next_round_syntony_and_obey_per_fight_caps() {
    let mut frozen = context(&[8220409, 8310407], 0);
    frozen.artefacts[0].count2 = 40; // 60% + 40% = 100%.
    let mut h = Harness::new(Some(frozen));
    for _ in 0..6 {
        h.cast(10, 30040111, true, true);
    }
    let active = h.managers.rouge.active.as_ref().unwrap();
    assert_eq!(active.next_syntony_bonus, 7); // black horn 2, ancient axe 5.
    assert_eq!(active.current_syntony_bonus, 0);
    end_incantation_round(&mut h.managers);
    assert_eq!(
        h.managers
            .rouge
            .active
            .as_ref()
            .unwrap()
            .current_syntony_bonus,
        7
    );
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 5);
    end_incantation_round(&mut h.managers);
    assert_eq!(
        h.managers
            .rouge
            .active
            .as_ref()
            .unwrap()
            .current_syntony_bonus,
        0
    );
}

#[test]
fn syntony_axe_level_two_grant_and_level_five_dodge_use_real_cards() {
    let mut frozen = context(&[8310407, 8320408], 0);
    frozen.artefacts[0].count2 = 60; // 40% + 60% = 100%.
    let mut h = Harness::new(Some(frozen));
    for _ in 0..4 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 90001), 12);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90001), 12);
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 90201), 1);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90201), 1);
}

#[test]
fn ultimate_queues_inlay_and_frozen_range_boost_until_next_round() {
    let mut frozen = context(&[8320408], 0);
    frozen.artefacts[0].count2 = 2;
    frozen.artefacts[0].syntony_in_range_count = 4;
    let mut h = Harness::new(Some(frozen));
    h.catalog.insert(ParsedSkillEffect {
        skill_id: 30030131,
        slots: vec![],
    });
    h.catalog.insert_damage_rate(30030131, 1000);
    h.catalog.insert_logic_target(30030131, 1);
    h.catalog.insert_effect_tag(30030131, 1);
    h.cast(10, 30030131, true, false);
    assert_eq!(
        h.managers.rouge.active.as_ref().unwrap().next_syntony_bonus,
        4
    );
    end_incantation_round(&mut h.managers);
    assert_eq!(
        h.managers
            .rouge
            .active
            .as_ref()
            .unwrap()
            .current_syntony_bonus,
        4
    );
}

#[test]
fn bullet_loss_consumes_actual_stacks_once_and_queues_only_up_to_quota() {
    use crate::engine::manager::buff::{BuffConsume, BuffSelector, DepletedBuff};
    let mut frozen = context(&[8220415], 0);
    frozen.artefacts[0].count2 = 20; // 80% + 20% = 100%.
    let mut h = Harness::new(Some(frozen));
    h.run(vec![RuleOp::Command(BattleCommand::Buff(
        BuffCommand::Grant(BuffGrant {
            origin: ORIGIN,
            source_uid: 10,
            target_uid: 10,
            buff_id: 90501,
            amount: Some(3),
            occurrences: 1,
            child_uid_reservations: 0,
        }),
    ))]);
    let bullet_uid = h.managers.buff.buff_id_or_type_uid(10, 90501).unwrap();
    assert_eq!(h.managers.buff.buff_id_amount(10, 90501), 3);
    h.run(vec![RuleOp::Command(BattleCommand::Buff(
        BuffCommand::ConsumeCoalesced(BuffConsume {
            origin: ORIGIN,
            target_uid: 10,
            selector: BuffSelector::ExactId(90501),
            amount: 1,
            depleted: DepletedBuff::Remove,
        }),
    ))]);
    assert_eq!(
        h.managers.rouge.active.as_ref().unwrap().next_syntony_bonus,
        1
    );
    let duplicate = BattleEvent::BuffChanged(crate::engine::event::payload::BuffChangeEvent {
        source_uid: 10,
        target_uid: 10,
        buff_uid: bullet_uid,
        buff_id: 90501,
        before_amount: 3,
        after_amount: 2,
        act_id: 0,
        act_value: 0,
        duration_expired: false,
    });
    super::growth::observe_bullet_change(&mut h.managers, &h.pool, &duplicate, &mut h.random);
    assert_eq!(
        h.managers.rouge.active.as_ref().unwrap().next_syntony_bonus,
        1
    );
    h.run(vec![RuleOp::Command(BattleCommand::Buff(
        BuffCommand::ConsumeCoalesced(BuffConsume {
            origin: ORIGIN,
            target_uid: 10,
            selector: BuffSelector::ExactId(90501),
            amount: 2,
            depleted: DepletedBuff::Remove,
        }),
    ))]);
    assert_eq!(
        h.managers.rouge.active.as_ref().unwrap().next_syntony_bonus,
        2
    );
    end_incantation_round(&mut h.managers);
    assert_eq!(
        h.managers
            .rouge
            .active
            .as_ref()
            .unwrap()
            .current_syntony_bonus,
        2
    );
}

#[test]
fn jade_ring_syntony_direct_hp_loss_is_not_counted_as_player_damage() {
    let mut h = Harness::new(Some(context(&[8330413], 0)));
    for _ in 0..3 {
        h.cast(10, 30030111, true, false);
    }
    assert_eq!(h.managers.hp.current(-1), 4465); // 4700 after attacks, then -5%.
    assert_eq!(h.managers.hp.current(-2), 4750);
    assert_eq!(h.managers.rouge.summary().unwrap().damage_dealt, 300);
}

#[test]
fn golden_shears_resonance_gives_all_allies_frozen_range_and_inlay_thorns() {
    let mut frozen = context(&[8210508], 0);
    frozen.artefacts[0].count4 = 2;
    frozen.artefacts[0].non_resonance_in_range = true;
    let mut h = Harness::new(Some(frozen));
    for skill in [30030111, 30050121, 30090121] {
        h.cast(10, skill, true, false);
    }
    assert_eq!(h.managers.buff.buff_id_amount(10, 90021), 5);
    assert_eq!(h.managers.buff.buff_id_amount(11, 90021), 5);
}

#[test]
fn telescope_guarantees_both_extra_action_procs_on_real_cast() {
    let mut frozen = context(&[8230714], 0);
    frozen.artefacts[0].extra_action_chance_doublers = 1;
    let mut h = Harness::new(Some(frozen));
    h.cast(10, 30030111, false, false);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90001), 2);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90002), 2);
}

#[test]
fn telescope_probability_multiplies_then_caps_without_touching_noncovered_sources() {
    let mut item = Artefact::default();
    assert_eq!(item.proc_chance(250), 250);
    item.extra_action_chance_doublers = 1;
    assert_eq!(item.proc_chance(250), 500);
    assert_eq!(item.proc_chance(600), 1000);
    item.extra_action_chance_doublers = 2;
    assert_eq!(item.proc_chance(250), 1000);
    assert_eq!(item.proc_chance(-10), 0);
}

#[test]
fn stag_struggle_grants_thorns_and_blessing_on_actual_resonance() {
    let mut frozen = context(&[8320709], 0);
    // Isolate resonance from the item's independent first/extra action rolls.
    frozen.artefacts[0].count2 = -60;
    frozen.artefacts[0].count4 = -50;
    let mut h = Harness::new(Some(frozen));
    h.cast(10, 30030111, true, false);
    h.cast(10, 30050121, true, false);
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 0);
        assert_eq!(h.managers.buff.buff_id_amount(uid, 91210030), 0);
    }
    h.cast(10, 30090121, true, false);
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90021), 1);
        assert_eq!(h.managers.buff.buff_id_amount(uid, 91210030), 1);
    }
}

#[test]
fn lance_ultimate_awards_ritual_and_reports_only_actual_new_stacks() {
    let mut frozen = context(&[8210209], 0);
    frozen.incantation_rules = None; // Lance is independent of the unlock.
    frozen.artefacts[0].count2 = 1;
    frozen.artefacts[0].count4 = 50; // 100% chance.
    frozen
        .permanent_growth
        .insert(100, BTreeMap::from([("ritual_stacks".to_owned(), 8)]));
    let mut h = Harness::new(Some(frozen));
    h.catalog.insert(ParsedSkillEffect {
        skill_id: 30030131,
        slots: vec![],
    });
    h.catalog.insert_damage_rate(30030131, 1000);
    h.catalog.insert_logic_target(30030131, 1);
    h.catalog.insert_effect_tag(30030131, 1);
    h.run(vec![damage(-1, -1, 4950)]);
    assert_eq!(h.managers.hp.current(-1), 50);
    h.cast(10, 30030131, true, false);
    assert_eq!(h.managers.hp.current(-1), 0);
    assert_eq!(h.managers.rouge.counters()[&100]["ritual_stacks"], 1);
    // 2 base + 1 inlay + 8 prior + 1 newly earned. The local timing policy
    // lets the kill increase the award from the same completed ultimate.
    for uid in [10, 11] {
        assert_eq!(h.managers.buff.buff_id_amount(uid, 90091), 12);
    }
    assert_eq!(h.managers.attribute.get(10, AttrId::DmgBonus), 0);
}

#[test]
fn lance_ritual_stack_limit_and_failed_roll_are_independent() {
    let mut frozen = context(&[8210209], 0);
    frozen
        .permanent_growth
        .insert(100, BTreeMap::from([("ritual_stacks".to_owned(), 9)]));
    frozen.artefacts[0].count4 = -50; // 0% means kill growth still applies.
    let mut h = Harness::new(Some(frozen));
    let action = crate::engine::skill::action::ActionEvent {
        source_uid: 10,
        skill_id: 30030131,
        enemy_kill_count: 4,
        ..Default::default()
    };
    let ops = growth::completed_action(&mut h.managers, &h.pool, &action, &mut h.random);
    h.run(ops);
    assert_eq!(h.managers.rouge.counters()[&100]["ritual_stacks"], 1);
    assert_eq!(h.managers.buff.buff_id_amount(10, 90091), 0);
    let ops = growth::completed_action(&mut h.managers, &h.pool, &action, &mut h.random);
    h.run(ops);
    assert_eq!(h.managers.rouge.counters()[&100]["ritual_stacks"], 1);
}
