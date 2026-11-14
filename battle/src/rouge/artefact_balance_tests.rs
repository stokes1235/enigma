use super::*;
use sonettobuf::{Fight, FightTeam};

fn managers(seed: u64, count2: i32, coin: i32) -> BattleManagers {
    crate::test_support::init_config();
    let fight = Fight {
        attacker: Some(FightTeam::default()),
        defender: Some(FightTeam::default()),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    super::super::configure(
        &mut managers,
        RougeContext {
            artefacts: vec![Artefact {
                uid: 17,
                item_id: 8210611,
                count2,
                ..Default::default()
            }],
            entry: Some(EntryContext {
                seed,
                ..Default::default()
            }),
            catalyst: Some(Catalyst {
                style_id: 1,
                coin,
                power_limit: 100,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    managers
}

#[test]
fn loss_clamps_to_balance_and_round_boundaries_are_idempotent() {
    let seed = (0..1000)
        .find(|seed| draw(*seed, 17, 1, 0x13_8210611) < 500)
        .unwrap();
    let mut a = managers(seed, 0, 6);
    let mut b = managers(seed, 0, 6);
    let ops = round(&mut a, 1, true);
    assert!(!ops.is_empty());
    assert_eq!(a.rouge.resources().unwrap().coin, 0);
    assert_eq!(a.rouge.resources().unwrap().spent_coin, 6);
    assert!(round(&mut a, 1, true).is_empty());
    assert!(round(&mut a, 6, true).is_empty());
    assert_eq!(format!("{ops:?}"), format!("{:?}", round(&mut b, 1, true)));
    assert_eq!(a.rouge.resources(), b.rouge.resources());
}

#[test]
fn jackpot_is_independent_of_regular_gain_and_is_not_repeated() {
    let seed = (0..100000)
        .find(|seed| {
            draw(*seed, 17, 1, 0x27_8210611) >= 500 && draw(*seed, 17, 1, 0x31_8210611) < 10
        })
        .unwrap();
    let mut a = managers(seed, 0, 25);
    round(&mut a, 1, false);
    assert_eq!(a.rouge.resources().unwrap().coin, 691);
    assert!(round(&mut a, 1, false).is_empty());
    assert!(round(&mut a, 6, false).is_empty());
    assert_eq!(a.rouge.resources().unwrap().coin, 691);
}
