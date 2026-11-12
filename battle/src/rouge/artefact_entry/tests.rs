use super::*;
use crate::{
    Battle,
    catalog::BattleCatalog,
    dungeon::BuiltFight,
    engine::{
        runtime::{determinism::RoundDeterminism, drain},
        skill::{
            action::{SkillInvocation, SkillModifiers, SkillRequest, SkillTarget},
            effect::SkillEffectCatalog,
            target::TargetContext,
        },
    },
    rouge::{Artefact, RougeContext},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

const MEDICATIONS: [i32; 12] = [
    8150006, 8150008, 8150010, 8150012, 8150014, 8150016, 8250007, 8250009, 8250011, 8250013,
    8250015, 8250017,
];

fn fight() -> Fight {
    crate::test_support::init_config();
    let entity = |uid: i64, hp: i32, max_hp: i32, position: i32| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(1001),
        position: Some(position),
        current_hp: Some(hp),
        attr: Some(HeroAttribute {
            hp: Some(max_hp),
            attack: Some(100),
            ..Default::default()
        }),
        skill_group1: if uid > 0 { vec![101] } else { vec![] },
        skill_group2: if uid > 0 { vec![201] } else { vec![] },
        ..Default::default()
    };
    Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 500, 1000, 1), entity(11, 500, 1000, 2)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 5000, 5000, 1), entity(-2, 5000, 5000, 2)],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    }
}

fn context(items: &[i32], entry: EntryContext) -> RougeContext {
    RougeContext {
        artefacts: items
            .iter()
            .enumerate()
            .map(|(index, id)| Artefact {
                uid: index as i64 + 100,
                item_id: *id,
                ..Default::default()
            })
            .collect(),
        entry: Some(entry),
        ..Default::default()
    }
}

fn fixture(
    context: RougeContext,
) -> (
    BattleManagers,
    TargetPool,
    SkillEffectCatalog,
    RoundDeterminism,
) {
    let fight = fight();
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(&mut managers, context);
    (
        managers,
        TargetPool::from_fight(&fight),
        SkillEffectCatalog::default(),
        RoundDeterminism::with_seed(42),
    )
}

