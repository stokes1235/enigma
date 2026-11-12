//! Settlement integration checks built from sanitized capture facts. No raw
//! capture, account profile or login credential is embedded in these fixtures.

use super::*;
use crate::rouge::original_tests::{init_config, insert_hero, test_db};
use std::collections::BTreeMap;

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

async fn seed_balance(db: &SqlitePool, balance: i32) {
    extra_rewards::balance(db, 1, 1).await.unwrap();
    sqlx::query("UPDATE user_rouge_extra_points SET balance = ? WHERE user_id = 1 AND season = 1")
        .bind(balance)
        .execute(db)
        .await
        .unwrap();
}

/// Reconstruct only the publicly meaningful score components of the captured
/// 8990-credit run. The duplicate low-value items stand in for the actual 46
/// owned collections, so this is a settlement fixture, not a full run replay.
async fn captured_clear(db: &SqlitePool) -> RougeRun {
    let expected = rouge::get_run(db, 1, 1).await.unwrap().unwrap();
    let mut run = expected.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
    for _ in 0..45 {
        bag.add(8130804).unwrap();
    }
    run.bag = serde_json::to_string(&bag).unwrap();
    run.details = serde_json::to_string(&flow::Details {
        end_id: 101,
        compositions: BTreeMap::from([(8250015, 1), (8250017, 1), (8310810, 1)]),
        consume_power: 48,
        ..Default::default()
    })
    .unwrap();
    run.middle_state = serde_json::to_string(&MiddleState {
        pieces: vec![],
        middle_layer_id: 151,
        entrust_id: 0,
        entrust_progress: 0,
        completed_entrusts: vec![1310233, 1110207],
    })
    .unwrap();
    run.spent_coin = 1160;
    run.terminal_state = 1;
    flow::save(db, &expected, &run).await.unwrap();
    sqlx::query("UPDATE user_rouge_run SET layer_index = 4, minor_count = 3, danger_count = 5, crisis_count = 2, step_count = 27 WHERE user_id = 1 AND season = 1")
        .execute(db).await.unwrap();
    sqlx::query(
        "UPDATE user_rouge_start SET genius_remainder = 410 WHERE user_id = 1 AND season = 1",
    )
    .execute(db)
    .await
    .unwrap();
    rouge::get_run(db, 1, 1).await.unwrap().unwrap()
}

#[tokio::test]
async fn second_area_failure_counts_one_cleared_area_and_spends_two_favors() {
    let (db, manager) = started().await;
    let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let graph = super::generate_layer(122, run.seed as u64);
    let first = graph.start_node().unwrap();
    sqlx::query("UPDATE user_rouge_run SET layer_index = 1, layer_id = 122, stage = ?, node_id = ?, graph = ?, map_type = 1 WHERE user_id = 1 AND season = 1")
        .bind(first.stage).bind(first.node_id).bind(serde_json::to_string(&graph).unwrap())
        .execute(&db).await.unwrap();
    assert_eq!(extra_rewards::balance(&db, 1, 1).await.unwrap(), 8);
    manager.abort_setup(&db, 1).await.unwrap();
    let (_, result) = manager.end(&db, 1).await.unwrap();
    // Captured failure after finishing area one: 750 credit, two extra tokens.
    assert_eq!(result.layer2_score.as_deref(), Some("1#750"));
    assert_eq!(result.extra_add_point, Some(2));
    assert_eq!(extra_rewards::balance(&db, 1, 1).await.unwrap(), 6);
}

#[tokio::test]
async fn failure_before_clearing_any_area_does_not_spend_weekly_favors() {
    let (db, manager) = started().await;
    let before = extra_rewards::balance(&db, 1, 1).await.unwrap();
    manager.abort_setup(&db, 1).await.unwrap();
    let (_, result) = manager.end(&db, 1).await.unwrap();
    assert_eq!(result.layer2_score.as_deref(), Some("0#0"));
    assert_eq!(result.extra_add_point, Some(0));
    assert_eq!(extra_rewards::balance(&db, 1, 1).await.unwrap(), before);
}

