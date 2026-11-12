use anyhow::{Result, ensure};
use battle::dungeon::{
    BattleFighter, BattleRoster, BattleRosterPlan, BuiltFight, EquipmentBuildInput, FightOptions,
    HeroBuildInput,
};
use database::models::game::{
    equipment::{Equipment, EquipmentModel, UserEquipmentModel},
    heros::{HeroData, HeroModel, UserHeroModel, get_hero_by_uid},
};
use sonettobuf::FightGroup;
use sqlx::SqlitePool;

pub async fn build_fight(
    db: &SqlitePool,
    player_id: i64,
    episode_id: i32,
    battle_id: i32,
    fight_group: &FightGroup,
    options: FightOptions,
    params: Option<&str>,
) -> Result<BuiltFight> {
    let catalog = battle::catalog::BattleCatalog::new(config::configs::get());
    let plan = BattleRosterPlan::configured(
        catalog,
        episode_id,
        battle_id,
        options.is_balance,
        fight_group,
        params,
    )?;
    let roster = load_roster(db, player_id, &plan, fight_group).await?;
    let mut built = battle::dungeon::build_fight(
        catalog,
        &roster,
        episode_id,
        battle_id,
        fight_group,
        options,
        params,
    )?;
    if config::configs::get()
        .episode
        .get(episode_id)
        .is_some_and(|episode| episode.chapter_id == 2901)
    {
        let run = database::db::game::rouge::get_run(db, player_id, 1)
            .await?
            .ok_or_else(|| anyhow::anyhow!("missing Rouge run"))?;
        let team: sonettobuf::RougeTeamInfo = serde_json::from_str(&run.team)?;
        let start = database::db::game::rouge::get_or_create_start(db, player_id, 1).await?;
        let mut rates = crate::rouge::flow::attribute_rates(&start, &run);
        let bag: crate::rouge::bag::RunBag = serde_json::from_str(&run.bag)?;
        let effects = crate::rouge::artifact_rules::evaluate_battle_with_state(
            config::configs::get(),
            &bag,
            run.coin,
            &crate::rouge::flow::details(&run).artifact_state,
        );
        for (id, value) in &effects.attribute_rates {
            *rates.entry(*id).or_default() += value;
        }
        let mut protocol_rates = rates.clone();
        let genesis = protocol_rates.remove(&217).unwrap_or_default();
        let unsupported = crate::rouge::combat::apply(
            &mut built.fight,
            &mut built.ex_attributes,
            &mut built.sp_attributes,
            &protocol_rates,
            &effects.slot_attribute_rates,
            &team,
        );
        let mut context = battle::rouge::RougeContext::default();
        if let Some(attacker) = &built.fight.attacker {
            let actors: Vec<i64> = attacker
                .entitys
                .iter()
                .chain(&attacker.sub_entitys)
                .filter_map(|entity| entity.uid)
                .filter(|uid| *uid > 0)
                .collect();
            context.permanent_growth = crate::rouge::artifact_growth::initial_growth(
                &bag,
                &crate::rouge::flow::details(&run).artifact_state,
            );
            context.growth_sources = crate::rouge::artifact_growth::frozen_sources(&bag, &actors)
                .into_iter()
                .map(|source| battle::rouge::GrowthSource {
                    uid: source.uid,
                    item_id: source.item_id,
                    syntony_artefact_count: source.syntony_artefact_count,
                    rare_artefact_count: source.rare_artefact_count,
                    actor_uids: source.actor_uids,
                })
                .collect();
            let mut party = std::collections::BTreeSet::new();
            for slot in &team.battle_hero_list {
                if attacker
                    .entitys
                    .iter()
                    .chain(&attacker.sub_entitys)
                    .any(|entity| entity.model_id == slot.hero_id)
                {
                    if let Some(id) = slot.hero_id.filter(|id| *id > 0) {
                        party.insert(id);
                    }
                    if let Some(id) = slot.support_hero_id.filter(|id| *id > 0) {
                        party.insert(id);
                    }
                }
            }
            context.party_hero_ids = party.into_iter().collect();
        }
        let growth = crate::rouge::flow::details(&run)
            .outside_genius
            .unwrap_or_default();
        if growth.contains(&401) {
            context.incantation_rules = Some(battle::rouge::IncantationRules {
                syntony_level_limit: 5 + i32::from(growth.contains(&411)),
                resonance_level_limit: 5 + i32::from(growth.contains(&416)),
            });
        }
        if let Some(attacker) = &built.fight.attacker {
            for entity in attacker.entitys.iter().chain(&attacker.sub_entitys) {
                if let Some(uid) = entity.uid {
                    let slot = team
                        .battle_hero_list
                        .iter()
                        .find(|hero| hero.hero_id == entity.model_id)
                        .and_then(|hero| hero.index);
                    let amount = genesis
                        + slot
                            .and_then(|slot| effects.slot_attribute_rates.get(&slot))
                            .and_then(|rates| rates.get(&217))
                            .copied()
                            .unwrap_or_default();
                    if amount != 0 {
                        context.extra_attributes.push((uid, 217, amount));
                    }
                    if slot == Some(1) {
                        context.first_slot_uid = Some(uid);
                    }
                    if let Some(assignment) = team
                        .battle_hero_list
                        .iter()
                        .find(|hero| hero.hero_id == entity.model_id)
                    {
                        let support = assignment.support_hero_id.unwrap_or_default();
                        let group = assignment.support_hero_skill.unwrap_or_default();
                        let alive = team.hero_life_list.iter().any(|life| {
                            life.hero_id == Some(support) && life.life.unwrap_or_default() > 0
                        });
                        if support > 0 && matches!(group, 1 | 2) && alive {
                            let (rank, ex_level): (i32, i32) = sqlx::query_as(
                                "SELECT rank, ex_skill_level FROM heroes WHERE user_id = ? AND hero_id = ?",
                            ).bind(player_id).bind(support).fetch_one(db).await?;
                            let (first, second, _) = battle::hero::active_skills(
                                config::configs::get(),
                                support,
                                rank,
                                ex_level,
                            );
                            let skill_ids = if group == 1 { first } else { second };
                            if !skill_ids.is_empty() {
                                context.support_skills.push(battle::rouge::SupportSkill {
                                    owner_uid: uid,
                                    hero_id: support,
                                    skill_ids,
                                });
                            }
                        }
                    }
                }
            }
        }
        for item in bag
            .items
            .iter()
            .filter(|item| crate::rouge::artifact_rules::is_active(item))
        {
            let count2 = crate::rouge::artifact_rules::inlay_counters(config::configs::get(), item)
                .get("count2")
                .copied()
                .unwrap_or_default();
            let only_support_below = crate::rouge::artifact_rules::only_support_below(item, &bag);
            context.artefacts.push(battle::rouge::Artefact {
                uid: item.uid,
                item_id: item.item_id,
                count2,
                only_support_below,
            });
        }
        fn predicate(
            value: crate::rouge::artifact_rules::AttackPredicate,
        ) -> battle::rouge::AttackPredicate {
            use crate::rouge::artifact_rules::AttackPredicate as P;
            use battle::rouge::AttackPredicate as B;
            match value {
                P::RankAtLeast(rank) => B::RankAtLeast(rank),
                P::Support => B::Support,
                P::ExtraAction => B::ExtraAction,
                P::HasBullet => B::HasBullet,
                P::NonUltimate => B::NonUltimate,
                P::Ultimate => B::Ultimate,
                P::All(values) => B::All(values.into_iter().map(predicate).collect()),
            }
        }
        context.conditional_modifiers =
            crate::rouge::artifact_rules::attack_modifiers(config::configs::get(), &bag)
                .into_iter()
                .map(|rule| battle::rouge::ConditionalModifier {
                    source_uid: rule.source_uid,
                    item_id: rule.item_id,
                    predicate: predicate(rule.predicate),
                    attr_id: rule.attr_id,
                    value: rule.value,
                })
                .collect();
        if let Some(attacker) = &mut built.fight.attacker {
            // FightHelper uses the fourth '#' segment for support-card rank
            // transitions. Keys are the deployed owners; expose their selected
            // support group's actual replacement-aware skill sequence.
            let mut skills = serde_json::Map::new();
            for support in &context.support_skills {
                let group = team
                    .battle_hero_list
                    .iter()
                    .find(|slot| slot.support_hero_id == Some(support.hero_id))
                    .and_then(|slot| slot.support_hero_skill)
                    .unwrap_or(1);
                skills.insert(
                    support.owner_uid.to_string(),
                    if group == 2 {
                        serde_json::json!({"skill2": support.skill_ids})
                    } else {
                        serde_json::json!({"skill1": support.skill_ids})
                    },
                );
            }
            attacker.ex_team_str = Some(format!(
                "{}#{}#{}#{}",
                run.coin,
                run.power,
                run.power_limit,
                serde_json::Value::Object(skills)
            ));
        }
        built.rouge = Some(context);
        if !unsupported.is_empty() {
            tracing::debug!(?unsupported, "unsupported Rouge numeric attribute fields");
        }
        crate::rouge::flow::apply_team_life(&mut built.fight, &team)?;
    }
    Ok(built)
}