fn settle(
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

#[test]
fn fixed_entry_buffs_have_real_stacks_targets_and_current_ids() {
    let entry = EntryContext::default();
    let (mut managers, pool, catalog, mut random) = fixture(context(
        &[
            8140401, 8240402, 8140701, 8240702, 8140501, 8240502, 8230042, 8320025,
        ],
        entry.clone(),
    ));
    let ops = opening(&managers, &pool, &entry);
    settle(&mut managers, &pool, &catalog, &mut random, ops);
    for uid in [10, 11] {
        assert_eq!(managers.buff.buff_id_amount(uid, 90001), 3);
        assert_eq!(managers.buff.buff_id_amount(uid, 90051), 6);
        assert_eq!(managers.buff.buff_id_amount(uid, 90201), 1);
        assert_eq!(managers.buff.buff_id_amount(uid, 90171), 2);
        assert_eq!(managers.buff.buff_id_amount(uid, 90031), 2);
        assert!(!managers.buff.has_buff_id(uid, 90101));
    }
    for uid in [-1, -2] {
        assert_eq!(managers.buff.buff_id_amount(uid, 90101), 12);
        assert!(!managers.buff.has_buff_id(uid, 90001));
    }
}

#[test]
fn random_entry_sets_prefer_missing_effects_and_are_replay_stable() {
    let entry = EntryContext {
        seed: 7,
        ..Default::default()
    };
    let (mut managers, pool, catalog, mut random) =
        fixture(context(&[8140401, 8230019, 8330607], entry.clone()));
    let ops = opening(&managers, &pool, &entry);
    // A deterministic seed and frozen context produce byte-identical choices.
    assert_eq!(
        format!("{ops:?}"),
        format!("{:?}", opening(&managers, &pool, &entry))
    );
    settle(&mut managers, &pool, &catalog, &mut random, ops);
    for uid in [10, 11] {
        let buffs = [90001, 90011, 90041, 90021, 90031, 90281];
        assert!(buffs.iter().all(|id| managers.buff.has_buff_id(uid, *id)));
        assert_eq!(
            buffs
                .iter()
                .map(|id| managers.buff.buff_id_amount(uid, *id))
                .sum::<i32>(),
            8
        );
        assert!(!managers.buff.has_buff_id(uid, 90061));
    }
}

#[test]
fn mirror_caps_its_own_rate_and_preserves_existing_shields() {
    assert_eq!(coin_shield_rate(49, 0), 100);
    assert_eq!(coin_shield_rate(50, 0), 110);
    assert_eq!(coin_shield_rate(40, 10), 110);
    assert_eq!(coin_shield_rate(30, 20), 110);
    assert_eq!(coin_shield_rate(50_000, 0), 500);
    assert_eq!(coin_shield_rate(50_000, 50), 500);
    let entry = EntryContext {
        coin: 200,
        ..Default::default()
    };
    let (mut managers, pool, catalog, mut random) =
        fixture(context(&[8120610, 8120610], entry.clone()));
    let ops = opening(&managers, &pool, &entry);
    settle(&mut managers, &pool, &catalog, &mut random, ops);
    assert_eq!(managers.hp.shield(10), 280);
    assert_eq!(managers.hp.shield(11), 280);
    assert!(managers.buff.has_buff_id(10, 812061001));
    assert!(!managers.buff.has_buff_id(-1, 812061001));
}

#[test]
fn new_art_box_charges_only_when_it_can_give_the_unstackable_protection() {
    let mut prepared = context(&[8220613, 8220613], EntryContext::default());
    prepared.catalyst = Some(crate::rouge::Catalyst {
        coin: 50,
        power: 0,
        power_limit: 40,
        ..Default::default()
    });
    let (mut managers, pool, catalog, mut random) = fixture(prepared.clone());
    let ops = paid_opening(&mut managers, &pool);
    settle(&mut managers, &pool, &catalog, &mut random, ops);
    assert!(managers.buff.has_buff_id(10, 822061301));
    assert!(!managers.buff.has_buff_id(11, 822061301));
    let resources = managers.rouge.resources().unwrap();
    assert_eq!((resources.coin, resources.spent_coin), (0, 50));

    prepared.catalyst.as_mut().unwrap().coin = 49;
    let (mut managers, pool, _, _) = fixture(prepared);
    assert!(paid_opening(&mut managers, &pool).is_empty());
    let resources = managers.rouge.resources().unwrap();
    assert_eq!((resources.coin, resources.spent_coin), (49, 0));
}

fn start(entry: Option<EntryContext>) -> Battle {
    start_context(entry.map(|entry| context(&[], entry)))
}

fn start_context(context: Option<RougeContext>) -> Battle {
    start_fight(fight(), context)
}

fn start_fight(fight: Fight, context: Option<RougeContext>) -> Battle {
    Battle::start(
        BattleCatalog::new(crate::test_support::game_data()),
        BuiltFight {
            fight,
            ex_attributes: vec![],
            sp_attributes: vec![],
            rouge: context,
            battle_rule_skills: vec![],
        },
        42,
    )
    .unwrap()
}

#[test]
fn bone_medications_use_each_recipients_max_hp_through_real_hand_play() {
    use sonettobuf::{BeginRoundOper, BeginRoundRequest};
    for item_id in [8150012, 8250013] {
        let skill_id = medication_skill_id(item_id).unwrap();
        let mut fight = fight();
        let second = &mut fight.attacker.as_mut().unwrap().entitys[1];
        second.attr.as_mut().unwrap().hp = Some(2_400);
        second.current_hp = Some(400);
        let entry = EntryContext {
            medication_cards: vec![MedicationCard {
                source_uid: 100,
                item_id,
                skill_id,
                count: 1,
            }],
            ..Default::default()
        };
        let mut battle = start_fight(fight, Some(context(&[], entry)));
        let hand = battle.reconnect_state().1.unwrap().team_a_cards1;
        let index = hand
            .iter()
            .position(|card| card.skill_id == Some(skill_id))
            .unwrap();
        crate::dungeon::begin_round(
            &mut battle,
            BeginRoundRequest {
                opers: vec![BeginRoundOper {
                    oper_type: Some(crate::engine::manager::card::CardOpType::PlayCard as i32),
                    param1: Some(index as i32 + 1),
                    to_id: Some(11),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(battle.entity_info(11).unwrap().shield_value, Some(1_200));
        assert_eq!(
            battle
                .entity_info(10)
                .unwrap()
                .shield_value
                .unwrap_or_default(),
            if item_id == 8250013 { 500 } else { 0 }
        );
    }
}

#[test]
fn all_twelve_medications_can_be_selected_from_the_started_hand_and_reach_real_effects() {
    use sonettobuf::{BeginRoundOper, BeginRoundRequest};
    for item_id in MEDICATIONS {
        let skill_id = medication_skill_id(item_id).unwrap();
        let entry = EntryContext {
            medication_cards: vec![MedicationCard {
                source_uid: 100,
                item_id,
                skill_id,
                count: 1,
            }],
            ..Default::default()
        };
        let mut frozen = context(&[], entry);
        frozen.catalyst = Some(crate::rouge::Catalyst {
            style_id: 0,
            power: 1,
            power_limit: 40,
            ..Default::default()
        });
        let mut battle = start_context(Some(frozen));
        let initial = battle.reconnect_state().1.unwrap();
        let index = initial
            .team_a_cards1
            .iter()
            .position(|card| card.uid == Some(0) && card.skill_id == Some(skill_id))
            .unwrap_or_else(|| panic!("item {item_id} was lost during opening draw"));
        let target = if matches!(item_id, 8150010 | 8250011) {
            -1
        } else {
            10
        };
        let reply = crate::dungeon::begin_round(
            &mut battle,
            BeginRoundRequest {
                opers: vec![BeginRoundOper {
                    oper_type: Some(crate::engine::manager::card::CardOpType::PlayCard as i32),
                    param1: Some(index as i32 + 1),
                    to_id: Some(target),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("item {item_id}: {error}"));
        assert!(
            reply
                .round
                .unwrap()
                .fight_step
                .iter()
                .any(|step| step.from_id == Some(0) && step.act_id == Some(skill_id)),
            "item {item_id} did not execute as the real scene-owned special card"
        );
        let ally = battle.entity_info(10).unwrap();
        let other = battle.entity_info(11).unwrap();
        let has = |entity: &FightEntityInfo, buff_id| {
            entity
                .buffs
                .iter()
                .any(|buff| buff.buff_id == Some(buff_id))
        };
        match item_id {
            8150006 => {
                assert_eq!(ally.current_hp, Some(750));
                assert_eq!(other.current_hp, Some(500));
            }
            8250007 => {
                assert_eq!(ally.current_hp, Some(750));
                assert_eq!(other.current_hp, Some(750));
            }
            8150008 | 8250009 => assert_eq!(
                battle.rouge_resources().unwrap().power,
                if item_id == 8150008 { 6 } else { 11 }
            ),
            8150010 | 8250011 => {
                assert!(has(&battle.entity_info(-1).unwrap(), 90384));
                assert!(has(&battle.entity_info(-1).unwrap(), 90154));
                assert_eq!(
                    has(&battle.entity_info(-2).unwrap(), 90154),
                    item_id == 8250011
                );
                assert!(!has(&ally, 90154));
            }
            8150012 | 8250013 => {
                assert_eq!(ally.shield_value, Some(500));
                assert_eq!(
                    other.shield_value.unwrap_or_default(),
                    if item_id == 8250013 { 500 } else { 0 }
                );
            }
            8150014 | 8250015 => {
                for id in [90001, 90011, 90051, 90041] {
                    assert!(has(&ally, id));
                    assert_eq!(has(&other, id), item_id == 8250015);
                }
            }
            8150016 | 8250017 => {
                assert!(has(&ally, 5025));
                assert_eq!(has(&other, 5025), item_id == 8250017);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn battle_start_keeps_normal_opening_and_adds_all_twelve_real_medication_cards_after_draw() {
    let baseline = start(None);
    let entry = EntryContext {
        medication_cards: MEDICATIONS
            .into_iter()
            .enumerate()
            .map(|(index, id)| MedicationCard {
                source_uid: 100 + index as i64,
                item_id: id,
                skill_id: medication_skill_id(id).unwrap(),
                count: 2,
            })
            .collect(),
        ..Default::default()
    };
    let with_cards = start(Some(entry.clone()));
    let replayed = start(Some(
        serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap(),
    ));
    let hand = with_cards.reconnect_state().1.unwrap().team_a_cards1;
    let normal: Vec<_> = hand
        .iter()
        .filter(|card| card.uid != Some(0))
        .cloned()
        .collect();
    assert_eq!(normal, baseline.reconnect_state().1.unwrap().team_a_cards1);
    for item_id in MEDICATIONS {
        let cards: Vec<_> = hand
            .iter()
            .filter(|card| card.skill_id == medication_skill_id(item_id))
            .collect();
        assert_eq!(cards.len(), 2, "item {item_id}");
        assert!(cards.iter().all(|card| card.uid == Some(0)
            && card.card_type == Some(CardType::RougeSp as i32)
            && card.temp_card == Some(false)
            && card.hero_id == Some(0)));
    }
    assert_eq!(with_cards.reconnect_state(), replayed.reconnect_state());
    assert_eq!(with_cards.reconnect_state(), with_cards.reconnect_state());
}

fn cast_medication(
    item_id: i32,
    target_uid: i64,
    before: impl FnOnce(&mut BattleManagers),
) -> BattleManagers {
    let (mut managers, pool, mut catalog, mut random) =
        fixture(context(&[], EntryContext::default()));
    before(&mut managers);
    let skill_id = medication_skill_id(item_id).unwrap();
    BattleCatalog::new(crate::test_support::game_data()).extend_skill_roots(
        &mut catalog,
        [skill_id],
        [],
    );
    let mut invocation: SkillInvocation = SkillRequest {
        source_uid: 0,
        skill_id,
    }
    .into();
    invocation.target = SkillTarget::Explicit(target_uid);
    invocation.card_index = 1;
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
    managers
}

#[test]
fn current_painkiller_and_bone_card_skills_apply_real_target_and_team_effects() {
    let single = cast_medication(8150006, 10, |_| {});
    assert_eq!(single.hp.current(10), 750);
    assert_eq!(single.hp.current(11), 500);
    let team = cast_medication(8250007, 10, |_| {});
    assert_eq!(team.hp.current(10), 750);
    assert_eq!(team.hp.current(11), 750);
    let single = cast_medication(8150012, 10, |_| {});
    assert_eq!(single.hp.shield(10), 500);
    assert_eq!(single.hp.shield(11), 0);
    let team = cast_medication(8250013, 10, |_| {});
    assert_eq!(team.hp.shield(10), 500);
    assert_eq!(team.hp.shield(11), 500);
}

#[test]
fn current_mistletoe_and_belladonna_card_skills_use_current_buff_and_doubling_rules() {
    let single = cast_medication(8150014, 10, |managers| {
        managers
            .execute_buff(BuffCommand::Grant(BuffGrant {
                origin: ORIGIN,
                source_uid: 10,
                target_uid: 10,
                buff_id: 90001,
                amount: Some(1),
                occurrences: 1,
                child_uid_reservations: 0,
            }))
            .unwrap();
    });
    assert_eq!(single.buff.buff_id_amount(10, 90001), 5);
    assert_eq!(single.buff.buff_id_amount(10, 90011), 2);
    assert!(!single.buff.has_buff_id(11, 90001));
    let team = cast_medication(8250015, 10, |_| {});
    for uid in [10, 11] {
        for buff_id in [90001, 90011, 90051, 90041] {
            assert_eq!(team.buff.buff_id_amount(uid, buff_id), 2);
        }
    }
    let single = cast_medication(8150016, 10, |_| {});
    assert!(single.buff.has_buff_id(10, 5025));
    assert!(!single.buff.has_buff_id(11, 5025));
    let team = cast_medication(8250017, 10, |_| {});
    assert!(team.buff.has_buff_id(10, 5025));
    assert!(team.buff.has_buff_id(11, 5025));
}

#[test]
fn current_toxin_card_skills_apply_five_instances_to_the_actual_selected_side() {
    let single = cast_medication(8150010, -1, |_| {});
    assert_eq!(single.buff.buff_id_amount(-1, 90384), 5);
    assert_eq!(single.buff.buff_id_amount(-1, 90154), 5);
    assert!(!single.buff.has_buff_id(-2, 90384));
    let team = cast_medication(8250011, -1, |_| {});
    for uid in [-1, -2] {
        assert_eq!(team.buff.buff_id_amount(uid, 90384), 5);
        assert_eq!(team.buff.buff_id_amount(uid, 90154), 5);
    }
    assert!(!team.buff.has_buff_id(10, 90384));
}

#[test]
fn card_generation_rejects_mismatched_skill_ids_and_duplicate_instance_sources() {
    let entry = EntryContext {
        medication_cards: vec![
            MedicationCard {
                source_uid: 1,
                item_id: 8150006,
                skill_id: 815000611,
                count: 1,
            },
            MedicationCard {
                source_uid: 1,
                item_id: 8150006,
                skill_id: 815000611,
                count: 2,
            },
            MedicationCard {
                source_uid: 2,
                item_id: 8150006,
                skill_id: 90001,
                count: 1,
            },
        ],
        ..Default::default()
    };
    assert_eq!(opening_cards(&entry).len(), 1);
    assert_eq!(medication_skill_ids(&entry), vec![815000611]);
}
