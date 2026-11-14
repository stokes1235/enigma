use super::*;

#[tokio::test]
async fn placed_opening_artefact_excludes_warehouse_and_replays_on_reconnect() {
    let f = Fixture::new().await;
    let baseline = start_controlled(f.build().await, f.actor, 1_000_000)
        .card_info_push()
        .card_group
        .len();
    let mut bag: RunBag = serde_json::from_str(&f.run().await.bag).unwrap();
    let small = bag.add(8140301).unwrap().uid;
    bag.place(small, -1, -1, 0).unwrap();
    let stored_big = bag.add(8240302).unwrap().uid;
    f.store_bag(&bag).await;

    let built = f.build().await;
    let context = built.rouge.as_ref().unwrap();
    assert!(context.artefacts.iter().any(|item| item.uid == small));
    assert!(!context.artefacts.iter().any(|item| item.uid == stored_big));
    let actual = start_controlled(built, f.actor, 1_000_000);
    assert_eq!(actual.card_info_push().card_group.len(), baseline + 1);

    // Exercise the server's real checkpoint/replay boundary, not a clone of
    // the live battle. Changing the warehouse after entry must not add a
    // second opening bonus or alter the already dealt cards on reconnect.
    let request = sonettobuf::StartDungeonRequest {
        chapter_id: Some(ROUGE_CHAPTER_ID),
        episode_id: Some(f.episode),
        fight_group: Some(f.group.clone()),
        ..Default::default()
    };
    let mut active = ActiveBattle::prepare(&f.db, USER, f.episode, f.battle, request)
        .await
        .unwrap();
    active
        .activate(&f.db, USER, &Default::default())
        .await
        .unwrap();
    let original_cards = active.runtime.card_info_push().card_group;
    let original_state = active.runtime.reconnect_state();
    battle_db::update_fight_checkpoint(
        &f.db,
        USER,
        active.fight_id.unwrap(),
        &active.checkpoint_json().unwrap(),
    )
    .await
    .unwrap();
    bag.place(stored_big, -1, -1, 0).unwrap();
    f.store_bag(&bag).await;
    let record = battle_db::load_active_fight(&f.db, USER)
        .await
        .unwrap()
        .unwrap();
    let restored = ActiveBattle::restore(&f.db, USER, record).await.unwrap();
    assert_eq!(restored.runtime.card_info_push().card_group, original_cards);
    assert_eq!(restored.runtime.reconnect_state(), original_state);
    let context = restored.runtime.rouge_context().unwrap();
    assert!(context.artefacts.iter().any(|item| item.uid == small));
    assert!(!context.artefacts.iter().any(|item| item.uid == stored_big));
}
