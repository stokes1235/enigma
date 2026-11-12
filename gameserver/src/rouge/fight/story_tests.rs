//! Account-free story battle fixture using the real Rouge settlement transaction.

use super::*;
use crate::logic::rouge::{
    bag::RunBag,
    flow,
    map::MapNode,
    special_events::{ANCIENT_CHALICE, CHALICE_BATTLE, CHALICE_ENTRUST},
};
use database::db::game::{battle as battle_db, rouge, rouge_favorites};
use sqlx::sqlite::SqlitePoolOptions;

const USER: i64 = 88;
const EPISODE: i32 = 2_902_507;
const NODE: i32 = 10;

async fn fixture() -> SqlitePool {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
    config::init(data.to_str().unwrap()).unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (?, 'rouge-story-battle', 0, 0)",
    )
    .bind(USER)
    .execute(&pool)
    .await
    .unwrap();
    rouge::get_or_create_start(&pool, USER, 1).await.unwrap();
    sqlx::query(
        "UPDATE user_rouge_start SET state = 4, difficulty = 1, style = 1,
         hero_ids = '[3001]' WHERE user_id = ? AND season = 1",
    )
    .bind(USER)
    .execute(&pool)
    .await
    .unwrap();
    let graph = LayerMap {
        layer_id: 141,
        stages: 3,
        nodes: vec![
            MapNode {
                stage: 0,
                node_id: 1,
                last: vec![],
                next: vec![NODE],
                event_id: 0,
                node_type: 0,
            },
            MapNode {
                stage: 1,
                node_id: NODE,
                last: vec![1],
                next: vec![20],
                event_id: CHALICE_BATTLE,
                node_type: 2,
            },
            MapNode {
                stage: 2,
                node_id: 20,
                last: vec![NODE],
                next: vec![],
                event_id: 108101,
                node_type: 8,
            },
        ],
    };
    let team = RougeTeamInfo {
        hero_life_list: vec![RougeHeroLife {
            hero_id: Some(3001),
            life: Some(600),
        }],
        ..Default::default()
    };
    let bag = RunBag {
        rows: 8,
        cols: 8,
        items: vec![],
        uid_counter: 0,
    };
    let middle = serde_json::json!({
        "pieces": [], "middle_layer_id": 131,
        "entrust_id": CHALICE_ENTRUST, "entrust_progress": 0,
        "completed_entrusts": []
    });
    sqlx::query(
        "INSERT INTO user_rouge_run
         (user_id, season, layer_index, layer_id, stage, node_id, seed, graph,
          coin, power, power_limit, team, bag, middle_state, details, updated_at)
         VALUES (?, 1, 3, 141, 1, ?, 1, ?, 0, 40, 40, ?, ?, ?, ?, 0)",
    )
    .bind(USER)
    .bind(NODE)
    .bind(serde_json::to_string(&graph).unwrap())
    .bind(serde_json::to_string(&team).unwrap())
    .bind(serde_json::to_string(&bag).unwrap())
    .bind(middle.to_string())
    .bind(serde_json::to_string(&flow::Details::default()).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn active(pool: &SqlitePool, attacker_hp: i32, defender_hp: i32) -> ActiveBattle {
    let battle_id = config::configs::get()
        .episode
        .get(EPISODE)
        .unwrap()
        .battle_id;
    let fight_id = battle_db::create_fight_instance(
        pool,
        battle_db::NewFightInstance {
            user_id: USER,
            episode_id: EPISODE,
            battle_id,
            multiplication: 1,
            entry_cost: "{}",
            checkpoint: "{}",
            created_at: 0,
        },
    )
    .await
    .unwrap();
    let entity = |uid, model_id, hp| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(model_id),
        current_hp: Some(hp),
        attr: Some(sonettobuf::HeroAttribute {
            hp: Some(100),
            ..Default::default()
        }),
        ..Default::default()
    };
    ActiveBattle {
        fight_id: Some(fight_id),
        chapter_id: ROUGE_CHAPTER_ID,
        episode_id: EPISODE,
        battle_id,
        runtime: battle::tooling::unstarted_battle(
            battle::catalog::BattleCatalog::new(config::configs::get()),
            sonettobuf::Fight {
                battle_id: Some(battle_id),
                attacker: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(1, 3001, attacker_hp)],
                    ..Default::default()
                }),
                defender: Some(sonettobuf::FightTeam {
                    entitys: vec![entity(-1, 2001, defender_hp)],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        fight_group: Some(sonettobuf::FightGroup {
            hero_list: vec![1],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn chalice_count(run: &database::models::game::rouge::RougeRun) -> usize {
    serde_json::from_str::<RunBag>(&run.bag)
        .unwrap()
        .items
        .iter()
        .filter(|item| item.item_id == ANCIENT_CHALICE)
        .count()
}

#[tokio::test]
async fn chalice_quest_victory_records_key_completion_and_album_once_while_failure_grants_nothing()
{
    let pool = fixture().await;
    assert!(can_start(&pool, USER, EPISODE).await.unwrap());
    let failed = active(&pool, 0, 100).await;
    let failure = settle_active(&pool, USER, &failed).await.unwrap();
    assert_eq!(failure.result.is_win, Some(false));
    assert_eq!(failure.result.retry_num, Some(1));
    let after_failure = rouge::get_run(&pool, USER, 1).await.unwrap().unwrap();
    assert_eq!(chalice_count(&after_failure), 0);
    let middle: serde_json::Value = serde_json::from_str(&after_failure.middle_state).unwrap();
    assert_eq!(middle["entrust_id"], CHALICE_ENTRUST);
    assert_eq!(middle["completed_entrusts"], serde_json::json!([]));
    assert!(
        flow::details(&after_failure)
            .special_events
            .rewarded_battles
            .is_empty()
    );
    assert!(
        rouge_favorites::collections(&pool, USER, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(can_start(&pool, USER, EPISODE).await.unwrap());

    let won = active(&pool, 40, 0).await;
    let victory = settle_active(&pool, USER, &won).await.unwrap();
    assert_eq!(victory.result.is_win, Some(true));
    assert_eq!(victory.result.retry_num, Some(0));
    assert_eq!(
        victory.map.cur_interactive.as_deref(),
        Some("12#1003"),
        "the story key supplements the ordinary battle drop"
    );
    let after_victory = rouge::get_run(&pool, USER, 1).await.unwrap().unwrap();
    assert_eq!(chalice_count(&after_victory), 1);
    let middle: serde_json::Value = serde_json::from_str(&after_victory.middle_state).unwrap();
    assert_eq!(middle["entrust_id"], 0);
    assert_eq!(
        middle["completed_entrusts"],
        serde_json::json!([CHALICE_ENTRUST])
    );
    assert_eq!(
        flow::details(&after_victory)
            .special_events
            .rewarded_battles
            .len(),
        1
    );
    assert!(!can_start(&pool, USER, EPISODE).await.unwrap());

    // The real outside reply exposes acquisition history independently of the
    // complete, potentially available collection pool.
    let outside = crate::logic::rouge::rouge_outside_info(&pool, USER, 1)
        .await
        .unwrap();
    let record = outside.rouge_info.unwrap().game_record_info.unwrap();
    assert!(record.pass_collections.contains(&ANCIENT_CHALICE));
    let favorites = crate::logic::rouge::favorites::load(&pool, USER, 1)
        .await
        .unwrap();
    assert!(favorites.new_collection_ids.contains(&ANCIENT_CHALICE));
    assert!(
        rouge_favorites::mark_seen(&pool, USER, 1, 1, ANCIENT_CHALICE)
            .await
            .unwrap()
    );

    // Reusing a completed fight instance fails and rolls back its whole second
    // transaction. In particular it cannot add another key or reset its red dot.
    assert!(settle_active(&pool, USER, &won).await.is_err());
    let replay = rouge::get_run(&pool, USER, 1).await.unwrap().unwrap();
    assert_eq!(chalice_count(&replay), 1);
    assert_eq!(replay.bag, after_victory.bag);
    assert_eq!(replay.middle_state, after_victory.middle_state);
    assert_eq!(replay.details, after_victory.details);
    assert_eq!(
        (replay.coin, replay.team_exp),
        (after_victory.coin, after_victory.team_exp)
    );
    let favorites = crate::logic::rouge::favorites::load(&pool, USER, 1)
        .await
        .unwrap();
    assert!(favorites.pass_collections.contains(&ANCIENT_CHALICE));
    assert!(!favorites.new_collection_ids.contains(&ANCIENT_CHALICE));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_dungeons WHERE user_id = ?")
            .bind(USER)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
