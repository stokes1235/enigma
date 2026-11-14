use super::*;
use crate::{Battle, catalog::BattleCatalog, dungeon::BuiltFight};
use sonettobuf::{
    ActEffect, BeginRoundOper, BeginRoundRequest, Fight, FightEntityInfo, FightTeam, HeroAttribute,
    effect_type_enum::EffectType,
};

fn fixture(ids: Option<&[i32]>) -> Battle {
    crate::test_support::init_config();
    let ally = |uid: i64, position: i32| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(3003),
        position: Some(position),
        current_hp: Some(10_000),
        attr: Some(HeroAttribute {
            hp: Some(10_000),
            attack: Some(100),
            ..Default::default()
        }),
        skill_group1: vec![30030111, 30030112, 30030113],
        skill_group2: vec![30030121, 30030122, 30030123],
        ..Default::default()
    };
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: (0..4).map(|i| ally(10 + i64::from(i), i + 1)).collect(),
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                position: Some(1),
                model_id: Some(1001),
                current_hp: Some(1_000_000),
                attr: Some(HeroAttribute {
                    hp: Some(1_000_000),
                    attack: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        version: Some(7),
        cur_round: Some(1),
        max_round: Some(20),
        ..Default::default()
    };
    let rouge = ids.map(|ids| RougeContext {
        artefacts: ids
            .iter()
            .enumerate()
            .map(|(index, item_id)| Artefact {
                uid: index as i64 + 100,
                item_id: *item_id,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    Battle::start(
        BattleCatalog::new(crate::test_support::game_data()),
        BuiltFight {
            fight,
            ex_attributes: vec![],
            sp_attributes: vec![],
            rouge,
            battle_rule_skills: vec![],
        },
        42,
    )
    .unwrap()
}

fn collect_limit_effects<'a>(effect: &'a ActEffect, result: &mut Vec<&'a ActEffect>) {
    if effect.effect_type == Some(EffectType::Addcardlimit as i32) {
        result.push(effect);
    }
    if let Some(step) = &effect.fight_step {
        for child in &step.act_effect {
            collect_limit_effects(child, result);
        }
    }
}

#[test]
fn real_opening_draws_respect_stacking_and_publish_the_absolute_limit() {
    let cases: &[(Option<&[i32]>, usize)] = &[
        (None, 8),
        (Some(&[]), 8),
        (Some(&[8140301]), 9),
        (Some(&[8140301, 8140301]), 10),
        (Some(&[8240302]), 10),
        (Some(&[8240302, 8240302]), 10),
        (Some(&[8140301, 8240302, 8240302]), 11),
        // An existing persistent powder-box bonus is part of the absolute
        // limit sent to the client, not overwritten by the opening bonus.
        (Some(&[8230305, 8240302]), 11),
    ];
    for &(ids, expected) in cases {
        let battle = fixture(ids);
        let round = battle.reconnect_state().1.unwrap();
        assert_eq!(battle.card_info_push().card_group.len(), expected, "{ids:?}");
        assert_eq!(round.team_a_cards1.len(), expected, "{ids:?}");
        let mut effects = Vec::new();
        for step in &round.fight_step {
            for effect in &step.act_effect {
                collect_limit_effects(effect, &mut effects);
            }
        }
        if ids.is_some_and(|items| items.iter().any(|id| [8140301, 8240302].contains(id))) {
            assert_eq!(effects.len(), 1, "{ids:?}");
            assert_eq!(effects[0].target_id, Some(10));
            assert_eq!(effects[0].effect_num, Some(expected as i32));
            assert_eq!(effects[0].config_effect, Some(40007));
        } else {
            assert!(effects.is_empty(), "{ids:?}");
        }
    }
}

#[test]
fn opening_bonus_expires_before_the_next_real_round_refill() {
    let mut battle = fixture(Some(&[8240302]));
    let initial = battle.card_info_push().card_group;
    assert_eq!(initial.len(), 10);
    // Consume three real cards backwards, leaving seven. The next round must
    // refill to the ordinary limit eight rather than the opening limit ten.
    let opers = (0..3)
        .rev()
        .map(|index| {
            let card = &initial[index];
            BeginRoundOper {
                oper_type: Some(crate::engine::manager::card::CardOpType::PlayCard as i32),
                param1: Some(index as i32 + 1),
                to_id: Some(if card.skill_id == Some(30030111) {
                    -1
                } else {
                    card.uid.unwrap()
                }),
                ..Default::default()
            }
        })
        .collect();
    crate::dungeon::begin_round(
        &mut battle,
        BeginRoundRequest {
            opers,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(battle.current_round(), 2);
    assert_eq!(battle.card_info_push().card_group.len(), 8);
}
