use super::original_tests::{init_config, insert_hero, test_db};
use super::*;

async fn started(all_growth: bool) -> (SqlitePool, RougeManager) {
    init_config();
    let db = test_db().await;
    if all_growth {
        for row in config::configs::get()
            .rouge_genius_branch
            .iter()
            .filter(|row| row.season == 1)
        {
            rouge::insert_genius(&db, 1, 1, row.id).await.unwrap();
        }
    }
    insert_hero(&db, 1, 1, 3003).await;
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    let info = manager.select_style(&db, 1, 1).await.unwrap();
    if all_growth {
        assert_eq!((info.coin, info.team_size), (Some(150), Some(23)));
    }
    manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
    (db, manager)
}

#[tokio::test]
async fn growth_initial_resources_and_frozen_battle_rewards_are_applied_once() {
    let (db, manager) = started(true).await;
    let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(expected.coin, 150);
    let mut run = expected.clone();
    run.team_exp = 10;
    let mut bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let item = bag.add(8320618).unwrap();
    bag.place(item.uid, 0, 0, 0).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    run.battle_interactive = "12#1001".into();
    run.interactive_index = 0;
    flow::freeze_battle_reward(&mut run, 101104).unwrap();
    assert_eq!(
        (flow::reward(&run, 1001).coin, flow::reward(&run, 1001).exp),
        (75, 15)
    );
    // Moving the bonus item after winning must not rewrite the reward quote.
    bag.remove(item.uid).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    flow::save(&db, &expected, &run).await.unwrap();
    let map = map_info(&run);
    let offer: serde_json::Value =
        serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
    assert_eq!(offer["dropCollectList"].as_array().unwrap().len(), 4);
    let (map, _, _, _) = manager.select_drop(&db, 1, &[0]).await.unwrap();
    assert_eq!(map.cur_interactive.as_deref(), Some("5"));
    let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!((after.coin, after.team_exp), (225, 25));
    assert!(manager.select_drop(&db, 1, &[0]).await.is_err());
}

#[tokio::test]
async fn artifact_limit_recomputes_delta_and_keeps_permanent_limit_rewards() {
    let (db, manager) = started(false).await;
    let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut run = expected.clone();
    run.power_limit = 65;
    run.power = 60;
    let mut bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let item = bag.add(8320109).unwrap();
    bag.place(item.uid, 0, 0, 0).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    flow::save(&db, &expected, &run).await.unwrap();
    for _ in 0..2 {
        assert_eq!(
            manager
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap()
                .power_limit,
            Some(115)
        );
    }
    let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let expected = run.clone();
    bag.remove(item.uid).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    run.power = 100;
    flow::save(&db, &expected, &run).await.unwrap();
    let info = manager
        .original_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!((info.power_limit, info.power), (Some(65), Some(65)));
}

#[tokio::test]
async fn shop_displays_and_charges_the_same_artefact_discount() {
    let (db, manager) = started(false).await;
    let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
    let node = graph
        .nodes
        .iter_mut()
        .find(|node| node.node_id == run.node_id)
        .unwrap();
    node.node_type = 7;
    node.event_id = 107010;
    let mut bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let item = bag.add(8330001).unwrap();
    bag.place(item.uid, 0, 0, 0).unwrap();
    let stock = shop::generate(
        111,
        run.node_id,
        run.seed as u64,
        config::configs::get().rouge_goods.iter().cloned(),
    );
    let original = config::configs::get()
        .rouge_goods
        .get(stock.stock[0])
        .unwrap()
        .price;
    sqlx::query(
        "UPDATE user_rouge_run SET graph=?, bag=?, shop=?,coin=10000 WHERE user_id=1 AND season=1",
    )
    .bind(serde_json::to_string(&graph).unwrap())
    .bind(serde_json::to_string(&bag).unwrap())
    .bind(serde_json::to_string(&stock).unwrap())
    .execute(&db)
    .await
    .unwrap();
    let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let map = map_info(&run);
    let node = map
        .layer_info
        .unwrap()
        .node_info
        .into_iter()
        .find(|node| node.node_id == Some(run.node_id))
        .unwrap();
    let data: serde_json::Value =
        serde_json::from_str(node.event_data.as_deref().unwrap()).unwrap();
    let price = original * 900 / 1000;
    assert_eq!(data["posGoodMap"]["1"]["price"], price);
    manager
        .buy_goods(&db, 1, 111, run.node_id, 107010, &[1])
        .await
        .unwrap();
    let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!((after.coin, after.spent_coin), (10000 - price, price));
}

#[tokio::test]
async fn combat_capacity_counts_supports_without_limiting_the_recruited_roster() {
    let (db, manager) = started(false).await;
    let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let expected = run.clone();
    let mut team = persisted_team(&run).unwrap();
    for (uid, id) in [(2, 3004), (3, 3007), (4, 3009)] {
        insert_hero(&db, 1, uid, id).await;
        team.hero_life_list.push(RougeHeroLife {
            hero_id: Some(id),
            life: Some(1000),
        });
    }
    run.team = serde_json::to_string(&team).unwrap();
    flow::save(&db, &expected, &run).await.unwrap();
    let mut party: Vec<_> = [3003, 3004, 3007]
        .into_iter()
        .enumerate()
        .map(|(slot, id)| RougeBattleHero {
            index: Some(slot as i32 + 1),
            hero_id: Some(id),
            ..Default::default()
        })
        .collect();
    manager.group_change(&db, 1, &party).await.unwrap(); // 12 / 15 noise.
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    party[0].support_hero_id = Some(3009);
    assert!(manager.group_change(&db, 1, &party).await.is_err()); // 16 / 15.
    assert_eq!(
        rouge::get_run(&db, 1, 1).await.unwrap().unwrap().team,
        before.team
    );
    let mut run = before.clone();
    run.team_exp = 20; // One level adds two capacity.
    flow::save(&db, &before, &run).await.unwrap();
    let team = manager.group_change(&db, 1, &party).await.unwrap();
    assert_eq!(team.hero_life_list.len(), 4);
    assert_eq!(team.battle_hero_list[0].support_hero_id, Some(3009));
}