async fn load_roster(
    db: &SqlitePool,
    player_id: i64,
    plan: &BattleRosterPlan,
    fight_group: &FightGroup,
) -> Result<BattleRoster> {
    let heroes = UserHeroModel::new(player_id, db.clone());
    let mut fighters = Vec::new();
    for uid in plan.hero_uids().iter().copied() {
        let hero = heroes.get_uid(uid).await?;
        let equips = load_equips(db, &hero, fight_group).await?;
        fighters.push(BattleFighter {
            hero: hero_build_input(&hero),
            equips: equips.iter().map(equipment_build_input).collect(),
        });
    }

    Ok(BattleRoster {
        user_id: player_id,
        fighters,
        compose_support: load_compose_support(db, &heroes, plan).await?,
    })
}

async fn load_equips(
    db: &SqlitePool,
    hero: &HeroData,
    fight_group: &FightGroup,
) -> Result<Vec<Equipment>> {
    let requested = fight_group
        .equips
        .iter()
        .find(|equip| equip.hero_uid == Some(hero.record.uid))
        .map(|equip| equip.equip_uid.as_slice())
        .unwrap_or_default();
    let mut requested = requested.iter().copied().filter(|uid| *uid != 0);
    let selected_uid = requested.next().unwrap_or(hero.record.default_equip_uid);
    ensure!(
        requested.all(|uid| uid == selected_uid),
        "multiple primary psychubes selected for hero {}",
        hero.record.uid
    );

    let model = UserEquipmentModel::new(hero.record.user_id, db.clone());
    let mut equips = Vec::new();
    if selected_uid != 0 {
        equips.push(model.get_equip(selected_uid).await?);
    }
    let companions = equips
        .iter()
        .filter_map(|equip| {
            config::configs::get().linked_psychube_id(hero.record.hero_id, equip.equip_id)
        })
        .collect::<Vec<_>>();
    if !companions.is_empty() {
        let owned = EquipmentModel::get_all(&model).await?;
        for equip_id in companions {
            if !equips.iter().any(|equip| equip.equip_id == equip_id)
                && let Some(equip) = owned.iter().find(|equip| equip.equip_id == equip_id)
            {
                equips.push(equip.clone());
            }
        }
    }
    Ok(equips)
}

