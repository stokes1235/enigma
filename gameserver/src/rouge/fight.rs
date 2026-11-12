//! Battle settlement for Rouge nodes.
//!
//! The client enters a rouge fight through the ordinary dungeon fight flow with a
//! fixed chapter id (`RougeMapEnum.ChapterId = 2901`) and the episode carried by
//! `rouge_fight_event`. Settlement therefore arrives here instead of the normal
//! dungeon path: the node is marked resolved and a `RougeFightResultPush` is sent
//! back so the map can continue.

use crate::error::AppError;
use sonettobuf::{
    FightEntityInfo, RougeBattleHero, RougeFightResultPush, RougeHeroInfo, RougeHeroLife,
    RougeMapInfo, RougeTeamInfo,
};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::logic::rouge::map::LayerMap;
use crate::player::battle::ActiveBattle;

/// Chapter id the client uses for every rouge fight (`RougeMapEnum.ChapterId`).
pub const ROUGE_CHAPTER_ID: i32 = 2901;

/// Rouge represents a hero's remaining health as a 0-1000 ratio.
const ROUGE_LIFE_SCALE: i64 = 1_000;

pub struct RougeFightSettlement {
    pub result: RougeFightResultPush,
    pub map: RougeMapInfo,
}

/// Whether an active battle belongs to a rouge run.
pub fn matches_battle(active: &ActiveBattle) -> bool {
    active.chapter_id == ROUGE_CHAPTER_ID
}

/// Validates that an episode belongs to the player's current unresolved map node.
pub async fn can_start(db: &SqlitePool, player_id: i64, episode_id: i32) -> Result<bool, AppError> {
    let Some(run) = database::db::game::rouge::get_run(db, player_id, 1).await? else {
        return Ok(false);
    };
    if run.terminal_state != 0 {
        return Ok(false);
    }
    if run.map_type == 2 {
        let detail = crate::logic::rouge::flow::details(&run);
        return Ok(detail.end_episode == episode_id && detail.end_id > 0);
    }
    if run.map_type != 1 {
        return Ok(false);
    }
    let layer_map: LayerMap = serde_json::from_str(&run.graph)?;
    let Some(node) = layer_map.node(run.node_id) else {
        return Ok(false);
    };
    let finished: Vec<i32> = serde_json::from_str(&run.finished).unwrap_or_default();
    Ok(!finished.contains(&node.node_id)
        && config::configs::get().rouge_fight_episode(node.event_id) == Some(episode_id))
}

/// Resolves the run node that owns an episode.
#[cfg(test)]
pub fn node_for_episode(graph: &str, episode_id: i32) -> Option<i32> {
    let layer_map: LayerMap = serde_json::from_str(graph).ok()?;
    let tables = config::configs::get();
    layer_map
        .nodes
        .iter()
        .find(|node| tables.rouge_fight_episode(node.event_id) == Some(episode_id))
        .map(|node| node.node_id)
}

fn life_ratio(entity: &FightEntityInfo) -> i32 {
    let current_hp = entity.current_hp.unwrap_or_default().max(0) as i64;
    let max_hp = entity
        .attr
        .as_ref()
        .and_then(|attr| attr.hp)
        .or_else(|| entity.base_attr.as_ref().and_then(|attr| attr.hp))
        .unwrap_or(entity.current_hp.unwrap_or_default())
        .max(0) as i64;

    if max_hp == 0 {
        return 0;
    }

    (current_hp.saturating_mul(ROUGE_LIFE_SCALE) / max_hp).clamp(0, ROUGE_LIFE_SCALE) as i32
}

fn team_info(active: &ActiveBattle) -> RougeTeamInfo {
    let heroes: Vec<FightEntityInfo> = if let Some(group) = active.fight_group.as_ref() {
        group
            .hero_list
            .iter()
            .chain(&group.sub_hero_list)
            .filter_map(|uid| active.runtime.entity_info(*uid))
            .collect()
    } else {
        let (fight, _) = active.runtime.reconnect_state();
        fight
            .attacker
            .into_iter()
            .flat_map(|team| {
                let mut entities = team.entitys;
                entities.extend(team.sub_entitys);
                entities
            })
            .filter(|entity| entity.model_id.is_some())
            .collect()
    };

    let battle_hero_list = heroes
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| {
            Some(RougeBattleHero {
                index: Some(index as i32 + 1),
                hero_id: entity.model_id,
                equip_uid: entity.equip_uid,
                ..Default::default()
            })
        })
        .collect();
    let hero_life_list = heroes
        .iter()
        .filter_map(|entity| {
            Some(RougeHeroLife {
                hero_id: entity.model_id,
                life: Some(life_ratio(entity)),
            })
        })
        .collect();
    let hero_info_list = heroes
        .iter()
        .filter_map(|entity| {
            Some(RougeHeroInfo {
                hero_id: entity.model_id,
                ..Default::default()
            })
        })
        .collect();

    RougeTeamInfo {
        battle_hero_list,
        hero_life_list,
        hero_info_list,
        ..Default::default()
    }
}