#[tokio::test]
async fn recruitment_returns_safe_zone_healing_and_recovery_without_reapplying_it() {
    let (db, manager) = started(false).await;
    let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut run = expected.clone();
    let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
    let node = graph
        .nodes
        .iter_mut()
        .find(|node| node.node_id == run.node_id)
        .unwrap();
    node.node_type = map::NODE_MINOR_OBSTACLE;
    node.next.clear();
    run.graph = serde_json::to_string(&graph).unwrap();
    run.finished = serde_json::to_string(&vec![run.node_id]).unwrap();
    let mut team = persisted_team(&run).unwrap();
    team.hero_life_list[0].life = Some(100);
    run.team = serde_json::to_string(&team).unwrap();
    let mut bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let item = bag.add(8210045).unwrap();
    bag.place(item.uid, 0, 0, 0).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    let mut detail = flow::details(&run);
    detail.outside_genius = Some(vec![204]);
    run.details = serde_json::to_string(&detail).unwrap();
    run.power_limit = 90;
    run.power = 0;
    run.battle_interactive = "5".into();
    run.interactive_index = 0;
    // Graph is fixed by run generation; update the test fixture separately.
    sqlx::query("UPDATE user_rouge_run SET graph=? WHERE user_id=1 AND season=1")
        .bind(&run.graph)
        .execute(&db)
        .await
        .unwrap();
    flow::save(&db, &expected, &run).await.unwrap();
    let (team, map) = manager.recruit_heroes(&db, 1, &[]).await.unwrap();
    assert!(map.middle_layer_info.is_some());
    assert_eq!(team.hero_life_list[0].life, Some(500)); // Growth 20% + artefact 20%.
    let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(saved.power, 40); // Recover the catalyst's 40, not the enlarged cap.
    assert_eq!(
        persisted_team(&saved).unwrap().hero_life_list,
        team.hero_life_list
    );
    let reread = manager
        .original_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!(reread.team_info.unwrap().hero_life_list[0].life, Some(500));
    assert_eq!(reread.power, Some(40));
}

#[tokio::test]
async fn growth_applies_to_event_essence_and_safe_zone_commission_rewards() {
    let (db, _) = started(false).await;
    let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut detail = flow::details(&run);
    detail.outside_genius = Some(vec![202, 208]);
    run.details = serde_json::to_string(&detail).unwrap();
    run.coin = 0;
    run.team_exp = 0;
    flow::apply_effect(&mut run, 1110201).unwrap();
    flow::apply_automatic_drop(&mut run, 10610206).unwrap();
    assert_eq!((run.coin, run.team_exp), (110, 22));
    let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
    let node = graph
        .nodes
        .iter_mut()
        .find(|node| node.node_id == run.node_id)
        .unwrap();
    node.node_type = map::NODE_MINOR_OBSTACLE;
    node.next.clear();
    run.graph = serde_json::to_string(&graph).unwrap();
    run.finished = serde_json::to_string(&vec![run.node_id]).unwrap();
    let mut state = middle_state(&run);
    state.entrust_id = 1110201;
    state.entrust_progress = 200;
    run.middle_state = serde_json::to_string(&state).unwrap();
    flow::finish_node(&mut run).unwrap();
    assert_eq!(run.coin, 220);
    assert!(middle_state(&run).completed_entrusts.contains(&1110201));
    flow::finish_node(&mut run).unwrap();
    assert_eq!(run.coin, 220);
}

#[tokio::test]
async fn old_run_actions_and_repeated_layer_transitions_preserve_the_new_state() {
    let (db, _) = started(false).await;
    let old = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let new_seed = old.seed.wrapping_add(1);
    sqlx::query("UPDATE user_rouge_run SET seed=? WHERE user_id=1 AND season=1")
        .bind(new_seed)
        .execute(&db)
        .await
        .unwrap();
    let mut changed = old.clone();
    changed.coin += 100;
    assert!(flow::save(&db, &old, &changed).await.is_err());
    assert_eq!(
        rouge::get_run(&db, 1, 1).await.unwrap().unwrap().coin,
        old.coin
    );
    sqlx::query("UPDATE user_rouge_run SET map_type=2, middle_layer_id=111, position_index=-1 WHERE user_id=1 AND season=1")
        .execute(&db).await.unwrap();
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let next = RougeRunPosition {
        map_type: MAP_TYPE_NORMAL,
        layer_index: 1,
        layer_id: 122,
        stage: 0,
        node_id: 0,
        middle_layer_id: 0,
        position_index: 0,
    };
    assert!(
        rouge::advance_run_layer(&db, 1, 1, &before, &next, &before.graph, 1, 2, 0, 4)
            .await
            .unwrap()
    );
    assert!(
        !rouge::advance_run_layer(&db, 1, 1, &before, &next, &before.graph, 1, 2, 0, 4)
            .await
            .unwrap()
    );
    let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(
        (after.minor_count, after.danger_count, after.step_count),
        (1, 2, 4)
    );
}
