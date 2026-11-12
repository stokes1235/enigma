use super::*;
use crate::engine::manager::card::CardManager;
use sonettobuf::{CardEnchant, CardInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn enemy(uid: i64, attack: i32, team: i32) -> FightEntityInfo {
    let attr = HeroAttribute {
        hp: Some(1000),
        attack: Some(attack),
        defense: Some(50),
        mdefense: Some(60),
        ..Default::default()
    };
    FightEntityInfo {
        uid: Some(uid),
        model_id: Some(801110301),
        entity_type: Some(2),
        team_type: Some(team),
        current_hp: Some(750),
        attr: Some(attr),
        base_attr: Some(attr),
        ..Default::default()
    }
}

fn fixture(profile: Option<Difficulty>) -> (BattleManagers, TargetPool) {
    crate::test_support::init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![enemy(10, 100, 1)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![enemy(-1, 150, 2)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            difficulty: profile,
            ..Default::default()
        },
    );
    configure(&mut managers);
    (managers, TargetPool::from_fight(&fight))
}

#[test]
fn frozen_initial_enemy_is_not_scaled_twice_and_new_enemy_gets_the_same_baseline() {
    let profile = Difficulty {
        enemy_attributes: vec![(102, 500), (205, 150), (207, 100)],
        initial_enemy_uids: vec![-1],
        ..Default::default()
    };
    let (mut managers, _) = fixture(Some(profile));
    managers.attribute.add(-1, AttrId::DmgBonus, 150); // frozen Ex seed
    let initial_add = managers.attribute.get(-1, AttrId::DmgBonus);
    register_enemy(&mut managers, -1);
    assert_eq!(managers.attribute.base(-1, AttrId::Attack), 150);
    assert_eq!(managers.attribute.get(-1, AttrId::DmgBonus), initial_add);
    managers.register_entity(&enemy(-2, 100, 2));
    register_enemy(&mut managers, -2);
    assert_eq!(managers.attribute.base(-2, AttrId::Attack), 150);
    assert_eq!(managers.attribute.get(-2, AttrId::DmgBonus), initial_add);
    assert_eq!(managers.attribute.get(-2, AttrId::FinalDmgBonus), 100);
    assert_eq!(managers.hp.current(-2), 750);
    register_enemy(&mut managers, -2);
    assert_eq!(managers.attribute.base(-2, AttrId::Attack), 150);
    assert_eq!(managers.attribute.get(-2, AttrId::DmgBonus), initial_add);
    managers.register_entity(&enemy(11, 100, 1));
    register_enemy(&mut managers, 11);
    assert_eq!(managers.attribute.base(11, AttrId::Attack), 100);
}

#[test]
fn rebase_preserves_explicit_extended_and_special_seed_values() {
    let (mut managers, _) = fixture(Some(Difficulty {
        enemy_attributes: vec![(102, 500), (205, 150), (208, 100)],
        initial_enemy_uids: vec![-1],
        ..Default::default()
    }));
    let fresh = enemy(-3, 100, 2);
    let catalog = managers.catalog();
    managers.entity.register(catalog, &fresh);
    managers.attribute.register_with_catalog(catalog, &fresh);
    managers.hp.register(&fresh);
    managers.attribute.add(-3, AttrId::CriticalRate, 99);
    managers.attribute.add(-3, AttrId::FinalDmgReduction, 80);
    let critical = managers.attribute.get(-3, AttrId::CriticalRate);
    register_enemy(&mut managers, -3);
    assert_eq!(managers.attribute.base(-3, AttrId::Attack), 150);
    assert_eq!(managers.attribute.get(-3, AttrId::CriticalRate), critical);
    assert_eq!(managers.attribute.get(-3, AttrId::FinalDmgReduction), 180);
}

#[test]
fn round_growth_starts_at_six_and_replayed_round_does_not_add_again() {
    let (mut managers, pool) = fixture(Some(Difficulty {
        attack_growth_start_round: 6,
        attack_growth_per_round: 50,
        initial_enemy_uids: vec![-1],
        ..Default::default()
    }));
    let mut random = RoundDeterminism::with_seed(42);
    assert!(round_start(&mut managers, &pool, 5, &mut random).is_empty());
    assert_eq!(managers.attribute.get(-1, AttrId::Attack), 0);
    round_start(&mut managers, &pool, 6, &mut random);
    assert_eq!(managers.attribute.get(-1, AttrId::Attack), 50);
    assert_eq!(managers.origin_attribute(-1, AttrId::Attack), 157);
    round_start(&mut managers, &pool, 6, &mut random);
    round_start(&mut managers, &pool, 4, &mut random);
    assert_eq!(managers.attribute.get(-1, AttrId::Attack), 50);
    round_start(&mut managers, &pool, 7, &mut random);
    assert_eq!(managers.attribute.get(-1, AttrId::Attack), 100);
    assert_eq!(managers.attribute.get(10, AttrId::Attack), 0);
}

#[test]
fn scalding_selects_distinct_eligible_cards_reproducibly_and_never_overwrites_a_surface() {
    let (mut managers, pool) = fixture(Some(Difficulty {
        scalding_cards: 2,
        ..Default::default()
    }));
    let card = |uid, skill| CardInfo {
        uid: Some(uid),
        skill_id: Some(skill),
        ..Default::default()
    };
    let mut frozen = card(10, 300301);
    frozen.enchants.push(CardEnchant {
        enchant_id: Some(10001),
        duration: Some(1),
        ..Default::default()
    });
    managers.card = CardManager::new(vec![
        frozen,
        card(10, 300302),
        card(10, 300303),
        card(-1, 100),
    ]);
    let mut again = managers.clone();
    let mut first_random = RoundDeterminism::with_seed(17);
    let mut second_random = RoundDeterminism::with_seed(17);
    let ops = round_start(&mut managers, &pool, 1, &mut first_random);
    assert_eq!(ops, round_start(&mut again, &pool, 1, &mut second_random));
    let [RuleOp::Command(BattleCommand::Card(CardCommand::EnchantHand(change)))] = ops.as_slice()
    else {
        panic!("expected one surface rewrite command")
    };
    assert_eq!(change.indices, vec![1, 2]);
    assert_eq!(change.enchant, EnchantedType::Burn);
    assert_eq!(change.duration, -1);
    assert!(round_start(&mut managers, &pool, 1, &mut first_random).is_empty());
}

#[test]
fn no_profile_preserves_old_battles_and_ap_afflatus_apply_only_to_enemies() {
    let (mut managers, pool) = fixture(None);
    let mut random = RoundDeterminism::with_seed(1);
    assert!(round_start(&mut managers, &pool, 6, &mut random).is_empty());
    register_enemy(&mut managers, -1);
    assert_eq!(managers.attribute.base(-1, AttrId::Attack), 150);
    assert_eq!(enemy_ap_bonus(&managers), 0);
    let (managers, _) = fixture(Some(Difficulty {
        extra_enemy_ap: 1,
        stronger_afflatus: 300,
        ..Default::default()
    }));
    assert_eq!(enemy_ap_bonus(&managers), 1);
    assert_eq!(stronger_afflatus_bonus(&managers, -1), 300);
    assert_eq!(stronger_afflatus_bonus(&managers, 10), 0);
}
