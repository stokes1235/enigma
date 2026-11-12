//! Real manager requests over synthetic, account-free story fixtures. These
//! verify persistent choices/costs and phase boundaries, not official spawn odds.

use super::special_events::{
    AACHEN_ROSE, ANCIENT_CHALICE, CHALICE_DISCOVERY, CHALICE_ENTRUST, LITTLE_CONCH,
};
use super::*;
use crate::rouge::original_tests::{init_config, insert_hero, test_db};

async fn started() -> (SqlitePool, RougeManager) {
    init_config();
    let db = test_db().await;
    insert_hero(&db, 1, 1, 3003).await;
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    manager.select_style(&db, 1, 1).await.unwrap();
    manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
    (db, manager)
}

async fn run(db: &SqlitePool) -> RougeRun {
    rouge::get_run(db, 1, 1).await.unwrap().unwrap()
}

fn item_count(run: &RougeRun, id: i32) -> i32 {
    let bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
    flow::collection_counts(&bag).get(&id).copied().unwrap_or(0)
}

fn menu(map: &RougeMapInfo, node_id: i32) -> Vec<i32> {
    let node = map
        .layer_info
        .as_ref()
        .unwrap()
        .node_info
        .iter()
        .find(|node| node.node_id == Some(node_id))
        .unwrap();
    let event: serde_json::Value =
        serde_json::from_str(node.event_data.as_deref().unwrap()).unwrap();
    event["canChoiceSet"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_i64().unwrap() as i32)
        .collect()
}

fn event_for(map: &RougeMapInfo, node_id: i32) -> i32 {
    map.layer_info
        .as_ref()
        .unwrap()
        .node_info
        .iter()
        .find(|node| node.node_id == Some(node_id))
        .unwrap()
        .event_id
        .unwrap()
}

/// Position the run at a real nonterminal slot; only its script/resources are
/// replaced so later requests still use the actual manager validation path.
async fn fixture_event(
    db: &SqlitePool,
    event: i32,
    coin: i32,
    power: i32,
    conch: bool,
) -> RougeRun {
    let mut current = run(db).await;
    let mut graph: map::LayerMap = serde_json::from_str(&current.graph).unwrap();
    let node = graph
        .nodes
        .iter_mut()
        .find(|node| node.stage > 0 && !node.next.is_empty())
        .unwrap();
    node.event_id = event;
    node.node_type = 6;
    current.node_id = node.node_id;
    current.stage = node.stage;
    let mut detail = flow::details(&current);
    detail.passed_endings = Some(vec![101]);
    let mut bag: bag::RunBag = serde_json::from_str(&current.bag).unwrap();
    if conch {
        bag.add(LITTLE_CONCH).unwrap();
    }
    sqlx::query("UPDATE user_rouge_run SET graph = ?, node_id = ?, stage = ?, details = ?, bag = ?, coin = ?, power = ?, power_limit = 80, choices = '{}', finished = '[]', interactive_index = -1, battle_interactive = '' WHERE user_id = 1 AND season = 1")
        .bind(serde_json::to_string(&graph).unwrap()).bind(current.node_id).bind(current.stage)
        .bind(serde_json::to_string(&detail).unwrap()).bind(serde_json::to_string(&bag).unwrap())
        .bind(coin).bind(power).execute(db).await.unwrap();
    run(db).await
}

#[tokio::test]
async fn noble_requires_both_phases_spends_power_and_grants_one_conch() {
    let (db, manager) = started().await;
    let current = fixture_event(&db, 106151, 0, 60, false).await;
    assert_eq!(
        menu(&map_info(&current), current.node_id),
        vec![10615101, 10615102]
    );
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615111)
            .await
            .is_err()
    );
    assert_eq!(run(&db).await.power, 60);
    assert_eq!(item_count(&run(&db).await, LITTLE_CONCH), 0);

    let map = manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615101)
        .await
        .unwrap();
    assert_eq!(event_for(&map, current.node_id), 1061511);
    assert_eq!(menu(&map, current.node_id), vec![10615111, 10615112]);
    assert_eq!(
        run(&db).await.power,
        60,
        "opening phase two is not the paid decision"
    );
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615101)
            .await
            .is_err()
    );

    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 1061511, 10615111)
        .await
        .unwrap();
    let after = run(&db).await;
    assert_eq!(after.power, 10);
    assert_eq!(flow::details(&after).consume_power, 50);
    assert_eq!(item_count(&after, LITTLE_CONCH), 1);
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 1061511, 10615111)
            .await
            .is_err()
    );
    manager.original_info(&db, 1).await.unwrap();
    let reconnect = run(&db).await;
    assert_eq!(reconnect.power, 10);
    assert_eq!(item_count(&reconnect, LITTLE_CONCH), 1);
}