#[tokio::test]
async fn captured_three_plus_eight_tokens_credit_eleven_and_replayed_end_does_not_spend() {
    let (db, manager) = started().await;
    seed_balance(&db, 14).await;
    captured_clear(&db).await;
    let (snapshot, result) = manager.end(&db, 1).await.unwrap();
    assert_eq!(result.final_score, Some(8990));
    assert_eq!(result.add_point, Some(3));
    assert_eq!(result.extra_add_point, Some(8));
    let outside = manager
        .original_outside_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!(outside.cur_extra_point, Some(6));
    assert_eq!(outside.point, Some(11));
    assert_eq!(outside.have_get_point, Some(11));
    assert_eq!(outside.genius_point, Some(13));
    let replay = manager.end(&db, 1).await.unwrap();
    assert_eq!(replay, (snapshot, result));
    let after = manager
        .original_outside_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!(
        (
            after.cur_extra_point,
            after.point,
            after.have_get_point,
            after.genius_point
        ),
        (Some(6), Some(11), Some(11), Some(13))
    );
}

async fn attempt_stale_settlement(
    db: &SqlitePool,
    seed: i64,
    terminal: i32,
    info: &RougeInfo,
    result: &sonettobuf::RougeResultInfo,
) -> bool {
    rouge::settle_run(
        db,
        1,
        1,
        1,
        true,
        result.final_score.unwrap(),
        result.remain_score2_point.unwrap(),
        result.remain_score2_genius_point.unwrap(),
        result.add_genius_point.unwrap(),
        "[21]",
        &result.finish_entrust_id,
        result.add_point.unwrap(),
        &serde_json::to_string(info).unwrap(),
        &serde_json::to_string(result).unwrap(),
        0,
        0,
        seed,
        terminal,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn zero_balance_stale_end_cannot_settle_or_delete_the_next_run() {
    let (db, manager) = started().await;
    seed_balance(&db, 0).await;
    let old = captured_clear(&db).await;
    let (old_info, old_result) = manager.end(&db, 1).await.unwrap();
    assert_eq!(old_result.extra_add_point, Some(0));
    let pending = manager
        .original_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    manager
        .select_reward(&db, 1, &[pending.last_reward[0].id.unwrap()])
        .await
        .unwrap();
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    manager.select_style(&db, 1, 1).await.unwrap();
    manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
    // Ensure identity differs even on a machine that creates both runs in one
    // clock tick. The second run is active and has not earned a settlement.
    let new_seed = old.seed.wrapping_add(1);
    sqlx::query("UPDATE user_rouge_run SET seed = ? WHERE user_id = 1 AND season = 1")
        .bind(new_seed)
        .execute(&db)
        .await
        .unwrap();
    let before = manager
        .original_outside_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    let cached: (String, String) = sqlx::query_as("SELECT rouge_info, result_info FROM user_rouge_settlement WHERE user_id = 1 AND season = 1")
        .fetch_one(&db).await.unwrap();

    // An old request with zero extra cost passes the balance check, but its run
    // identity is stale. All tentative point credits and setup updates roll back.
    assert!(
        !attempt_stale_settlement(&db, old.seed, old.terminal_state, &old_info, &old_result).await
    );
    let active = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!((active.seed, active.terminal_state), (new_seed, 0));
    assert_eq!(
        manager
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap()
            .state,
        Some(4)
    );

    // Guard active state even if an erroneous caller supplies its current seed.
    assert!(!attempt_stale_settlement(&db, new_seed, 0, &old_info, &old_result).await);
    // Isolate the seed guard: a new terminal run still isn't the old run.
    sqlx::query("UPDATE user_rouge_run SET terminal_state = 1 WHERE user_id = 1 AND season = 1")
        .execute(&db)
        .await
        .unwrap();
    assert!(!attempt_stale_settlement(&db, old.seed, 1, &old_info, &old_result).await);
    // Isolate the terminal-state guard with the correct seed but wrong outcome.
    assert!(!attempt_stale_settlement(&db, new_seed, 2, &old_info, &old_result).await);

    let still_present = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(
        (still_present.seed, still_present.terminal_state),
        (new_seed, 1)
    );
    let after = manager
        .original_outside_info(&db, 1)
        .await
        .unwrap()
        .rouge_info
        .unwrap();
    assert_eq!(
        (
            after.point,
            after.have_get_point,
            after.genius_point,
            after.cur_extra_point
        ),
        (
            before.point,
            before.have_get_point,
            before.genius_point,
            before.cur_extra_point
        )
    );
    let after_cache: (String, String) = sqlx::query_as("SELECT rouge_info, result_info FROM user_rouge_settlement WHERE user_id = 1 AND season = 1")
        .fetch_one(&db).await.unwrap();
    assert_eq!(
        after_cache, cached,
        "failed stale requests cannot replace the replay snapshot"
    );
}
