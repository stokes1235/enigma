use super::*;
use crate::engine::{
    entity::attr::AttrId,
    skill::rule::{DefinitionKey, RuleDomain},
};
use sonettobuf::{Fight, FightEntityInfo, FightTeam, HeroAttribute};

fn managers() -> BattleManagers {
    crate::test_support::init_config();
    BattleManagers::seeded(&Fight {
        attacker: Some(FightTeam {
            entitys: [(10, 1_000), (11, 2_400)]
                .into_iter()
                .map(|(uid, hp)| FightEntityInfo {
                    uid: Some(uid),
                    current_hp: Some(hp),
                    attr: Some(HeroAttribute {
                        hp: Some(hp),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn bone(source_uid: i64, target_uid: i64) -> ShieldCommand {
    ShieldCommand {
        origin: CommandOrigin {
            domain: RuleDomain::Behavior,
            key: DefinitionKey::new(1, "AddBuff"),
        },
        source_uid,
        target_uid,
        buff_id: 815001211,
        amount_attr: AttrId::Hp,
        amount_rate: 500,
        multiplier_bonus: None,
        max_attr: AttrId::Hp,
        max_rate: 500,
        scope: ShieldScope::Entity,
        carrier_uid: ShieldCarrierUid::Definition,
    }
}

#[test]
fn scene_owned_bone_uses_each_recipients_hp_and_keeps_scene_attribution() {
    let mut managers = managers();
    for (uid, expected) in [(10, 500), (11, 1_200)] {
        let changes = execute(&mut managers, bone(0, uid)).unwrap();
        assert_eq!(managers.hp.shield(uid), expected);
        assert_eq!(changes.command.source_uid, 0);
        assert_eq!(changes.hp.unwrap().shield_granted.unwrap().added, expected);
    }
}

#[test]
fn scene_bone_refreshes_existing_carrier_without_growing_beyond_its_cap() {
    let mut managers = managers();
    execute(&mut managers, bone(0, 11)).unwrap();
    managers.hp.set_shield(11, 200);
    execute(&mut managers, bone(0, 11)).unwrap();
    assert_eq!(managers.hp.shield(11), 1_200);
    assert_eq!(
        managers
            .buff
            .active_for(11)
            .filter(|buff| buff.buff_id == Some(815001211))
            .count(),
        1
    );
}

#[test]
fn entity_owned_shields_keep_the_original_casters_attribute_basis() {
    let mut managers = managers();
    execute(&mut managers, bone(10, 11)).unwrap();
    assert_eq!(managers.hp.shield(11), 500);
}