#[tokio::test]
async fn noble_can_spend_all_remaining_power_and_coin_branch_grants_no_key() {
    let (db, manager) = started().await;
    let current = fixture_event(&db, 106151, 0, 20, false).await;
    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615101)
        .await
        .unwrap();
    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 1061511, 10615111)
        .await
        .unwrap();
    let after = run(&db).await;
    assert_eq!(after.power, 0);
    assert_eq!(flow::details(&after).consume_power, 20);
    assert_eq!(item_count(&after, LITTLE_CONCH), 1);

    let (db, manager) = started().await;
    let current = fixture_event(&db, 106151, 0, 20, false).await;
    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615102)
        .await
        .unwrap();
    let after = run(&db).await;
    assert_eq!(after.coin, 100);
    assert_eq!(after.power, 20);
    assert_eq!(item_count(&after, LITTLE_CONCH), 0);
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106151, 10615102)
            .await
            .is_err()
    );
    assert_eq!(run(&db).await.coin, 100);
}

#[tokio::test]
async fn army_requires_both_phases_charges_current_life_and_grants_one_chalice() {
    let (db, manager) = started().await;
    let current = fixture_event(&db, 106152, 0, 40, true).await;
    let mut team: RougeTeamInfo = serde_json::from_str(&current.team).unwrap();
    team.hero_life_list[0].life = Some(500);
    sqlx::query("UPDATE user_rouge_run SET team = ? WHERE user_id = 1 AND season = 1")
        .bind(serde_json::to_string(&team).unwrap())
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        menu(&map_info(&run(&db).await), current.node_id),
        vec![10615201, 10615202]
    );
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106152, 10615211)
            .await
            .is_err()
    );
    let map = manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 106152, 10615201)
        .await
        .unwrap();
    assert_eq!(event_for(&map, current.node_id), 1061521);
    assert_eq!(menu(&map, current.node_id), vec![10615211, 10615212]);
    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 1061521, 10615211)
        .await
        .unwrap();
    let after = run(&db).await;
    let team: RougeTeamInfo = serde_json::from_str(&after.team).unwrap();
    assert_eq!(team.hero_life_list[0].life, Some(125));
    assert_eq!(item_count(&after, ANCIENT_CHALICE), 1);
    assert_eq!(
        item_count(&after, LITTLE_CONCH),
        1,
        "the displayed cost does not consume the conch"
    );
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 1061521, 10615211)
            .await
            .is_err()
    );
    let after = run(&db).await;
    let team: RougeTeamInfo = serde_json::from_str(&after.team).unwrap();
    assert_eq!(team.hero_life_list[0].life, Some(125));
    assert_eq!(item_count(&after, ANCIENT_CHALICE), 1);
}

#[tokio::test]
async fn empty_studio_requires_three_hundred_coins_and_records_the_actual_spend_once() {
    let (db, manager) = started().await;
    let current = fixture_event(&db, 106153, 299, 40, true).await;
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106153, 10615301)
            .await
            .is_err()
    );
    let rejected = run(&db).await;
    assert_eq!(rejected.coin, 299);
    assert_eq!(item_count(&rejected, AACHEN_ROSE), 0);
    sqlx::query("UPDATE user_rouge_run SET coin = 300 WHERE user_id = 1 AND season = 1")
        .execute(&db)
        .await
        .unwrap();
    manager
        .choice_event(&db, 1, current.layer_id, current.node_id, 106153, 10615301)
        .await
        .unwrap();
    let after = run(&db).await;
    assert_eq!(after.coin, 0);
    assert_eq!(after.spent_coin, current.spent_coin + 300);
    assert_eq!(item_count(&after, AACHEN_ROSE), 1);
    assert!(
        manager
            .choice_event(&db, 1, current.layer_id, current.node_id, 106153, 10615301)
            .await
            .is_err()
    );
    let after = run(&db).await;
    assert_eq!(after.coin, 0);
    assert_eq!(item_count(&after, AACHEN_ROSE), 1);
}

