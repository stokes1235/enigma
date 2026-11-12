//! Apply the supported numeric Rouge modifiers before constructing the battle
//! runtime, using the same AttrId -> protocol fields as AttributeManager.
use sonettobuf::{Fight, HeroExAttribute, HeroSpAttribute, RougeTeamInfo};
use std::collections::{BTreeMap, BTreeSet};

pub fn apply(
    fight: &mut Fight,
    ex: &mut Vec<(i64, HeroExAttribute)>,
    sp: &mut Vec<(i64, HeroSpAttribute)>,
    global: &BTreeMap<i32, i32>,
    slots: &BTreeMap<i32, BTreeMap<i32, i32>>,
    roster: &RougeTeamInfo,
) -> BTreeSet<i32> {
    let mut per_hero = Vec::new();
    let mut unsupported = BTreeSet::new();
    if let Some(team) = &mut fight.attacker {
        for entity in team.entitys.iter_mut().chain(team.sub_entitys.iter_mut()) {
            let mut rates = global.clone();
            if let Some(slot) = roster
                .battle_hero_list
                .iter()
                .find(|hero| hero.hero_id == entity.model_id)
                .and_then(|hero| hero.index)
                && let Some(extra) = slots.get(&slot)
            {
                for (id, value) in extra {
                    *rates.entry(*id).or_default() += value;
                }
            }
            if let Some(attr) = &mut entity.attr {
                for (id, stat) in [
                    (101, &mut attr.hp),
                    (102, &mut attr.attack),
                    (103, &mut attr.defense),
                    (104, &mut attr.mdefense),
                ] {
                    if let Some(value) = stat {
                        *value = (*value as i64
                            * (1000_i64 + rates.get(&id).copied().unwrap_or(0) as i64).max(0)
                            / 1000)
                            .clamp(0, i32::MAX as i64) as i32;
                    }
                }
            }
            if let Some(uid) = entity.uid {
                per_hero.push((uid, rates));
            }
        }
    }
    for (uid, rates) in per_hero {
        if !ex.iter().any(|(id, _)| *id == uid) {
            ex.push((uid, HeroExAttribute::default()));
        }
        if !sp.iter().any(|(id, _)| *id == uid) {
            sp.push((uid, HeroSpAttribute::default()));
        }
        let ex = &mut ex.iter_mut().find(|(id, _)| *id == uid).unwrap().1;
        let sp = &mut sp.iter_mut().find(|(id, _)| *id == uid).unwrap().1;
        for (id, value) in rates {
            let field = match id {
                101..=104 => continue,
                201 => &mut ex.cri,
                202 => &mut ex.recri,
                203 => &mut ex.cri_dmg,
                204 => &mut ex.cri_def,
                205 => &mut ex.add_dmg,
                206 => &mut ex.drop_dmg,
                207 => &mut sp.final_add_dmg,
                208 => &mut sp.final_drop_dmg,
                209 => &mut sp.revive,
                210 => &mut sp.absorb,
                211 => &mut sp.clutch,
                212 => &mut sp.heal,
                213 => &mut sp.defense_ignore,
                214 => &mut sp.normal_skill_rate,
                215 => &mut sp.play_add_rate,
                216 => &mut sp.play_drop_rate,
                218 => &mut sp.rebound_dmg,
                219 => &mut sp.extra_dmg,
                220 => &mut sp.reuse_dmg,
                231 => &mut sp.device_skill_rate,
                _ => {
                    unsupported.insert(id);
                    continue;
                }
            };
            *field = Some(field.unwrap_or_default().saturating_add(value));
        }
    }
    unsupported
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combines_rates_once_and_sends_ex_and_sp_to_only_allies() {
        let entity = |uid, id| sonettobuf::FightEntityInfo {
            uid: Some(uid),
            model_id: Some(id),
            attr: Some(sonettobuf::HeroAttribute {
                hp: Some(1000),
                attack: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut fight = Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![entity(1, 3003), entity(2, 3004)],
                ..Default::default()
            }),
            defender: Some(sonettobuf::FightTeam {
                entitys: vec![entity(-1, 2001)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let roster = RougeTeamInfo {
            battle_hero_list: vec![sonettobuf::RougeBattleHero {
                index: Some(1),
                hero_id: Some(3003),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut ex = vec![(
            1,
            HeroExAttribute {
                cri: Some(100),
                ..Default::default()
            },
        )];
        let mut sp = vec![];
        let unsupported = apply(
            &mut fight,
            &mut ex,
            &mut sp,
            &BTreeMap::from([
                (101, 100),
                (102, 200),
                (201, 50),
                (207, 80),
                (212, 40),
                (217, 70),
            ]),
            &BTreeMap::from([(1, BTreeMap::from([(101, 200)]))]),
            &roster,
        );
        assert_eq!(
            fight.attacker.as_ref().unwrap().entitys[0]
                .attr
                .as_ref()
                .unwrap()
                .hp,
            Some(1300)
        );
        assert_eq!(
            fight.attacker.as_ref().unwrap().entitys[1]
                .attr
                .as_ref()
                .unwrap()
                .hp,
            Some(1100)
        );
        assert_eq!(
            fight.attacker.as_ref().unwrap().entitys[0]
                .attr
                .as_ref()
                .unwrap()
                .attack,
            Some(120)
        );
        assert_eq!(
            fight.defender.as_ref().unwrap().entitys[0]
                .attr
                .as_ref()
                .unwrap()
                .hp,
            Some(1000)
        );
        assert_eq!(ex[0].1.cri, Some(150));
        assert_eq!(sp[0].1.final_add_dmg, Some(80));
        assert_eq!(sp[0].1.heal, Some(40));
        assert_eq!(unsupported, BTreeSet::from([217]));
    }
}
