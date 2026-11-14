use super::*;
use crate::engine::event::payload::BuffChangeEvent;
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn setup() -> (BattleManagers, TargetPool) {
    crate::test_support::init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                team_type: Some(1),
                current_hp: Some(100),
                attr: Some(HeroAttribute {
                    hp: Some(100),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        defender: Some(FightTeam::default()),
        ..Default::default()
    };
    let mut managers = BattleManagers::seeded(&fight);
    configure(
        &mut managers,
        RougeContext {
            artefacts: vec![
                Artefact {
                    uid: 1,
                    item_id: 8130603,
                    ..Default::default()
                },
                Artefact {
                    uid: 2,
                    item_id: 8130604,
                    ..Default::default()
                },
            ],
            catalyst: Some(Catalyst {
                style_id: 1,
                coin: 10,
                power_limit: 100,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    (managers, TargetPool::from_fight(&fight))
}

#[test]
fn committed_buff_increase_awards_correct_coins_once_and_replays() {
    for (buff_id, amount) in [(90011, 1), (90001, 2)] {
        let (mut managers, pool) = setup();
        let change = BuffChangeEvent {
            source_uid: 10,
            target_uid: 10,
            buff_uid: 55,
            buff_id,
            before_amount: 0,
            after_amount: 1,
            act_id: 0,
            act_value: 0,
            duration_expired: false,
        };
        let added = BattleEvent::BuffAdded(change);
        let mut random = RoundDeterminism::with_seed(42);
        random.enqueue_permille_rolls([0]);
        assert!(!observe(&mut managers, &pool, &added, &mut random).is_empty());
        assert_eq!(managers.rouge.resources().unwrap().coin, 10 + amount);
        assert!(observe(&mut managers, &pool, &added, &mut random).is_empty());
        assert_eq!(managers.rouge.resources().unwrap().coin, 10 + amount);
    }
}
