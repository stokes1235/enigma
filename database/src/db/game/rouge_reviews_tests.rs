use super::*;

const MIGRATION: &str = include_str!("../../../migrations/125_rouge_reviews.sql");
const TIME: i64 = 1_794_480_000_000;

async fn legacy() -> SqlitePool {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE users(id INTEGER PRIMARY KEY); INSERT INTO users VALUES(1),(2),(3);
         CREATE TABLE user_rouge_settlement(user_id INTEGER,season INTEGER,rouge_info TEXT,result_info TEXT,PRIMARY KEY(user_id,season));
         CREATE TABLE user_rouge_run(
             user_id INTEGER,season INTEGER,layer_id INTEGER DEFAULT 111,
             map_type INTEGER DEFAULT 1,terminal_state INTEGER DEFAULT 0,
             node_id INTEGER DEFAULT 10,graph TEXT DEFAULT '{}',
             finished TEXT DEFAULT '[]',choices TEXT DEFAULT '{}',
             interactive_index INTEGER DEFAULT -1,updated_at INTEGER DEFAULT 0,
             PRIMARY KEY(user_id,season));"
    ).execute(&db).await.unwrap();
    db
}

fn result(end: i32, finish: i64) -> String {
    serde_json::to_string(&sonettobuf::RougeResultInfo {
        review_info: Some(sonettobuf::RougeReviewInfo {
            season: Some(1),
            difficulty: Some(1),
            style: Some(1),
            finish_time: Some(finish),
            end_id: Some(end),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap()
}

#[tokio::test]
async fn migration_keeps_the_only_legacy_report_without_inventing_a_seed_or_prior_routes() {
    let db = legacy().await;
    sqlx::query("INSERT INTO user_rouge_settlement VALUES(1,1,'{}',?)")
        .bind(result(101, TIME / 1000))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_rouge_settlement VALUES(2,1,'{}','bad json')")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_rouge_run(user_id,season,layer_id,graph,finished) VALUES(1,1,141,?, '[10]')")
        .bind(r#"{"nodes":[{"node_id":10,"event_id":106153}]}"#).execute(&db).await.unwrap();
    sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
    let rows = list(&db, 1, 1).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].run_seed, None);
    assert_eq!(rows[0].finish_time, TIME);
    assert!(list(&db, 2, 1).await.unwrap().is_empty());
    assert_eq!(passed_events(&db, 1, 1).await.unwrap(), vec![106153]);
    assert!(
        passed_layers(&db, 1, 1).await.unwrap().is_empty(),
        "a late layer number does not prove earlier layers"
    );
    let mut tx = db.begin().await.unwrap();
    assert!(
        record_review_in_transaction(&mut tx, 1, 1, 0, &result(0, TIME + 1), TIME + 1)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(
        list(&db, 1, 1).await.unwrap().len(),
        2,
        "a real seed0 is distinct from an unknown legacy seed"
    );
    let raw: String =
        sqlx::query_scalar("SELECT result_info FROM user_rouge_settlement WHERE user_id=2")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(raw, "bad json");
}

#[tokio::test]
async fn failure_appends_instead_of_replacing_and_concurrent_duplicates_keep_the_first_report() {
    let db = legacy().await;
    sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
    let append = |seed, end, time| {
        let db = &db;
        async move {
            let mut tx = db.begin().await.unwrap();
            let inserted =
                record_review_in_transaction(&mut tx, 1, 1, seed, &result(end, time), time)
                    .await
                    .unwrap();
            tx.commit().await.unwrap();
            inserted
        }
    };
    assert!(append(11, 101, TIME).await);
    assert!(append(12, 0, TIME + 1).await);
    let (first, second) = tokio::join!(append(13, 102, TIME + 2), append(13, 102, TIME + 2));
    assert_ne!(first, second);
    assert!(!append(11, 104, TIME + 3).await);
    let rows = list(&db, 1, 1).await.unwrap();
    assert_eq!(
        rows.iter().map(|row| row.run_seed).collect::<Vec<_>>(),
        vec![Some(13), Some(12), Some(11)]
    );
    let endings: Vec<_> = rows
        .iter()
        .map(|row| {
            serde_json::from_str::<sonettobuf::RougeReviewInfo>(&row.review_info)
                .unwrap()
                .end_id
                .unwrap()
        })
        .collect();
    assert_eq!(endings, vec![102, 0, 101]);
    assert!(list(&db, 2, 1).await.unwrap().is_empty());
    let mut tx = db.begin().await.unwrap();
    record_review_in_transaction(&mut tx, 1, 1, 14, &result(103, TIME + 4), TIME + 4)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(list(&db, 1, 1).await.unwrap().len(), 3);
}

#[tokio::test]
async fn events_wait_for_choice_rewards_and_survive_layer_changes_and_failed_runs() {
    let db = legacy().await;
    sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
    let graph = r#"{"nodes":[{"node_id":10,"event_id":1061511},{"node_id":20,"event_id":110109},{"node_id":30,"event_id":106153}]}"#;
    sqlx::query("INSERT INTO user_rouge_run(user_id,season,layer_id,graph,choices,interactive_index) VALUES(1,1,141,?,'{\"10\":10615111}',0)")
        .bind(graph).execute(&db).await.unwrap();
    assert!(
        passed_events(&db, 1, 1).await.unwrap().is_empty(),
        "pending choice rewards are not completed events"
    );
    sqlx::query("UPDATE user_rouge_run SET interactive_index=-1 WHERE user_id=1")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(passed_events(&db, 1, 1).await.unwrap(), vec![1061511]);
    sqlx::query(
        "UPDATE user_rouge_run SET node_id=20,finished='[20]',interactive_index=0 WHERE user_id=1",
    )
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(passed_events(&db, 1, 1).await.unwrap(), vec![1061511]);
    sqlx::query("UPDATE user_rouge_run SET interactive_index=-1,map_type=2 WHERE user_id=1")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        passed_events(&db, 1, 1).await.unwrap(),
        vec![110109, 1061511]
    );
    assert_eq!(passed_layers(&db, 1, 1).await.unwrap(), vec![141]);
    sqlx::query("UPDATE user_rouge_run SET layer_id=151,map_type=1,graph='{}',finished='[]',choices='{}',terminal_state=2 WHERE user_id=1").execute(&db).await.unwrap();
    sqlx::query("DELETE FROM user_rouge_run WHERE user_id=1")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        passed_events(&db, 1, 1).await.unwrap(),
        vec![110109, 1061511]
    );
    assert_eq!(passed_layers(&db, 1, 1).await.unwrap(), vec![141]);
    assert!(
        !passed_events(&db, 1, 1).await.unwrap().contains(&106153),
        "unvisited generated nodes never unlock CGs"
    );
}

#[tokio::test]
async fn event_facts_roll_back_and_bad_or_primitive_json_does_not_create_progress() {
    let db = legacy().await;
    sqlx::raw_sql(MIGRATION).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO user_rouge_run(user_id,season,graph,finished,choices) VALUES(1,1,'bad','bad','bad')")
        .execute(&db).await.unwrap();
    sqlx::query("UPDATE user_rouge_run SET graph='{\"nodes\":[1,\"text\",{\"node_id\":10,\"event_id\":\"106153\"}]}',finished='[10]' WHERE user_id=1")
        .execute(&db).await.unwrap();
    assert!(passed_events(&db, 1, 1).await.unwrap().is_empty());
    let mut tx = db.begin().await.unwrap();
    sqlx::query("UPDATE user_rouge_run SET graph='{\"nodes\":[{\"node_id\":10,\"event_id\":106153}]}',choices='{\"10\":10615301}',interactive_index=-1 WHERE user_id=1")
        .execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(passed_events(&db, 1, 1).await.unwrap().is_empty());
    mark_illustrations_seen(&db, 1, 1, &[43, 43]).await.unwrap();
    assert_eq!(seen_illustrations(&db, 1, 1).await.unwrap(), vec![43]);
}