/// Settles a Rouge battle without touching ordinary dungeon progression.
///
/// Rouge episodes live in chapter 2901, but the client starts them through the
/// dungeon battle protocol. Keeping their settlement in a separate transaction
/// prevents those episodes from being recorded in `user_dungeons` or granting
/// normal dungeon rewards.
pub async fn settle_active(
    db: &SqlitePool,
    player_id: i64,
    active: &ActiveBattle,
) -> Result<RougeFightSettlement, AppError> {
    if !matches_battle(active) {
        return Err(AppError::InvalidRequest);
    }
    let fight_id = active.fight_id.ok_or(AppError::InvalidRequest)?;
    let mut tx = db.begin().await?;
    let push = settle_in_transaction(&mut tx, player_id, active)
        .await?
        .ok_or(AppError::InvalidRequest)?;
    database::db::game::battle::finish_fight_instance_in_transaction(&mut tx, player_id, fight_id)
        .await?;
    tx.commit().await?;
    Ok(push)
}

/// Marks the node resolved and builds the settlement push.
///
/// Returns `None` when the battle is not a rouge fight or the node cannot be
/// matched, so the caller can fall back to the regular dungeon settlement.
pub async fn settle_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    player_id: i64,
    active: &ActiveBattle,
) -> Result<Option<RougeFightSettlement>, AppError> {
    if !matches_battle(active) {
        return Ok(None);
    }

    let run = sqlx::query_as::<_, database::models::game::rouge::RougeRun>(
        "SELECT user_id, season, layer_index, layer_id, stage, node_id, seed, graph, coin, power,
                updated_at, map_type, middle_layer_id, position_index, choices, finished, bag, shop,
                spent_coin, retry_num, terminal_state, minor_count, danger_count, crisis_count,
                step_count, team, power_limit, interactive_index, interactive_refresh, battle_interactive, team_exp, middle_state, details
         FROM user_rouge_run WHERE user_id = ? AND season = 1",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(mut run) = run else {
        return Ok(None);
    };

    if run.map_type == 2 {
        let detail = crate::logic::rouge::flow::details(&run);
        if detail.end_episode != active.episode_id || detail.end_id == 0 || run.terminal_state != 0
        {
            return Err(AppError::InvalidRequest);
        }
        let won = active.runtime.outcome() == battle::BattleOutcome::Victory;
        let fight_team = team_info(active);
        let mut team: RougeTeamInfo = serde_json::from_str(&run.team)?;
        for life in fight_team.hero_life_list {
            if let Some(old) = team
                .hero_life_list
                .iter_mut()
                .find(|old| old.hero_id == life.hero_id)
            {
                *old = life;
            }
        }
        // A retry restarts from the pre-fight roster. Keep failed-fight life
        // only in its result; persisting all-zero life would strand the retry.
        if won {
            run.team = serde_json::to_string(&team)?;
        }
        if won {
            run.terminal_state = 1;
        } else {
            run.retry_num += 1;
        }
        sqlx::query("UPDATE user_rouge_run SET terminal_state = ?, retry_num = ?, team = ? WHERE user_id = ? AND season = 1 AND terminal_state = 0")
            .bind(run.terminal_state).bind(run.retry_num).bind(&run.team).bind(player_id).execute(&mut **tx).await?;
        if won {
            sqlx::query("UPDATE user_rouge_start SET state = 5 WHERE user_id = ? AND season = 1 AND state = 4")
                .bind(player_id).execute(&mut **tx).await?;
        }
        return Ok(Some(RougeFightSettlement {
            result: RougeFightResultPush {
                season: Some(1),
                is_win: Some(won),
                retry_num: Some(run.retry_num),
                team_info: Some(team),
                ..Default::default()
            },
            map: crate::logic::rouge::map_info(&run),
        }));
    }

    let layer_map: LayerMap = serde_json::from_str(&run.graph)?;
    let node = layer_map
        .node(run.node_id)
        .filter(|node| {
            config::configs::get().rouge_fight_episode(node.event_id) == Some(active.episode_id)
        })
        .ok_or(AppError::InvalidRequest)?;
    let node_id = Some(node.node_id);
    let final_node = node.next.is_empty();
    let won = active.runtime.outcome() == battle::BattleOutcome::Victory;
    let retry_num = if won {
        0
    } else {
        run.retry_num.saturating_add(1)
    };
    let fight_team = team_info(active);
    let mut persisted_team: RougeTeamInfo = serde_json::from_str(&run.team).unwrap_or_default();
    if persisted_team.battle_hero_list.is_empty() {
        persisted_team.battle_hero_list = fight_team.battle_hero_list.clone();
    }
    if persisted_team.hero_info_list.is_empty() {
        persisted_team.hero_info_list = fight_team.hero_info_list.clone();
    }
    for life in &fight_team.hero_life_list {
        if let Some(current) = persisted_team
            .hero_life_list
            .iter_mut()
            .find(|current| current.hero_id == life.hero_id)
        {
            *current = *life;
        } else {
            persisted_team.hero_life_list.push(*life);
        }
    }
    let team_json = if won {
        serde_json::to_string(&persisted_team)?
    } else {
        run.team.clone()
    };
    sqlx::query(
        "UPDATE user_rouge_run SET retry_num = ?, team = ? WHERE user_id = ? AND season = 1",
    )
    .bind(retry_num)
    .bind(&team_json)
    .bind(player_id)
    .execute(&mut **tx)
    .await?;
    run.retry_num = retry_num;
    run.team = team_json;

    if won {
        let mut finished: Vec<i32> = serde_json::from_str(&run.finished).unwrap_or_default();
        if !finished.contains(&node.node_id) {
            finished.push(node.node_id);
            run.finished = serde_json::to_string(&finished)?;
            sqlx::query("UPDATE user_rouge_run SET finished = ? WHERE user_id = ? AND season = 1")
                .bind(&run.finished)
                .bind(player_id)
                .execute(&mut **tx)
                .await?;
        }
    }

    let pipeline = if won {
        crate::logic::rouge::battle_reward_pipeline(node.event_id, final_node)
    } else {
        None
    };
    if won {
        let (coin, exp) = crate::logic::rouge::battle_automatic_reward(node.event_id);
        if coin > 0 || exp > 0 {
            sqlx::query("UPDATE user_rouge_run SET coin = coin + ?, team_exp = team_exp + ? WHERE user_id = ? AND season = 1")
                .bind(coin).bind(exp).bind(player_id).execute(&mut **tx).await?;
            run.coin = run.coin.saturating_add(coin);
            run.team_exp = run.team_exp.saturating_add(exp);
        }
        let event_complete = pipeline
            .as_ref()
            .is_none_or(|(pipeline, index)| pipeline.split('|').nth(*index as usize) == Some("5"));
        let middle = crate::logic::rouge::automatic_fight_entrust_update(
            &run,
            coin,
            event_complete.then_some(node.event_id),
        )?;
        if middle != run.middle_state {
            sqlx::query(
                "UPDATE user_rouge_run SET middle_state = ? WHERE user_id = ? AND season = 1",
            )
            .bind(&middle)
            .bind(player_id)
            .execute(&mut **tx)
            .await?;
            run.middle_state = middle;
        }
    }
    if let Some((pipeline, index)) = &pipeline {
        sqlx::query("UPDATE user_rouge_run SET battle_interactive = ?, interactive_index = ?, interactive_refresh = 0 WHERE user_id = ? AND season = 1")
            .bind(pipeline).bind(index).bind(player_id).execute(&mut **tx).await?;
        run.battle_interactive = pipeline.clone();
        run.interactive_index = *index;
        run.interactive_refresh = 0;
    }

    if won && final_node && pipeline.is_none() {
        crate::logic::rouge::flow::finish_node(&mut run)?;
        sqlx::query("UPDATE user_rouge_run SET map_type = ?, middle_layer_id = ?, position_index = ?, middle_state = ?, coin = ?, power = ?, power_limit = ?, battle_interactive = ?, interactive_index = ? WHERE user_id = ? AND season = 1")
            .bind(run.map_type).bind(run.middle_layer_id).bind(run.position_index).bind(&run.middle_state)
            .bind(run.coin).bind(run.power).bind(run.power_limit).bind(&run.battle_interactive).bind(run.interactive_index)
            .bind(player_id).execute(&mut **tx).await?;
    }

    let (reward_coin, reward_exp, reward_count, reward_select) = if won {
        crate::logic::rouge::battle_reward_preview(node.event_id)
    } else {
        (0, 0, 0, 0)
    };
    Ok(Some(RougeFightSettlement {
        result: RougeFightResultPush {
            season: Some(1),
            node_id,
            is_win: Some(won),
            team_info: Some(persisted_team),
            add_coin: Some(reward_coin),
            drop_collection_num: Some(reward_count),
            add_exp: Some(reward_exp),
            retry_num: Some(retry_num),
            drop_select_num: Some(reward_select),
        },
        map: crate::logic::rouge::map_info(&run),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::rouge::map::MapNode;
    use database::db::game::battle as battle_db;
    use sqlx::sqlite::SqlitePoolOptions;

    fn init_config() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
    }

    fn runtime(fight: sonettobuf::Fight) -> battle::Battle {
        battle::tooling::unstarted_battle(
            battle::catalog::BattleCatalog::new(config::configs::get()),
            fight,
        )
    }

    #[tokio::test]
    async fn selected_middle_ending_fight_finishes_the_run_and_can_be_settled_once() {
        init_config();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&pool).await.unwrap();
        sqlx::query("INSERT INTO users (id, username, created_at, updated_at) VALUES (77, 'ending-test', 0, 0)").execute(&pool).await.unwrap();
        database::db::game::rouge::get_or_create_start(&pool, 77, 1)
            .await
            .unwrap();
        sqlx::query("UPDATE user_rouge_start SET state = 4, difficulty = 1, style = 1, hero_ids = '[3001]' WHERE user_id = 77 AND season = 1").execute(&pool).await.unwrap();
        let graph = LayerMap {
            layer_id: 151,
            stages: 0,
            nodes: vec![],
        };
        let team = RougeTeamInfo {
            hero_life_list: vec![RougeHeroLife {
                hero_id: Some(3001),
                life: Some(1000),
            }],
            ..Default::default()
        };
        sqlx::query("INSERT INTO user_rouge_run (user_id, season, layer_index, layer_id, map_type, middle_layer_id, position_index, graph, team, updated_at) VALUES (77, 1, 4, 151, 2, 151, 2, ?, ?, 0)")
            .bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&team).unwrap()).execute(&pool).await.unwrap();
        assert!(!can_start(&pool, 77, 2902317).await.unwrap());
        sqlx::query("UPDATE user_rouge_run SET details = ? WHERE user_id = 77 AND season = 1")
            .bind(
                serde_json::to_string(&crate::logic::rouge::flow::Details {
                    end_episode: 2902317,
                    end_id: 101,
                    ..Default::default()
                })
                .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();
        assert!(can_start(&pool, 77, 2902317).await.unwrap());
        assert!(!can_start(&pool, 77, 2902401).await.unwrap());
        let battle_id = config::configs::get()
            .episode
            .get(2902317)
            .unwrap()
            .battle_id;
        let entity = |uid, model, hp| FightEntityInfo {
            uid: Some(uid),
            model_id: Some(model),
            current_hp: Some(hp),
            attr: Some(sonettobuf::HeroAttribute {
                hp: Some(100),
                ..Default::default()
            }),
            ..Default::default()
        };
        let active = ActiveBattle {
            chapter_id: ROUGE_CHAPTER_ID,
            episode_id: 2902317,
            battle_id,
            runtime: runtime(sonettobuf::Fight {
                battle_id: Some(battle_id),
                attacker: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(1, 3001, 40)],
                    ..Default::default()
                }),
                defender: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(-1, 2001, 0)],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            fight_group: Some(sonettobuf::FightGroup {
                hero_list: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut tx = pool.begin().await.unwrap();
        let result = settle_in_transaction(&mut tx, 77, &active)
            .await
            .unwrap()
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(result.result.is_win, Some(true));
        assert!(result.result.add_coin.is_none() && result.result.add_exp.is_none());
        assert!(!can_start(&pool, 77, 2902317).await.unwrap());
        let manager = crate::logic::rouge::RougeManager::new(77);
        let final_info = manager
            .original_info(&pool, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        assert_eq!((final_info.state, final_info.end_id), (Some(5), Some(101)));
        let (info, result) = manager.end(&pool, 1).await.unwrap();
        assert_eq!(result.end2_score.as_deref(), Some("101#400"));
        assert_eq!(manager.end(&pool, 1).await.unwrap(), (info, result));
    }

    #[test]
    fn fight_events_resolve_to_their_episode() {
        init_config();
        let tables = config::configs::get();
        assert_eq!(tables.rouge_fight_episode(101901), Some(2902401));
        assert_eq!(tables.rouge_fight_episode(101902), Some(2902402));
        // 105001 is a plain (non-fight) event.
        assert_eq!(tables.rouge_fight_episode(105001), None);
    }

    #[test]
    fn maps_an_episode_back_to_its_node() {
        init_config();
        let map = LayerMap {
            layer_id: 111,
            stages: 2,
            nodes: vec![
                MapNode {
                    stage: 0,
                    node_id: 1,
                    last: Vec::new(),
                    next: vec![10],
                    event_id: 0,
                    node_type: 0,
                },
                MapNode {
                    stage: 1,
                    node_id: 10,
                    last: vec![1],
                    next: Vec::new(),
                    event_id: 101901,
                    node_type: 1,
                },
            ],
        };
        let graph = serde_json::to_string(&map).unwrap();
        assert_eq!(node_for_episode(&graph, 2902401), Some(10));
        assert_eq!(node_for_episode(&graph, 2902402), None);
    }

    #[tokio::test]
    async fn rouge_settlement_persists_retries_without_ordinary_dungeon_progress() {
        init_config();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (77, 'rouge-settlement', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let map = LayerMap {
            layer_id: 111,
            stages: 2,
            nodes: vec![MapNode {
                stage: 1,
                node_id: 10,
                last: vec![1],
                next: Vec::new(),
                event_id: 101104,
                node_type: 1,
            }],
        };
        sqlx::query(
            "INSERT INTO user_rouge_run
             (user_id, season, layer_index, layer_id, stage, node_id, seed, graph, coin, power,
              updated_at)
             VALUES (77, 1, 0, 111, 1, 10, 1, ?, 0, 0, 0)",
        )
        .bind(serde_json::to_string(&map).unwrap())
        .execute(&pool)
        .await
        .unwrap();

        let episode_id = 2901104;
        let roster = RougeTeamInfo {
            hero_life_list: vec![RougeHeroLife {
                hero_id: Some(3001),
                life: Some(600),
            }],
            ..Default::default()
        };
        sqlx::query("UPDATE user_rouge_run SET team = ? WHERE user_id = 77 AND season = 1")
            .bind(serde_json::to_string(&roster).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let battle_id = config::configs::get()
            .episode
            .get(episode_id)
            .unwrap()
            .battle_id;
        let fight_id = battle_db::create_fight_instance(
            &pool,
            battle_db::NewFightInstance {
                user_id: 77,
                episode_id,
                battle_id,
                multiplication: 1,
                entry_cost: "{}",
                checkpoint: "{}",
                created_at: 0,
            },
        )
        .await
        .unwrap();
        let entity = |uid, model_id, hp, max_hp| sonettobuf::FightEntityInfo {
            uid: Some(uid),
            model_id: Some(model_id),
            current_hp: Some(hp),
            attr: Some(sonettobuf::HeroAttribute {
                hp: Some(max_hp),
                ..Default::default()
            }),
            ..Default::default()
        };
        let active = |fight_id, attacker_hp, defender_hp| ActiveBattle {
            fight_id: Some(fight_id),
            chapter_id: ROUGE_CHAPTER_ID,
            episode_id,
            battle_id,
            runtime: runtime(sonettobuf::Fight {
                battle_id: Some(battle_id),
                attacker: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(1, 3001, attacker_hp, 100)],
                    ..Default::default()
                }),
                defender: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(-1, 2001, defender_hp, 100)],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            fight_group: Some(sonettobuf::FightGroup {
                hero_list: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(can_start(&pool, 77, episode_id).await.unwrap());
        assert!(!can_start(&pool, 77, 2902402).await.unwrap());
        let failed = active(fight_id, 0, 100);
        let settlement = settle_active(&pool, 77, &failed).await.unwrap();
        assert_eq!(settlement.map.map_type, Some(1));
        let push = settlement.result;
        assert_eq!(push.node_id, Some(10));
        assert_eq!(push.is_win, Some(false));
        assert_eq!(push.retry_num, Some(1));
        assert!(can_start(&pool, 77, episode_id).await.unwrap());
        let team_info = push.team_info.unwrap();
        assert_eq!(team_info.battle_hero_list.len(), 1);
        assert_eq!(team_info.battle_hero_list[0].index, Some(1));
        assert_eq!(team_info.battle_hero_list[0].hero_id, Some(3001));
        assert_eq!(team_info.hero_life_list.len(), 1);
        assert_eq!(team_info.hero_life_list[0].hero_id, Some(3001));
        assert_eq!(team_info.hero_life_list[0].life, Some(0));
        let after_loss = database::db::game::rouge::get_run(&pool, 77, 1)
            .await
            .unwrap()
            .unwrap();
        let retry_team: RougeTeamInfo = serde_json::from_str(&after_loss.team).unwrap();
        assert_eq!(retry_team.hero_life_list[0].life, Some(600));
        let finished: String = sqlx::query_scalar(
            "SELECT finished FROM user_rouge_run WHERE user_id = 77 AND season = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<i32>>(&finished).unwrap(),
            Vec::<i32>::new()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_dungeons WHERE user_id = 77")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM fight_instances WHERE id = ? AND active = 1",
            )
            .bind(fight_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );

        let retry_fight_id = battle_db::create_fight_instance(
            &pool,
            battle_db::NewFightInstance {
                user_id: 77,
                episode_id,
                battle_id,
                multiplication: 1,
                entry_cost: "{}",
                checkpoint: "{}",
                created_at: 1,
            },
        )
        .await
        .unwrap();
        let won = active(retry_fight_id, 40, 0);
        let settlement = settle_active(&pool, 77, &won).await.unwrap();
        assert_eq!(settlement.map.map_type, Some(1));
        assert_eq!(settlement.map.cur_interactive.as_deref(), Some("12#1001"));
        let push = settlement.result;
        assert_eq!(push.is_win, Some(true));
        assert_eq!(push.retry_num, Some(0));
        assert_eq!(push.add_coin, Some(50));
        assert_eq!(push.add_exp, Some(12));
        assert_eq!(push.drop_collection_num, Some(3));
        assert_eq!(push.team_info.unwrap().hero_life_list[0].life, Some(400));
        assert_eq!(
            sqlx::query_scalar::<_, i32>(
                "SELECT retry_num FROM user_rouge_run WHERE user_id = 77 AND season = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i32>(
                "SELECT map_type FROM user_rouge_run WHERE user_id = 77 AND season = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert!(!can_start(&pool, 77, episode_id).await.unwrap());
        let finished: String = sqlx::query_scalar(
            "SELECT finished FROM user_rouge_run WHERE user_id = 77 AND season = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<i32>>(&finished).unwrap(),
            vec![10]
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM fight_instances WHERE id = ? AND active = 1",
            )
            .bind(retry_fight_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );

        // The last layer also waits for its reward before entering terminal state.
        sqlx::query(
            "UPDATE user_rouge_run
             SET layer_index = 4, map_type = 1, middle_layer_id = 0, finished = '[]', battle_interactive = '', interactive_index = -1
             WHERE user_id = 77 AND season = 1",
        )
        .execute(&pool)
        .await
        .unwrap();
        let final_fight_id = battle_db::create_fight_instance(
            &pool,
            battle_db::NewFightInstance {
                user_id: 77,
                episode_id,
                battle_id,
                multiplication: 1,
                entry_cost: "{}",
                checkpoint: "{}",
                created_at: 2,
            },
        )
        .await
        .unwrap();
        let final_win = active(final_fight_id, 80, 0);
        let settlement = settle_active(&pool, 77, &final_win).await.unwrap();
        assert_eq!(settlement.map.map_type, Some(1));
        assert_eq!(
            sqlx::query_scalar::<_, i32>(
                "SELECT terminal_state FROM user_rouge_run WHERE user_id = 77 AND season = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }
}
