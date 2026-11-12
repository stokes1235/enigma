use super::*;
use crate::rouge::original_tests::{init_config, test_db};

fn tables() -> &'static config::GameDB {
    init_config();
    config::configs::get()
}

fn report(end: i32, time: i64) -> sonettobuf::RougeResultInfo {
    sonettobuf::RougeResultInfo {
        review_info: Some(RougeReviewInfo {
            season: Some(1),
            player_name: Some("local fixture".into()),
            player_level: Some(1),
            portrait: Some(171603),
            finish_time: Some(time),
            difficulty: Some(1),
            style: Some(1),
            team_level: Some(1),
            collection_num: Some(0),
            gain_coin: Some(0),
            team_info: Some(sonettobuf::RougeTeamInfo::default()),
            end_id: Some(end),
            layer_id: Some(141),
            middle_layer_id: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn factions_follow_their_real_growth_and_ending_requirements() {
    let tables = tables();
    assert_eq!(unlocked_styles(tables, 1, &[], &[]), vec![1, 2, 3, 4]);
    assert_eq!(unlocked_styles(tables, 1, &[], &[999]), vec![1, 2, 3, 4]);
    assert_eq!(
        unlocked_styles(tables, 1, &[401, 701], &[102]),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    let first_ending = unlocked_styles(tables, 1, &[], &[101]);
    assert!(first_ending.contains(&9));
    assert!(!first_ending.contains(&8));
    assert!(
        !unlocked_styles(tables, 1, &[], &[]).contains(&10),
        "DLC faction remains excluded"
    );
    assert!(unlocked_styles(tables, 2, &[401, 701], &[101, 102]).is_empty());
}

#[test]
fn cg_unlocks_accept_the_documented_parent_or_child_event_and_nothing_unobserved() {
    let tables = tables();
    assert!(unlocked_illustrations(tables, 1, &[]).is_empty());
    for event in [106151, 1061511] {
        assert_eq!(unlocked_illustrations(tables, 1, &[event]), vec![43]);
    }
    for event in [106152, 1061521] {
        assert_eq!(unlocked_illustrations(tables, 1, &[event]), vec![44]);
    }
    assert_eq!(unlocked_illustrations(tables, 1, &[110109]), vec![38]);
    assert_eq!(unlocked_illustrations(tables, 1, &[106153]), vec![45]);
    assert!(unlocked_illustrations(tables, 1, &[-1]).is_empty());
}

#[tokio::test]
async fn multiple_reports_keep_failure_and_prior_success_and_normalize_legacy_seconds() {
    tables();
    let db = test_db().await;
    let time = 1_794_480_000_000;
    for (seed, end, finish) in [(1, 101, time / 1000), (2, 0, time + 1), (3, 102, time + 2)] {
        let mut tx = db.begin().await.unwrap();
        store::record_review_in_transaction(
            &mut tx,
            1,
            1,
            seed,
            &serde_json::to_string(&report(end, finish)).unwrap(),
            time + 2,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let progress = load(&db, 1, 1).await.unwrap();
    assert_eq!(
        progress
            .reviews
            .iter()
            .map(|review| review.end_id.unwrap())
            .collect::<Vec<_>>(),
        vec![102, 0, 101]
    );
    assert_eq!(progress.reviews[2].finish_time, Some(time));
    assert_eq!(progress.last_game_time, time + 2);
    assert!(
        progress.unlock_illustration_ids.is_empty(),
        "an ending report alone does not prove a generated event was completed"
    );
}

#[tokio::test]
async fn cg_red_dots_are_acquired_from_completed_events_and_marked_without_unlocking_others() {
    tables();
    let db = test_db().await;
    sqlx::query("INSERT INTO user_rouge_passed_events(user_id,season,event_id,first_observed_at) VALUES(1,1,1061511,0)")
        .execute(&db).await.unwrap();
    let progress = load(&db, 1, 1).await.unwrap();
    assert_eq!(progress.unlock_illustration_ids, vec![43]);
    assert_eq!(progress.new_illustration_ids, vec![43]);
    assert!(!mark_illustration_seen(&db, 1, 1, 44).await.unwrap());
    assert!(mark_illustration_seen(&db, 1, 1, 43).await.unwrap());
    assert!(
        load(&db, 1, 1)
            .await
            .unwrap()
            .new_illustration_ids
            .is_empty()
    );
    sqlx::query("INSERT INTO user_rouge_passed_events(user_id,season,event_id,first_observed_at) VALUES(1,1,106153,0)")
        .execute(&db).await.unwrap();
    assert_eq!(
        load(&db, 1, 1).await.unwrap().new_illustration_ids,
        vec![45]
    );
    assert!(mark_illustration_seen(&db, 1, 1, 0).await.unwrap());
    let progress = load(&db, 1, 1).await.unwrap();
    assert!(progress.new_illustration_ids.is_empty());
    assert_eq!(progress.unlock_illustration_ids, vec![43, 45]);
}

#[tokio::test]
async fn real_settlement_appends_distinct_runs_and_cached_replay_does_not_append_again() {
    tables();
    let db = test_db().await;
    crate::rouge::original_tests::insert_hero(&db, 1, 1, 3003).await;
    let manager = crate::rouge::RougeManager::new(1);
    for index in 0..2 {
        manager
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        manager.select_style(&db, 1, 1).await.unwrap();
        manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
        // Explicit identities keep this independent of clock granularity.
        sqlx::query("UPDATE user_rouge_run SET seed=? WHERE user_id=1 AND season=1")
            .bind(100_i64 + index)
            .execute(&db)
            .await
            .unwrap();
        manager.abort_setup(&db, 1).await.unwrap();
        let first = manager.end(&db, 1).await.unwrap();
        assert_eq!(manager.end(&db, 1).await.unwrap(), first);
        let progress = load(&db, 1, 1).await.unwrap();
        assert_eq!(progress.reviews.len(), index as usize + 1);
        assert!(
            progress
                .reviews
                .iter()
                .all(|review| review.end_id == Some(0))
        );
        assert!(
            progress
                .reviews
                .iter()
                .all(|review| review.finish_time.unwrap() > 100_000_000_000)
        );
        assert!(progress.pass_event_ids.is_empty());
        // The next-run reward selector is unrelated to report persistence.
        sqlx::query(
            "UPDATE user_rouge_start SET state=0,rewards='[]' WHERE user_id=1 AND season=1",
        )
        .execute(&db)
        .await
        .unwrap();
    }
    assert_eq!(store::list(&db, 1, 1).await.unwrap().len(), 2);
}
