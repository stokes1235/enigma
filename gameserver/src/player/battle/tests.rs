use super::*;
use flate2::read::GzDecoder;

fn runtime(fight: sonettobuf::Fight) -> ::battle::Battle {
    ::battle::tooling::unstarted_battle(
        ::battle::catalog::BattleCatalog::new(config::configs::get()),
        fight,
    )
}

#[test]
fn team_level_is_absent_when_no_entity_has_a_level() {
    let empty = sonettobuf::FightTeam::default();
    assert_eq!(average_team_level(&empty), None);

    let team = sonettobuf::FightTeam {
        entitys: vec![
            sonettobuf::FightEntityInfo {
                level: Some(10),
                ..Default::default()
            },
            sonettobuf::FightEntityInfo::default(),
            sonettobuf::FightEntityInfo {
                level: Some(20),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(average_team_level(&team), Some(15));
}

#[test]
fn replay_record_keeps_cloth_and_card_operations_in_the_same_round() {
    let mut active = ActiveBattle::default();
    active.pending_cloth_skill_opers.push(UseClothSkillRequest {
        skill_id: Some(12),
        ..Default::default()
    });

    active.record_round(BeginRoundRequest {
        opers: vec![sonettobuf::BeginRoundOper::default()],
        ..Default::default()
    });

    let records = active.oper_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].cloth_skill_opers[0].skill_id, Some(12));
    assert_eq!(records[0].opers.len(), 1);
    assert!(active.pending_cloth_skill_opers.is_empty());
}

#[test]
fn replay_mode_is_projected_from_the_active_battle() {
    let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
    let _ = config::init(&data_dir);
    let active = ActiveBattle {
        is_replay: Some(true),
        runtime: runtime(sonettobuf::Fight {
            is_record: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(active.start_reply().fight.unwrap().is_record, Some(true));
    assert_eq!(
        active.reconnect_reply().fight.unwrap().is_record,
        Some(true)
    );
}

#[tokio::test]
async fn replay_uses_the_saved_battle_seed() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE dungeon_records (user_id INTEGER, episode_id INTEGER, seed TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO dungeon_records VALUES (7, 60107, ?)")
        .bind(u64::MAX.to_string())
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        initial_battle_seed(&pool, 7, 60107, true).await.unwrap(),
        u64::MAX
    );
}

#[test]
fn begin_round_steps_use_the_clients_compressed_framing() {
    let reply = compress_round_steps(BeginRoundReply {
        round: Some(sonettobuf::FightRound {
            fight_step: vec![Default::default(), Default::default()],
            ..Default::default()
        }),
    })
    .unwrap();
    let round = reply.round.unwrap();

    assert_eq!(round.total_step, Some(2));
    assert!(round.fight_step.is_empty());

    let compressed = round.fight_step_bytes.unwrap();
    let mut decoder = GzDecoder::new(compressed.as_slice());
    let mut framed = Vec::new();
    decoder.read_to_end(&mut framed).unwrap();
    assert_eq!(framed, [0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn active_fight_reconnects_from_its_committed_checkpoint() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
                    let _ = config::init(&data_dir);
                    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
                    database::run_migrations(&pool).await.unwrap();
                    sqlx::query(
                        "INSERT INTO users (id, username, created_at, updated_at)
                         VALUES (9, 'reconnect', 0, 0)",
                    )
                    .execute(&pool)
                    .await
                    .unwrap();
                    database::db::starter_data::load_all_starter_data(&pool, 9)
                        .await
                        .unwrap();

                    let mut active = ActiveBattle::prepare(
                        &pool,
                        9,
                        10002,
                        1002,
                        StartDungeonRequest {
                            chapter_id: Some(301),
                            episode_id: Some(10002),
                            fight_group: Some(sonettobuf::FightGroup {
                                cloth_id: Some(7),
                                ..Default::default()
                            }),
                            multiplication: Some(1),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                    active
                        .activate(&pool, 9, &RewardSet::default())
                        .await
                        .unwrap();
                    let fight_id = active.fight_id.unwrap();
                    let mut state = BattleState::default();
                    state.restore_active(active);

                    let before = format!("{:?}", state.active_snapshot());
                    let checkpoint_before = battle::load_active_fight(&pool, 9)
                        .await
                        .unwrap()
                        .unwrap()
                        .checkpoint;
                    let mut rejected = state.active_snapshot().unwrap();
                    rejected.multiplication = Some(2);
                    rejected.fight_id = Some(fight_id + 1);
                    assert!(state.commit_active(&pool, 9, rejected).await.is_err());
                    assert_eq!(format!("{:?}", state.active_snapshot()), before);
                    assert_eq!(
                        battle::load_active_fight(&pool, 9)
                            .await
                            .unwrap()
                            .unwrap()
                            .checkpoint,
                        checkpoint_before
                    );

                    let mut candidate = state.active_snapshot().unwrap();
                    candidate
                        .use_cloth_skill(UseClothSkillRequest {
                            skill_id: Some(30010701),
                            from_id: Some(0),
                            r#type: Some(0),
                            ..Default::default()
                        })
                        .unwrap();
                    state.commit_active(&pool, 9, candidate).await.unwrap();

                    let mut candidate = state.active_snapshot().unwrap();
                    candidate.begin_round(BeginRoundRequest::default()).unwrap();
                    state.commit_active(&pool, 9, candidate).await.unwrap();

                    let mut candidate = state.active_snapshot().unwrap();
                    candidate.begin_round(BeginRoundRequest::default()).unwrap();
                    state.commit_active(&pool, 9, candidate).await.unwrap();

                    let active = state.active_snapshot().unwrap();
                    let expected = active.reconnect_reply();
                    let expected_start = active.start_reply();
                    let expected_cards = active.card_info_push();
                    let expected_records = active.oper_records();
                    assert!(matches!(
                        BattleState::default().ensure_can_start(&pool, 9).await,
                        Err(AppError::InvalidRequest)
                    ));
                    assert!(
                        battle::create_fight_instance(
                            &pool,
                            battle::NewFightInstance {
                                user_id: 9,
                                episode_id: 10002,
                                battle_id: 1002,
                                multiplication: 1,
                                entry_cost: "{}",
                                checkpoint: "{}",
                                created_at: 0,
                            },
                        )
                        .await
                        .is_err()
                    );

                    let record = battle::load_active_fight(&pool, 9).await.unwrap().unwrap();
                    let restored = ActiveBattle::restore(&pool, 9, record).await.unwrap();

                    assert_eq!(restored.reconnect_reply(), expected);
                    assert_eq!(restored.start_reply(), expected_start);
                    assert_eq!(restored.card_info_push(), expected_cards);
                    assert_eq!(restored.oper_records(), expected_records);
                    battle::finish_fight_instance(&pool, 9, fight_id)
                        .await
                        .unwrap();
                    assert!(battle::load_active_fight(&pool, 9).await.unwrap().is_none());
                    BattleState::default()
                        .ensure_can_start(&pool, 9)
                        .await
                        .unwrap();
                });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn rouge_checkpoint_freezes_attributes_and_context_when_the_bag_changes() {
    std::thread::Builder::new().stack_size(32*1024*1024).spawn(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let data=format!("{}/../data/excel2json",env!("CARGO_MANIFEST_DIR"));config::init(&data).unwrap();
            let pool=SqlitePool::connect("sqlite::memory:").await.unwrap();database::run_migrations(&pool).await.unwrap();
            sqlx::query("INSERT INTO users (id,username,created_at,updated_at) VALUES (9,'rouge-reconnect',0,0)").execute(&pool).await.unwrap();
            database::db::starter_data::load_all_starter_data(&pool,9).await.unwrap();
            // Starter progression does not grant an owned hero roster.
            sqlx::query("INSERT INTO heroes (uid,user_id,hero_id,create_time,level,exp,rank,breakthrough,skin,faith,active_skill_level,ex_skill_level) VALUES (9003003,9,3003,0,1,0,1,0,0,0,1,0),(9003051,9,3051,0,1,0,1,0,0,0,1,0)").execute(&pool).await.unwrap();
            let hero=3003;
            let uid:i64=sqlx::query_scalar("SELECT uid FROM heroes WHERE user_id=9 AND hero_id=?").bind(hero).fetch_one(&pool).await.unwrap();
            let manager=crate::logic::rouge::RougeManager::new(9);
            manager.select_difficulty(&pool,1,1,&[],false).await.unwrap();manager.select_style(&pool,1,1).await.unwrap();
            manager.select_heroes(&pool,1,&[hero,3051],0).await.unwrap();
            manager.group_change(&pool,1,&[sonettobuf::RougeBattleHero {
                index:Some(1),hero_id:Some(hero),support_hero_id:Some(3051),support_hero_skill:Some(1),..Default::default()
            }]).await.unwrap();
            let before=database::db::game::rouge::get_run(&pool,9,1).await.unwrap().unwrap();
            let mut after=before.clone();let mut bag=crate::logic::rouge::bag::RunBag::from_layout("8#8","").unwrap();
            let heart=bag.add(8330020).unwrap();bag.place(heart.uid,0,0,0).unwrap();
            after.bag=serde_json::to_string(&bag).unwrap();crate::logic::rouge::flow::save_action(&pool,&before,&mut after).await.unwrap();
            let episode=config::configs::get().rouge_fight_episode(101104).unwrap();
            let battle=config::configs::get().episode.get(episode).unwrap().battle_id;
            let mut active=ActiveBattle::prepare(&pool,9,episode,battle,StartDungeonRequest {
                chapter_id:Some(crate::rouge::fight::ROUGE_CHAPTER_ID),episode_id:Some(episode),
                fight_group:Some(sonettobuf::FightGroup {hero_list:vec![uid],..Default::default()}),..Default::default()
            }).await.unwrap();
            assert!(active.rouge_snapshot.as_ref().unwrap().context.as_ref().unwrap().extra_attributes.contains(&(uid,217,1000)));
            let supports=&active.rouge_snapshot.as_ref().unwrap().context.as_ref().unwrap().support_skills;
            assert_eq!(supports.len(),1);assert_eq!(supports[0].owner_uid,uid);assert_eq!(supports[0].hero_id,3051);
            assert_eq!(supports[0].skill_ids,::battle::hero::active_skills(config::configs::get(),3051,1,0).0);
            let initial_fight=&active.rouge_snapshot.as_ref().unwrap().fight;
            let ex=initial_fight.attacker.as_ref().unwrap().ex_team_str.as_deref().unwrap();
            let projected:serde_json::Value=serde_json::from_str(ex.split('#').nth(3).unwrap()).unwrap();
            assert_eq!(projected[uid.to_string()]["skill1"],serde_json::json!(supports[0].skill_ids));
            active.activate(&pool,9,&RewardSet::default()).await.unwrap();
            let expected=active.reconnect_reply();let start=active.start_reply();let cards=active.card_info_push();
            let before=database::db::game::rouge::get_run(&pool,9,1).await.unwrap().unwrap();let mut after=before.clone();
            bag.remove(heart.uid).unwrap();let basket=bag.add(8320049).unwrap();bag.place(basket.uid,0,0,0).unwrap();
            after.bag=serde_json::to_string(&bag).unwrap();after.coin=10000;
            crate::logic::rouge::flow::save_action(&pool,&before,&mut after).await.unwrap();
            manager.group_change(&pool,1,&[sonettobuf::RougeBattleHero {
                index:Some(1),hero_id:Some(hero),support_hero_id:Some(3051),support_hero_skill:Some(2),..Default::default()
            }]).await.unwrap();
            let record=battle::load_active_fight(&pool,9).await.unwrap().unwrap();
            let restored=ActiveBattle::restore(&pool,9,record).await.unwrap();
            assert_eq!(restored.reconnect_reply(),expected);assert_eq!(restored.start_reply(),start);assert_eq!(restored.card_info_push(),cards);
            assert_eq!(restored.rouge_snapshot.as_ref().unwrap().context,active.rouge_snapshot.as_ref().unwrap().context);
        });
    }).unwrap().join().unwrap();
}

#[tokio::test]
async fn malformed_checkpoint_is_the_only_discardable_restore_error() {
    let legacy: BattleCheckpoint = serde_json::from_str(
        r#"{
            "chapter_id": 301,
            "start_request": {},
            "seed": 7,
            "tower_context": null,
            "act229_context": null
        }"#,
    )
    .unwrap();
    assert!(legacy.rounds.is_empty());
    assert!(legacy.pending_cloth_skill_opers.is_empty());

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let error = ActiveBattle::restore(
        &pool,
        9,
        database::db::game::battle::ActiveFightRecord {
            id: 1,
            episode_id: 10002,
            battle_id: 1002,
            multiplication: 1,
            entry_cost: "{}".into(),
            checkpoint: "{".into(),
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(error, AppError::InvalidBattleCheckpoint(_)));
}

#[tokio::test]
async fn activation_rolls_back_cost_when_an_active_fight_already_exists() {
    let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
    let _ = config::init(&data_dir);
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (9, 'activation', 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO currencies (user_id, currency_id, quantity, last_recover_time)
         VALUES (9, 4, 5, ?)",
    )
    .bind(common::time::ServerTime::now_ms())
    .execute(&pool)
    .await
    .unwrap();
    let existing_fight = battle::create_fight_instance(
        &pool,
        battle::NewFightInstance {
            user_id: 9,
            episode_id: 10002,
            battle_id: 1002,
            multiplication: 1,
            entry_cost: "{}",
            checkpoint: "{}",
            created_at: 0,
        },
    )
    .await
    .unwrap();

    let mut active = ActiveBattle {
        chapter_id: 301,
        episode_id: 10002,
        battle_id: 1002,
        seed: 7,
        start_request: Some(StartDungeonRequest {
            chapter_id: Some(301),
            episode_id: Some(10002),
            fight_group: Some(Default::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(
        active
            .activate(&pool, 9, &reward::parse("2#4#3"))
            .await
            .is_err()
    );

    let quantity: i32 =
        sqlx::query_scalar("SELECT quantity FROM currencies WHERE user_id = 9 AND currency_id = 4")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(quantity, 5);
    assert!(active.fight_id.is_none());

    battle::finish_fight_instance(&pool, 9, existing_fight)
        .await
        .unwrap();
    active
        .activate(&pool, 9, &reward::parse("2#4#3"))
        .await
        .unwrap();
    let record = battle::load_active_fight(&pool, 9).await.unwrap().unwrap();
    let entry_cost: RewardSet = serde_json::from_str(&record.entry_cost).unwrap();
    assert_eq!(entry_cost.currencies, vec![(4, 3)]);
}
