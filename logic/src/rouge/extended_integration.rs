use super::original_tests::{init_config, insert_hero, test_db};
use super::*;

async fn started(genius: &[i32]) -> (SqlitePool, RougeManager) {
    started_with_history(genius, &[]).await
}

async fn started_with_history(genius: &[i32], passed: &[i32]) -> (SqlitePool, RougeManager) {
    init_config();
    let db = test_db().await;
    insert_hero(&db, 1, 1, 3003).await;
    for id in genius {
        rouge::insert_genius(&db, 1, 1, *id).await.unwrap();
    }
    let mut tx = db.begin().await.unwrap();
    for id in passed {
        database::db::game::rouge_history::record_ending_in_transaction(&mut tx, 1, 1, *id, 0)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    manager.select_style(&db, 1, 1).await.unwrap();
    manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
    (db, manager)
}

async fn safe_zone(db: &SqlitePool, manager: RougeManager, middle: i32) -> RougeMapInfo {
    sqlx::query("UPDATE user_rouge_run SET map_type=2,middle_layer_id=?,layer_id=?,position_index=1,interactive_index=-1,battle_interactive='',middle_state='' WHERE user_id=1 AND season=1")
        .bind(middle).bind(middle).execute(db).await.unwrap();
    manager
        .original_info(db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap()
        .map_info
        .unwrap()
}

#[tokio::test]
async fn acquisition_and_loss_grants_commit_once_and_update_commissions() {
    let (db, _) = started(&[]).await;
    let mut before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut state = middle_state(&before);
    state.entrust_id = 1110201;
    state.entrust_progress = 0;
    before.middle_state = serde_json::to_string(&state).unwrap();
    sqlx::query("UPDATE user_rouge_run SET middle_state=? WHERE user_id=1 AND season=1")
        .bind(&before.middle_state)
        .execute(&db)
        .await
        .unwrap();
    let mut after = before.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    let cup = bag.add(8230606).unwrap();
    after.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut after).await.unwrap();
    assert_eq!(after.coin, before.coin + 50);
    assert_eq!(middle_state(&after).entrust_progress, 50);
    let mut unchanged = after.clone();
    flow::save_action(&db, &after, &mut unchanged)
        .await
        .unwrap();
    assert_eq!(unchanged.coin, after.coin);
    let mut lost = after.clone();
    bag.items.retain(|item| item.uid != cup.uid);
    lost.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &after, &mut lost).await.unwrap();
    assert_eq!(lost.coin, before.coin + 100);
    assert_eq!(middle_state(&lost).entrust_progress, 100);
    let mut stale = lost.clone();
    assert!(flow::save_action(&db, &after, &mut stale).await.is_err());
    assert_eq!(
        rouge::get_run(&db, 1, 1).await.unwrap().unwrap().coin,
        lost.coin
    );
}

#[tokio::test]
async fn gift_chains_are_persisted_and_inlay_transfers_are_not_reacquisitions() {
    let (db, manager) = started(&[]).await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut after = before.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    bag.add(8330708).unwrap();
    after.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut after).await.unwrap();
    let saved: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    assert_eq!(saved.items.len(), 4);
    assert_eq!(flow::details(&after).artifact_state.acquired_uids.len(), 4);
    let info = manager
        .original_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!(
        info.bag.unwrap().layouts.len() + info.warehouse.unwrap().items.len(),
        4
    );
    let mut holder_bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let holder = holder_bag.add(8310405).unwrap();
    let disk = holder_bag.add(8140401).unwrap();
    let mut before = after.clone();
    before.bag = serde_json::to_string(&holder_bag).unwrap();
    let mut moved = before.clone();
    holder_bag.inlay(holder.uid, 0, disk.uid, 2).unwrap();
    moved.bag = serde_json::to_string(&holder_bag).unwrap();
    artifact_lifecycle::prepare(&before, &mut moved).unwrap();
    assert!(
        !flow::details(&moved)
            .artifact_state
            .lost_uids
            .contains(&disk.uid)
    );
    assert_eq!(
        artifact_lifecycle::owned(&serde_json::from_str(&moved.bag).unwrap()),
        artifact_lifecycle::owned(&serde_json::from_str(&before.bag).unwrap())
    );
}

