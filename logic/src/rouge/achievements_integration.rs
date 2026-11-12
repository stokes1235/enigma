use super::original_tests::{init_config, insert_hero, test_db};
use super::*;

async fn started() -> (SqlitePool, RougeManager) {
    init_config();
    let db = test_db().await;
    insert_hero(&db, 1, 1, 3003).await;
    insert_hero(&db, 1, 2, 3005).await;
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    manager.select_style(&db, 1, 1).await.unwrap();
    manager
        .select_heroes(&db, 1, &[3003, 3005], 0)
        .await
        .unwrap();
    (db, manager)
}

#[tokio::test]
async fn obtained_artefacts_survive_consumption_and_inlaid_disks_count_once() {
    let (db, _) = started().await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let original = flow::details(&before).achievements.collection_uids.len();
    let mut next = before.clone();
    let mut bag: bag::RunBag = serde_json::from_str(&next.bag).unwrap();
    let holder = bag.add(8310405).unwrap();
    let disk = bag.add(8140401).unwrap();
    next.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut next).await.unwrap();
    assert_eq!(
        flow::details(&next).achievements.collection_uids.len(),
        original + 2
    );
    assert_eq!(flow::details(&next).achievements.disk_uids.len(), 1);
    let before = next.clone();
    // Move a real instance into a hole; ownership and obtained count persist.
    let holder = bag
        .items
        .iter_mut()
        .find(|item| item.uid == holder.uid)
        .unwrap();
    assert!(!holder.holes.is_empty());
    holder.holes[0] = disk.item_id;
    holder.hole_ids[0] = disk.uid;
    bag.items.retain(|item| item.uid != disk.uid);
    next.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut next).await.unwrap();
    assert_eq!(
        flow::details(&next).achievements.collection_uids.len(),
        original + 2
    );
    assert_eq!(flow::details(&next).achievements.disk_uids.len(), 1);
    let before = next.clone();
    bag.items.retain(|item| item.item_id != 8310405);
    next.bag = serde_json::to_string(&bag).unwrap();
    flow::save_action(&db, &before, &mut next).await.unwrap();
    let facts = badges::facts(&next, 0, 0, 0);
    assert_eq!(facts.collections as usize, original + 2);
    assert_eq!(facts.disks, 1);
    assert!(facts.collection_ids.contains(&8310405));
    assert!(flow::collection_counts(&bag).get(&8310405).is_none());
    assert_eq!(next.coin, before.coin);
}

