use super::original_tests::{init_config, insert_hero, test_db};
use super::*;

async fn fixture(seed: i64) -> (SqlitePool, RougeManager) {
    init_config();
    let db = test_db().await;
    insert_hero(&db, 1, 1, 3005).await;
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    manager.select_style(&db, 1, 1).await.unwrap();
    manager.select_heroes(&db, 1, &[3005], 0).await.unwrap();
    let graph = map::LayerMap {
        layer_id: 111,
        stages: 3,
        nodes: vec![
            map::MapNode {
                stage: 0,
                node_id: 1,
                last: vec![],
                next: vec![4],
                event_id: 0,
                node_type: 0,
            },
            map::MapNode {
                stage: 1,
                node_id: 4,
                last: vec![1],
                next: vec![20],
                event_id: 106113,
                node_type: 6,
            },
            map::MapNode {
                stage: 2,
                node_id: 20,
                last: vec![4],
                next: vec![],
                event_id: 108101,
                node_type: 8,
            },
        ],
    };
    sqlx::query("UPDATE user_rouge_run SET seed=?,node_id=4,stage=1,graph=?,coin=1000,spent_coin=0 WHERE user_id=1")
        .bind(seed).bind(serde_json::to_string(&graph).unwrap()).execute(&db).await.unwrap();
    (db, manager)
}

#[tokio::test]
async fn three_real_fountain_wins_persist_menus_receipts_badge_and_cg_without_reapplying_drops() {
    let (db, manager) = fixture(7).await;
    for (event, choice, balance, next_event) in [
        (106113, 10611301, 1030, 1061131),
        (1061131, 10611311, 1090, 1061132),
        (1061132, 10611321, 1210, 1061132),
    ] {
        assert_eq!(
            config::configs::get().rouge_choices_for_event(event).len(),
            3
        );
        let wrong = if event == 106113 { 10611311 } else { 10611301 };
        assert!(
            manager
                .choice_event(&db, 1, 111, 4, event, wrong)
                .await
                .is_err()
        );
        manager
            .choice_event(&db, 1, 111, 4, event, choice)
            .await
            .unwrap();
        let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(
            saved.coin, balance,
            "the second stage uses the displayed +60, not the exported +80"
        );
        assert_eq!(saved.spent_coin, 0);
        let graph: map::LayerMap = serde_json::from_str(&saved.graph).unwrap();
        assert_eq!(graph.node(4).unwrap().event_id, next_event);
        assert_eq!(saved.interactive_index, -1);
        // A simulated disconnect reads exactly the committed roll/menu. Old
        // phase requests cannot repeat currency or rewind the current event.
        let _ = manager.choice_event(&db, 1, 111, 4, event, choice).await;
        let duplicate = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(duplicate.coin, saved.coin);
        assert_eq!(duplicate.details, saved.details);
    }
    let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let detail = flow::details(&saved);
    assert_eq!(detail.special_events.fountains["7:111:4"].outcomes.len(), 3);
    assert!(
        badges::evaluate(config::configs::get(), &badges::facts(&saved, 0, 0, 0))
            .iter()
            .any(|badge| badge.id == 19)
    );
    let outside = reviews::load(&db, 1, 1).await.unwrap();
    assert!(outside.unlock_illustration_ids.contains(&13));
}

#[tokio::test]
async fn fountain_losses_rollback_with_the_receipt_then_retry_the_same_roll_and_count_real_spending()
 {
    let (db, manager) = fixture(17).await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_fountain_test BEFORE UPDATE ON user_rouge_run WHEN NEW.coin<>OLD.coin BEGIN SELECT RAISE(ABORT,'Fountain rollback test'); END;")
        .execute(&db).await.unwrap();
    assert!(
        manager
            .choice_event(&db, 1, 111, 4, 106113, 10611301)
            .await
            .is_err()
    );
    let failed = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(
        (failed.coin, failed.spent_coin),
        (before.coin, before.spent_coin)
    );
    assert_eq!(failed.details, before.details);
    assert_eq!(failed.graph, before.graph);
    sqlx::query("DROP TRIGGER reject_fountain_test")
        .execute(&db)
        .await
        .unwrap();
    for (event, choice, balance, spent) in [
        (106113, 10611301, 980, 20),
        (1061131, 10611311, 940, 60),
        (1061132, 10611321, 840, 160),
    ] {
        manager
            .choice_event(&db, 1, 111, 4, event, choice)
            .await
            .unwrap();
        let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!((saved.coin, saved.spent_coin), (balance, spent));
    }
    let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let detail = flow::details(&saved);
    assert!(detail.achievements.successful_fountain_choices.is_empty());
    assert!(
        detail.special_events.fountains["17:111:4"]
            .outcomes
            .values()
            .all(|outcome| outcome.won == Some(false))
    );
    assert!(
        !badges::evaluate(config::configs::get(), &badges::facts(&saved, 0, 0, 0))
            .iter()
            .any(|badge| badge.id == 19)
    );
}
