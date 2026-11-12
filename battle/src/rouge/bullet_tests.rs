//! HasBullet snapshots the actual cast owner's live buff lane, never an item
//! name or a team-wide flag. Actual damage must remain local to one execution.
use super::*;
use crate::engine::{
    manager::card::{CardCommand, CardManager, CardPlay},
    runtime::{determinism::RoundDeterminism, drain},
    skill::{
        action::{SkillExecutionMode, SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
        effect::{ParsedSkillEffect, SkillEffectCatalog},
        target::TargetContext,
    },
};
use sonettobuf::{BuffInfo, CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn damage(owner: i64, bullet_uid: Option<i64>, modifier: bool, extra: bool) -> i32 {
    crate::test_support::init_config();
    let entity = |uid, team| FightEntityInfo {
        uid: Some(uid),
        team_type: Some(team),
        current_hp: Some(10000),
        attr: Some(HeroAttribute {
            hp: Some(10000),
            attack: Some(100),
            ..Default::default()
        }),
        // 90501 is an actual persistent Bullet carrier: 100#211#100|827.
        // Its other lane changes healing, so it cannot itself alter damage.
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
            entitys: vec![entity(10, 1), entity(11, 1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    if modifier {
        configure(
            &mut managers,
            RougeContext {
                conditional_modifiers: vec![ConditionalModifier {
                    source_uid: 101,
                    item_id: 8210711,
                    predicate: AttackPredicate::All(vec![
                        AttackPredicate::HasBullet,
                        AttackPredicate::NonExtraAction,
                    ]),
                    attr_id: 205,
                    value: 600,
                }],
                ..Default::default()
            },
        );
    }
    let pool = TargetPool::from_fight(&fight);
    let mut catalog = SkillEffectCatalog::default();
    catalog.insert(ParsedSkillEffect {
        skill_id: 30030111,
        slots: vec![],
    });
    catalog.insert_damage_rate(30030111, 1000);
    catalog.insert_logic_target(30030111, 1);
    catalog.insert_effect_tag(30030111, 1);
    let target = if owner > 0 { -1 } else { 10 };
    let before = managers.hp.current(target);
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: owner,
        skill_id: 30030111,
    }
    .into();
    invocation.mode = SkillExecutionMode::Active;
    invocation.card_index = if extra { 0 } else { 1 };
    if !extra {
        managers.card = CardManager::new(vec![CardInfo {
            uid: Some(owner),
            skill_id: Some(30030111),
            card_type: Some(0),
            ..Default::default()
        }]);
        managers
            .execute_card(CardCommand::Play(CardPlay {
                origin: ORIGIN,
                hand_index: 0,
                target_uid: Some(target),
                chosen_skill_id: None,
                choice: None,
                recorded_skill: None,
            }))
            .unwrap();
    }
    invocation.target = SkillTarget::Explicit(target);
    drain::run_skill(
        &mut managers,
        &pool,
        &catalog,
        &mut RoundDeterminism::with_seed(42),
        TargetContext {
            current_round: 1,
            ..Default::default()
        },
        invocation,
        SkillModifiers::default(),
    )
    .unwrap();
    assert_eq!(managers.attribute.get(owner, AttrId::DmgBonus), 0);
    before - managers.hp.current(target)
}

#[test]
fn actual_bullet_carrier_only_benefits_its_current_friendly_caster() {
    assert_eq!(damage(10, None, true, false), 100);
    assert_eq!(damage(10, Some(11), true, false), 100);
    assert_eq!(damage(10, Some(10), true, false), 160);
    assert_eq!(damage(10, Some(10), false, false), 100);
    assert_eq!(damage(-1, Some(-1), true, false), 100);
    assert_eq!(damage(10, Some(10), true, false), 160);
}

#[test]
fn non_extra_action_excludes_a_real_extra_skill_even_when_its_owner_has_a_bullet() {
    assert_eq!(damage(10, Some(10), true, true), 100);
    assert_eq!(damage(10, Some(10), true, false), 160);
}