#[tokio::test]
async fn rumor_in_layer132_safe_zone_injects_the_chalice_discovery_in_the_next_area() {
    let (db, manager) = started().await;
    let current = run(&db).await;
    let graph = generate_layer(132, current.seed as u64, &[], &[]);
    let leaf = graph
        .nodes
        .iter()
        .find(|node| node.next.is_empty())
        .unwrap();
    let middle = MiddleState {
        pieces: vec![RougePieceInfo {
            index: Some(1),
            id: Some(CHALICE_ENTRUST),
            talk_id: Some(CHALICE_ENTRUST),
            finish: Some(false),
            select_id: Some(0),
            ..Default::default()
        }],
        entrust_id: 0,
        entrust_progress: 0,
        completed_entrusts: vec![],
        middle_layer_id: 131,
    };
    sqlx::query("UPDATE user_rouge_run SET layer_index = 2, layer_id = 132, graph = ?, stage = ?, node_id = ?, map_type = 2, middle_layer_id = 131, position_index = 1, middle_state = ?, choices = '{}', finished = ?, interactive_index = -1, battle_interactive = '' WHERE user_id = 1 AND season = 1")
        .bind(serde_json::to_string(&graph).unwrap()).bind(leaf.stage).bind(leaf.node_id)
        .bind(serde_json::to_string(&middle).unwrap()).bind(serde_json::to_string(&vec![leaf.node_id]).unwrap())
        .execute(&db).await.unwrap();
    manager
        .piece_talk_select(&db, 1, 132, 131, 1, CHALICE_ENTRUST)
        .await
        .unwrap();
    let accepted = run(&db).await;
    let state: MiddleState = serde_json::from_str(&accepted.middle_state).unwrap();
    assert_eq!(state.entrust_id, CHALICE_ENTRUST);
    assert_eq!(flow::details(&accepted).special_events.pending.len(), 1);
    assert!(
        manager
            .piece_talk_select(&db, 1, 132, 131, 1, CHALICE_ENTRUST)
            .await
            .is_err()
    );
    manager.piece_move(&db, 1, 132, 131, -1).await.unwrap();
    let map = manager
        .leave_middle_layer(&db, 1, 132, 131, 141)
        .await
        .unwrap();
    let advanced = run(&db).await;
    let graph: map::LayerMap = serde_json::from_str(&advanced.graph).unwrap();
    let targets: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.event_id == CHALICE_DISCOVERY)
        .collect();
    assert_eq!(targets.len(), 1);
    let target = targets[0];
    assert!(
        !target.next.is_empty(),
        "the region's mandatory leaf is preserved"
    );
    assert_eq!(event_for(&map, target.node_id), CHALICE_DISCOVERY);
    assert!(flow::details(&advanced).special_events.pending.is_empty());
    let state: MiddleState = serde_json::from_str(&advanced.middle_state).unwrap();
    assert_eq!(state.entrust_id, CHALICE_ENTRUST);
    manager.original_info(&db, 1).await.unwrap();
    assert_eq!(
        serde_json::from_str::<map::LayerMap>(&run(&db).await.graph)
            .unwrap()
            .nodes
            .iter()
            .filter(|node| node.event_id == CHALICE_DISCOVERY)
            .count(),
        1
    );

    // Position at the injected discovery to exercise its real choice request.
    sqlx::query(
        "UPDATE user_rouge_run SET node_id = ?, stage = ? WHERE user_id = 1 AND season = 1",
    )
    .bind(target.node_id)
    .bind(target.stage)
    .execute(&db)
    .await
    .unwrap();
    let map = manager
        .choice_event(&db, 1, 141, target.node_id, CHALICE_DISCOVERY, 10614101)
        .await
        .unwrap();
    assert_eq!(
        event_for(&map, target.node_id),
        special_events::CHALICE_BATTLE
    );
    assert_eq!(
        config::configs::get().rouge_fight_episode(special_events::CHALICE_BATTLE),
        Some(2902507)
    );
    assert!(
        manager
            .choice_event(&db, 1, 141, target.node_id, CHALICE_DISCOVERY, 10614101)
            .await
            .is_err()
    );
}