#[tokio::test]
async fn barter_is_gated_priced_persistent_and_refresh_is_bounded() {
    let (db, manager) = started(&[201, 207, 213]).await;
    safe_zone(&db, manager, 111).await;
    let map = manager
        .piece_talk_select(&db, 1, 111, 111, 1, 1002)
        .await
        .unwrap();
    let piece = map
        .middle_layer_info
        .unwrap()
        .piece_info
        .into_iter()
        .find(|p| p.index == Some(1))
        .unwrap();
    let trigger: repair::Trigger =
        serde_json::from_str(piece.trigger_str.as_deref().unwrap()).unwrap();
    assert_eq!(trigger.repair_shop_collections.len(), 3);
    assert!(
        trigger
            .repair_shop_collections
            .values()
            .all(|id| matches!(id, 2101 | 2102))
    );
    let (&id, &price) = trigger.repair_shop_collections.iter().next().unwrap();
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    manager.repair_buy(&db, 1, id).await.unwrap();
    let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    if price == 2101 {
        assert_eq!(after.power_limit, before.power_limit - 8);
    } else {
        assert_eq!(
            persisted_team(&after).unwrap().hero_life_list[0].life,
            Some(700)
        );
    }
    assert_eq!(flow::details(&after).repair_shop_num, 1);
    assert!(manager.repair_buy(&db, 1, id).await.is_err());
    manager
        .piece_talk_select(&db, 1, 111, 111, 1, 1002)
        .await
        .unwrap();
    assert!(manager.repair_buy(&db, 1, id).await.is_err());
    manager.repair_random(&db, 1).await.unwrap();
    assert!(manager.repair_random(&db, 1).await.is_err());
    let info = manager
        .original_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert!(
        info.effect_info
            .iter()
            .any(|effect| effect.r#type == Some(21))
    );
    let (db, manager) = started(&[]).await;
    safe_zone(&db, manager, 111).await;
    assert!(
        manager
            .piece_talk_select(&db, 1, 111, 111, 1, 1002)
            .await
            .is_err()
    );
    assert!(manager.repair_buy(&db, 1, id).await.is_err());
}

#[tokio::test]
async fn displace_preserves_inserted_disks_and_cannot_exceed_its_budget() {
    let (db, manager) = started(&[201]).await;
    safe_zone(&db, manager, 111).await;
    manager
        .piece_talk_select(&db, 1, 111, 111, 1, 1003)
        .await
        .unwrap();
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut after = before.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    let unique = bag.add(8310405).unwrap();
    let holder = bag.add(8310609).unwrap();
    let disk = bag.add(8140401).unwrap();
    bag.inlay(holder.uid, 0, disk.uid, 2).unwrap();
    after.bag = serde_json::to_string(&bag).unwrap();
    flow::save(&db, &before, &after).await.unwrap();
    assert!(manager.displace(&db, 1, unique.uid).await.is_err());
    let (_, new_uid) = manager.displace(&db, 1, holder.uid).await.unwrap();
    let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    assert!(bag.item(holder.uid).is_none());
    assert_eq!(bag.item(disk.uid).unwrap().item_id, 8140401);
    assert!(bag.item(new_uid).is_some());
    assert_eq!(flow::details(&after).displace_num, 1);
    assert!(manager.displace(&db, 1, new_uid).await.is_err());
}

#[tokio::test]
async fn all_four_ending_routes_require_the_selected_story_and_have_their_boss() {
    for (choice, key, layer, ending, episode) in [
        (1410201, 0, 151, 101, 2902317),
        (1410202, 8320025, 152, 102, 2902318),
        (1410301, 8320907, 153, 103, 2902322),
        (1410302, 8180501, 154, 104, 2902323),
    ] {
        let history = if ending > 101 { vec![101] } else { vec![] };
        let (db, manager) = started_with_history(&[], &history).await;
        if key > 0 {
            let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
            let mut after = before.clone();
            let mut bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
            bag.add(key).unwrap();
            after.bag = serde_json::to_string(&bag).unwrap();
            flow::save_action(&db, &before, &mut after).await.unwrap();
        }
        let map = safe_zone(&db, manager, 141).await;
        manager.piece_move(&db, 1, 141, 141, -1).await.unwrap();
        assert!(
            manager
                .leave_middle_layer(&db, 1, 141, 141, layer)
                .await
                .is_err()
        );
        let npc = if ending <= 102 { 1410201 } else { 1410301 };
        let piece = map
            .middle_layer_info
            .unwrap()
            .piece_info
            .into_iter()
            .find(|piece| piece.id == Some(npc))
            .unwrap();
        let index = piece.index.unwrap();
        manager.piece_move(&db, 1, 141, 141, index).await.unwrap();
        manager
            .piece_talk_select(&db, 1, 141, 141, index, choice)
            .await
            .unwrap();
        manager.piece_move(&db, 1, 141, 141, -1).await.unwrap();
        let other = if layer == 151 { 152 } else { 151 };
        assert!(
            manager
                .leave_middle_layer(&db, 1, 141, 141, other)
                .await
                .is_err()
        );
        manager
            .leave_middle_layer(&db, 1, 141, 141, layer)
            .await
            .unwrap();
        let map = safe_zone(&db, manager, layer).await;
        let boss = 1510501 + ending - 101;
        let piece = map
            .middle_layer_info
            .unwrap()
            .piece_info
            .into_iter()
            .find(|piece| piece.id == Some(boss))
            .unwrap();
        manager
            .piece_move(&db, 1, layer, layer, piece.index.unwrap())
            .await
            .unwrap();
        manager
            .piece_talk_select(&db, 1, layer, layer, piece.index.unwrap(), boss)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(
            (flow::details(&run).end_id, flow::details(&run).end_episode),
            (ending, episode)
        );
        assert_eq!(run.terminal_state, 0);
    }
}

#[tokio::test]
async fn notebook_adds_a_frozen_disk_choice_without_changing_the_selection_count() {
    let (db, _) = started(&[]).await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut run = before.clone();
    let mut bag = bag::RunBag::from_layout("8#8", "").unwrap();
    let item = bag.add(8330002).unwrap();
    bag.place(item.uid, 0, 0, 0).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    run.battle_interactive = "12#1001".into();
    run.interactive_index = 0;
    flow::freeze_battle_reward(&mut run, 101104).unwrap();
    let drop = drop_state(&run, 1001, config::configs::get()).unwrap();
    assert_eq!(drop.offers.len(), 4);
    assert_eq!(drop.select_count, 1);
    assert!(collection_shapes::is_enchant_item(
        *drop.offers.last().unwrap()
    ));
    bag.remove(item.uid).unwrap();
    run.bag = serde_json::to_string(&bag).unwrap();
    assert_eq!(
        drop_state(&run, 1001, config::configs::get())
            .unwrap()
            .offers,
        drop.offers
    );
}

#[tokio::test]
async fn a_key_alone_cannot_bypass_the_alternative_endings_visibility_gate() {
    let (db, manager) = started(&[]).await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut after = before.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
    bag.add(8320025).unwrap();
    after.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut after).await.unwrap();
    let map = safe_zone(&db, manager, 141).await;
    let priest = map
        .middle_layer_info
        .unwrap()
        .piece_info
        .into_iter()
        .find(|piece| piece.id == Some(1410201))
        .unwrap();
    let index = priest.index.unwrap();
    manager.piece_move(&db, 1, 141, 141, index).await.unwrap();
    assert!(
        manager
            .piece_talk_select(&db, 1, 141, 141, index, 1410202)
            .await
            .is_err()
    );
    manager
        .piece_talk_select(&db, 1, 141, 141, index, 1410201)
        .await
        .unwrap();
}