#[tokio::test]
async fn revival_and_its_following_automatic_reward_commit_together_once() {
    let (db, manager) = started().await;
    let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut team: RougeTeamInfo = serde_json::from_str(&run.team).unwrap();
    team.hero_life_list
        .iter_mut()
        .find(|life| life.hero_id == Some(3005))
        .unwrap()
        .life = Some(0);
    let revive = config::configs::get()
        .rouge_interactive
        .iter()
        .find(|row| row.r#type == 4 && row.type_param == "1#0")
        .unwrap()
        .id;
    run.team = serde_json::to_string(&team).unwrap();
    run.battle_interactive = format!("4#{revive}|1#10610206|1#9002");
    run.middle_state=serde_json::json!({"pieces":[],"entrust_id":1110207,"entrust_progress":0,"completed_entrusts":[],"middle_layer_id":111}).to_string();
    run.interactive_index = 0;
    sqlx::query("UPDATE user_rouge_run SET team=?, battle_interactive=?,middle_state=?,interactive_index=0 WHERE user_id=1")
        .bind(&run.team).bind(&run.battle_interactive).bind(&run.middle_state).execute(&db).await.unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_revival_test BEFORE UPDATE ON user_rouge_run WHEN json_extract(NEW.details,'$.achievements.map_revives')=1 BEGIN SELECT RAISE(ABORT,'revival rollback test'); END;")
        .execute(&db).await.unwrap();
    assert!(manager.select_revive(&db, 1, &[3005]).await.is_err());
    let failed = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(failed.team, run.team);
    assert_eq!(failed.team_exp, run.team_exp);
    assert_eq!(failed.bag, run.bag);
    assert_eq!(failed.middle_state, run.middle_state);
    assert_eq!(flow::details(&failed).achievements.map_revives, 0);
    assert_eq!(failed.interactive_index, 0);
    sqlx::query("DROP TRIGGER reject_revival_test")
        .execute(&db)
        .await
        .unwrap();
    manager.select_revive(&db, 1, &[3005]).await.unwrap();
    let saved = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    assert_eq!(flow::details(&saved).achievements.map_revives, 1);
    assert_eq!(saved.team_exp, run.team_exp + 20);
    assert_eq!(
        flow::collection_counts(&serde_json::from_str(&saved.bag).unwrap())
            .values()
            .sum::<i32>(),
        flow::collection_counts(&serde_json::from_str(&run.bag).unwrap())
            .values()
            .sum::<i32>()
            + 1
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved.middle_state).unwrap()["entrust_progress"],
        1
    );
    assert_eq!(
        persisted_team(&saved)
            .unwrap()
            .hero_life_list
            .iter()
            .find(|life| life.hero_id == Some(3005))
            .unwrap()
            .life,
        Some(1000)
    );
    assert!(
        badges::evaluate(config::configs::get(), &badges::facts(&saved, 0, 0, 0))
            .iter()
            .any(|badge| badge.id == 7)
    );
    assert!(manager.select_revive(&db, 1, &[3005]).await.is_err());
    assert_eq!(
        rouge::get_run(&db, 1, 1).await.unwrap().unwrap().details,
        saved.details
    );
}

#[tokio::test]
async fn unlocked_growth_catalyst_can_be_selected_without_bypassing_setup_order() {
    init_config();
    let db = test_db().await;
    let manager = RougeManager::new(1);
    manager
        .select_difficulty(&db, 1, 1, &[], false)
        .await
        .unwrap();
    assert!(manager.select_style(&db, 1, 5).await.is_err());
    rouge::insert_genius(&db, 1, 1, 401).await.unwrap();
    assert_eq!(
        manager.select_style(&db, 1, 5).await.unwrap().style,
        Some(5)
    );
    assert!(manager.select_style(&db, 1, 6).await.is_err());
}

#[tokio::test]
async fn battle_achievement_reports_are_scoped_and_require_a_won_finale() {
    let (db, _) = started().await;
    let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
    let mut run = before.clone();
    let report = battle::rouge::BattleSummary {
        max_action_damage: 160000,
        ally_revives: 1,
        ..Default::default()
    };
    badges::record_battle(
        &mut run,
        7,
        Some(103206),
        true,
        true,
        &[3005],
        Some(&report),
    )
    .unwrap();
    badges::record_battle(
        &mut run,
        7,
        Some(103206),
        true,
        true,
        &[3003],
        Some(&report),
    )
    .unwrap();
    assert_eq!(flow::details(&run).achievements.battle_revives, 1);
    assert_eq!(
        flow::details(&run).achievements.final_party,
        Some(vec![3005])
    );
    let badge_ids = |run: &RougeRun| {
        badges::evaluate(config::configs::get(), &badges::facts(run, 0, 0, 0))
            .iter()
            .map(|badge| badge.id)
            .collect::<Vec<_>>()
    };
    for id in [6, 7, 13] {
        assert!(badge_ids(&run).contains(&id));
    }
    assert!(!badge_ids(&run).contains(&11));
    assert!(!badge_ids(&run).contains(&21));
    run.terminal_state = 1;
    assert!(badge_ids(&run).contains(&11));
    assert!(badge_ids(&run).contains(&21));
    flow::save_action(&db, &before, &mut run).await.unwrap();
    assert_eq!(
        flow::details(&rouge::get_run(&db, 1, 1).await.unwrap().unwrap())
            .achievements
            .max_action_damage,
        160000
    );
}
