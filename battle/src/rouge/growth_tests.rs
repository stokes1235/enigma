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