async fn load_compose_support(
    db: &SqlitePool,
    heroes: &UserHeroModel,
    plan: &BattleRosterPlan,
) -> Result<Option<HeroBuildInput>> {
    let Some(selection) = plan.compose_support() else {
        return Ok(None);
    };
    let hero = if let Some(uid) = selection.hero_uid {
        get_hero_by_uid(db, uid).await?
    } else {
        heroes.get_hero(selection.hero_id).await?
    };
    Ok(Some(hero_build_input(&hero)))
}

fn hero_build_input(hero: &HeroData) -> HeroBuildInput {
    let record = &hero.record;
    let template = hero
        .talent_templates
        .iter()
        .find(|(template, _)| template.template_id == record.use_talent_template_id)
        .or_else(|| hero.talent_templates.first());
    let cubes = template
        .filter(|(_, cubes)| !cubes.is_empty())
        .map(|(_, cubes)| cubes.as_slice())
        .unwrap_or(&hero.talent_cubes);
    HeroBuildInput {
        uid: record.uid,
        user_id: record.user_id,
        hero_id: record.hero_id,
        skin: record.skin,
        level: record.level,
        rank: record.rank,
        ex_skill_level: record.ex_skill_level,
        talent: record.talent,
        talent_style: template
            .map(|(template, _)| template.style)
            .unwrap_or_default(),
        talent_placements: cubes.iter().map(|cube| cube.cube_id).collect(),
        destiny_rank: record.destiny_rank,
        destiny_stone: record.destiny_stone,
        extra_str: record.extra_str.clone(),
    }
}

fn equipment_build_input(equip: &Equipment) -> EquipmentBuildInput {
    EquipmentBuildInput {
        uid: equip.uid,
        equip_id: equip.equip_id,
        level: equip.level,
        break_level: equip.break_lv,
        refine_level: equip.refine_lv,
    }
}

#[cfg(test)]
mod tests;
