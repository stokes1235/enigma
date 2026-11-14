use super::*;

#[test]
fn rouge_flow_new_commands_reach_handlers_and_decode_replies() {
    // Dispatch's debug future needs more stack than the default test thread.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(exercise_rouge_flow_wire());
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn exercise_rouge_flow_wire() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
    config::init(data_dir.to_str().unwrap()).unwrap();
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    let player_id = 979;
    sqlx::query("INSERT INTO users (id, username, created_at, updated_at) VALUES (?, 'rouge-flow-wire', 0, 0)")
                .bind(player_id).execute(&pool).await.unwrap();
    database::db::game::rouge::get_or_create_start(&pool, player_id, 1)
        .await
        .unwrap();
    database::db::game::rouge::get_or_create_outside(&pool, player_id, 1)
        .await
        .unwrap();
    sqlx::query("UPDATE user_rouge_outside SET point = 20 WHERE user_id = ? AND season = 1")
        .bind(player_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE user_rouge_start SET state = 4, difficulty = 7, style = 1 WHERE user_id = ? AND season = 1")
                .bind(player_id).execute(&pool).await.unwrap();
    let mut bag = logic::rouge::bag::RunBag::from_layout("8#8", "").unwrap();
    let a = bag.add(8150014).unwrap();
    let b = bag.add(8150014).unwrap();
    let graph = logic::rouge::map::LayerMap {
        layer_id: 111,
        stages: 0,
        nodes: vec![],
    };
    sqlx::query("INSERT INTO user_rouge_run (user_id, season, layer_id, map_type, middle_layer_id, position_index, seed, graph, bag, team_exp, step_count, updated_at) VALUES (?, 1, 111, 2, 111, 1, 42, ?, ?, 20, 10, 0)")
                .bind(player_id).bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&bag).unwrap())
                .execute(&pool).await.unwrap();
    let state = Box::leak(Box::new(AppState::new(pool, configs::get())));
    let (outbound, mut packets) = mpsc::channel(128);
    let mut ctx = ConnectionContext::new(outbound, state);
    ctx.player = Some(Player::new(player_id, PlayerState::new(player_id, 0)));
    let requests = [
        (
            CmdId::RougeMonsterFixAttrCmd,
            sonettobuf::RougeMonsterFixAttrRequest { season: Some(1) }.encode_to_vec(),
        ),
        (
            CmdId::ActiveTalentCmd,
            sonettobuf::ActiveTalentRequest {
                season: Some(1),
                index: Some(0),
            }
            .encode_to_vec(),
        ),
        (
            CmdId::RougeComposeCmd,
            sonettobuf::RougeComposeRequest {
                season: Some(1),
                compose_id: Some(13),
                consume_ids: vec![a.uid, b.uid],
            }
            .encode_to_vec(),
        ),
        (
            CmdId::RougeEndCmd,
            sonettobuf::RougeEndRequest { season: Some(1) }.encode_to_vec(),
        ),
        (
            CmdId::RougeGetUnlockCollectionsCmd,
            sonettobuf::RougeGetUnlockCollectionsRequest { season: Some(1) }.encode_to_vec(),
        ),
        (
            CmdId::RougeGetNewReddotInfoCmd,
            sonettobuf::RougeGetNewReddotInfoRequest { season: Some(1) }.encode_to_vec(),
        ),
        (
            CmdId::RougeMarkNewReddotCmd,
            sonettobuf::RougeMarkNewReddotRequest {
                season: Some(1),
                r#type: Some(1),
                id: Some(0),
            }
            .encode_to_vec(),
        ),
        (
            CmdId::RougeReceivePointBonusCmd,
            sonettobuf::RougeReceivePointBonusRequest {
                season: Some(1),
                bonus_id: Some(113),
            }
            .encode_to_vec(),
        ),
        (
            CmdId::RougeMarkBonusNewStageCmd,
            sonettobuf::RougeMarkBonusNewStageRequest { season: Some(1) }.encode_to_vec(),
        ),
        (
            CmdId::GetRougeOutsideInfoCmd,
            sonettobuf::GetRougeOutsideInfoRequest { season: Some(1) }.encode_to_vec(),
        ),
    ];
    for (sequence, (cmd, data)) in requests.into_iter().enumerate() {
        if cmd == CmdId::RougeEndCmd {
            sqlx::query(
                "UPDATE user_rouge_run SET terminal_state = 2 WHERE user_id = ? AND season = 1",
            )
            .bind(player_id)
            .execute(state.db)
            .await
            .unwrap();
        }
        let request = ClientPacket {
            sequence: sequence as i32 + 1,
            cmd_id: cmd as i16,
            up_tag: 45,
            data,
        }
        .encode();
        dispatch_command(&mut ctx, request).await.unwrap();
        let mut reply = None;
        while let Ok(packet) = packets.try_recv() {
            if let CommandPacket::Reply {
                cmd_id,
                body,
                result_code,
                up_tag,
                ..
            } = packet
            {
                assert_eq!(cmd_id, cmd);
                assert_eq!(result_code, 0);
                assert_eq!(up_tag, 45);
                reply = Some(body);
            }
        }
        let body = reply.expect("request must receive a reply");
        match cmd {
            CmdId::RougeMonsterFixAttrCmd => {
                let reply = sonettobuf::RougeMonsterFixAttrReply::decode(&*body).unwrap();
                assert_eq!(reply.fix_hp_rate.as_deref(), Some("0.25"));
                let start = database::db::game::rouge::get_or_create_start(state.db, player_id, 1)
                    .await
                    .unwrap();
                let run = database::db::game::rouge::get_run(state.db, player_id, 1)
                    .await
                    .unwrap()
                    .unwrap();
                let profile =
                    logic::rouge::difficulty::for_run(configs::get(), &start, &run).unwrap();
                let mut fight = sonettobuf::Fight {
                    defender: Some(sonettobuf::FightTeam {
                        entitys: vec![sonettobuf::FightEntityInfo {
                            uid: Some(-1),
                            entity_type: Some(2),
                            model_id: Some(801110301),
                            current_hp: Some(1000),
                            attr: Some(sonettobuf::HeroAttribute {
                                hp: Some(1000),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                logic::rouge::difficulty::apply_to_defenders(
                    configs::get(),
                    &mut fight,
                    &mut vec![],
                    &mut vec![],
                    &profile,
                );
                assert_eq!(
                    fight.defender.unwrap().entitys[0].attr.unwrap().hp,
                    Some(1250)
                );
                // The remaining independent command tests retain their original setup.
                sqlx::query(
                    "UPDATE user_rouge_start SET difficulty = 1 WHERE user_id = ? AND season = 1",
                )
                .bind(player_id)
                .execute(state.db)
                .await
                .unwrap();
                sqlx::query(
                    "UPDATE user_rouge_run SET step_count = 0 WHERE user_id = ? AND season = 1",
                )
                .bind(player_id)
                .execute(state.db)
                .await
                .unwrap();
            }
            CmdId::ActiveTalentCmd => assert_eq!(
                sonettobuf::ActiveTalentReply::decode(&*body)
                    .unwrap()
                    .rouge_talent_tree
                    .unwrap()
                    .rouge_talent[0]
                    .is_active,
                Some(1)
            ),
            CmdId::RougeComposeCmd => assert_eq!(
                sonettobuf::RougeComposeReply::decode(&*body)
                    .unwrap()
                    .item
                    .unwrap()
                    .item_id,
                Some(8250015)
            ),
            CmdId::RougeEndCmd => {
                let reply = sonettobuf::RougeEndReply::decode(&*body).unwrap();
                assert_eq!(reply.rouge_info.unwrap().state, Some(5));
                assert_eq!(
                    reply.result_info.unwrap().compose_res2_num_str.as_deref(),
                    Some("8250015#1")
                );
            }
            CmdId::RougeGetUnlockCollectionsCmd => {
                let reply = sonettobuf::RougeGetUnlockCollectionsReply::decode(&*body).unwrap();
                assert!(reply.unlock_collection_ids.contains(&8110021));
                assert!(!reply.unlock_collection_ids.contains(&8110410));
            }
            CmdId::RougeGetNewReddotInfoCmd => {
                assert_eq!(
                    sonettobuf::RougeGetNewReddotInfoReply::decode(&*body)
                        .unwrap()
                        .new_reddots
                        .len(),
                    4
                );
            }
            CmdId::RougeMarkNewReddotCmd => {
                assert_eq!(
                    sonettobuf::RougeMarkNewReddotReply::decode(&*body)
                        .unwrap()
                        .id,
                    Some(0)
                );
            }
            CmdId::GetRougeOutsideInfoCmd => {
                let info = sonettobuf::GetRougeOutsideInfoReply::decode(&*body)
                    .unwrap()
                    .rouge_info
                    .unwrap();
                assert!(
                    info.game_record_info
                        .unwrap()
                        .pass_collections
                        .contains(&8250015)
                );
                let bonus = info.bonus.unwrap();
                assert_eq!(bonus.is_new_stage, Some(false));
                assert_eq!(bonus.bonus_stages[0].bonus_ids, vec![111, 112, 113]);
            }
            CmdId::RougeReceivePointBonusCmd => {
                let reply = sonettobuf::RougeReceivePointBonusReply::decode(&*body).unwrap();
                assert_eq!(reply.bonus_id, Some(113));
                assert_eq!(reply.bonus_stage.unwrap().bonus_ids, vec![111, 112, 113]);
            }
            CmdId::RougeMarkBonusNewStageCmd => {
                assert_eq!(
                    sonettobuf::RougeMarkBonusNewStageReply::decode(&*body)
                        .unwrap()
                        .season,
                    Some(1)
                );
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn settlement_acknowledgements_reach_handlers_and_clear_their_flags() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    config::init(data_dir.to_str().unwrap()).unwrap();
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    let player_id = 526;
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (?, 'weekwalk-shallow-ack', 0, 0)",
    )
    .bind(player_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_weekwalk_info
         (user_id, issue_id, is_pop_deep_rule, is_pop_shallow_settle, is_pop_deep_settle)
         VALUES (?, 59, TRUE, TRUE, TRUE)",
    )
    .bind(player_id)
    .execute(&pool)
    .await
    .unwrap();

    let state = Box::leak(Box::new(AppState::new(pool, configs::get())));
    let (outbound, mut packets) = mpsc::channel(2);
    let mut ctx = ConnectionContext::new(outbound, state);
    ctx.player = Some(Player::new(player_id, PlayerState::new(player_id, 0)));

    for (sequence, up_tag) in [(1, 41), (2, 42)] {
        let mut data = Vec::new();
        MarkPopShallowSettleRequest {}.encode(&mut data).unwrap();
        let request = ClientPacket {
            sequence,
            cmd_id: CmdId::MarkPopShallowSettleCmd as i16,
            up_tag,
            data,
        }
        .encode();

        dispatch_command(&mut ctx, request).await.unwrap();

        let CommandPacket::Reply {
            cmd_id: CmdId::MarkPopShallowSettleCmd,
            body,
            up_tag: reply_up_tag,
            ..
        } = packets.try_recv().unwrap()
        else {
            panic!("shallow-settlement acknowledgement did not reach its handler");
        };
        assert_eq!(reply_up_tag, up_tag);
        MarkPopShallowSettleReply::decode(&*body).unwrap();
        assert!(packets.try_recv().is_err());
    }

    let info = ctx
        .player()
        .unwrap()
        .exploration
        .weekwalk_info(ctx.state.db)
        .await
        .unwrap()
        .info
        .unwrap();
    assert_eq!(info.issue_id, Some(59));
    assert_eq!(info.is_pop_shallow_settle, Some(false));
    assert_eq!(info.is_pop_deep_rule, Some(true));
    assert_eq!(info.is_pop_deep_settle, Some(true));

    let mut data = Vec::new();
    MarkPopDeepSettleRequest {}.encode(&mut data).unwrap();
    let request = ClientPacket {
        sequence: 3,
        cmd_id: CmdId::MarkPopDeepSettleCmd as i16,
        up_tag: 43,
        data,
    }
    .encode();

    dispatch_command(&mut ctx, request).await.unwrap();

    let CommandPacket::Reply {
        cmd_id: CmdId::MarkPopDeepSettleCmd,
        body,
        result_code: 0,
        up_tag: 43,
        ..
    } = packets.try_recv().unwrap()
    else {
        panic!("deep-settlement acknowledgement did not reach its handler");
    };
    MarkPopDeepSettleReply::decode(&*body).unwrap();
    assert!(packets.try_recv().is_err());

    let info = ctx
        .player()
        .unwrap()
        .exploration
        .weekwalk_info(ctx.state.db)
        .await
        .unwrap()
        .info
        .unwrap();
    assert_eq!(info.is_pop_shallow_settle, Some(false));
    assert_eq!(info.is_pop_deep_settle, Some(false));
}

#[tokio::test]
async fn act220_info_command_reaches_handler_and_decodes_captured_initial_episode() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let activity_id = 13710;
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let state = Box::leak(Box::new(AppState::new(pool, configs::get())));
    let (outbound, mut packets) = mpsc::channel(1);
    let mut ctx = ConnectionContext::new(outbound, state);

    let mut data = Vec::new();
    GetAct220InfoRequest {
        activity_id: Some(activity_id),
    }
    .encode(&mut data)
    .unwrap();
    let request = ClientPacket {
        sequence: 1,
        cmd_id: CmdId::GetAct220InfoCmd as i16,
        up_tag: 52,
        data,
    }
    .encode();

    dispatch_command(&mut ctx, request).await.unwrap();

    let CommandPacket::Reply {
        cmd_id: CmdId::GetAct220InfoCmd,
        body,
        result_code: 0,
        up_tag: 52,
        ..
    } = packets.try_recv().unwrap()
    else {
        panic!("Act220 information request did not reach its handler");
    };
    let reply = GetAct220InfoReply::decode(&*body).unwrap();
    assert_eq!(
        reply,
        GetAct220InfoReply {
            activity_id: Some(activity_id),
            episodes: vec![Act220EpisodeRecord {
                episode_id: Some(1371001),
                is_finished: Some(false),
                unlock_branch_ids: Vec::new(),
                progress: Some(String::new()),
            }],
        }
    );
    assert!(packets.try_recv().is_err());
}

#[tokio::test]
async fn rouge2_outside_info_command_decodes_captured_initial_boss_state() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    let player_id = 534;
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (?, 'rouge2-boss-route', 0, 0)",
    )
    .bind(player_id)
    .execute(&pool)
    .await
    .unwrap();

    let state = Box::leak(Box::new(AppState::new(pool, configs::get())));
    let (outbound, mut packets) = mpsc::channel(1);
    let mut ctx = ConnectionContext::new(outbound, state);
    ctx.player = Some(Player::new(player_id, PlayerState::new(player_id, 0)));

    let mut data = Vec::new();
    GetRouge2OutsideInfoRequest {}.encode(&mut data).unwrap();
    let request = ClientPacket {
        sequence: 1,
        cmd_id: CmdId::GetRouge2OutsideInfoCmd as i16,
        up_tag: 54,
        data,
    }
    .encode();

    dispatch_command(&mut ctx, request).await.unwrap();

    let CommandPacket::Reply {
        cmd_id: CmdId::GetRouge2OutsideInfoCmd,
        body,
        result_code: 0,
        up_tag: 54,
        ..
    } = packets.try_recv().unwrap()
    else {
        panic!("Rouge2 outside information request did not reach its handler");
    };
    let reply = GetRouge2OutsideInfoReply::decode(&*body).unwrap();
    let mut expected_career_levels = configs::get()
        .rouge2_career
        .iter()
        .map(|row| Rouge2CareerLevelInfo {
            career_id: Some(row.id),
            exp: Some(0),
        })
        .collect::<Vec<_>>();
    expected_career_levels.sort_by_key(|row| row.career_id);
    let mut expected_rewards = configs::get()
        .rouge2_reward
        .iter()
        .map(|row| Rouge2RewardInfo {
            id: Some(row.id),
            buy_count: Some(0),
        })
        .collect::<Vec<_>>();
    expected_rewards.sort_by_key(|row| row.id);
    let mut expected_materials = configs::get()
        .rouge2_material
        .iter()
        .map(|row| Rouge2AlchemyMaterialInfo {
            id: Some(row.id),
            num: Some(0),
        })
        .collect::<Vec<_>>();
    expected_materials.sort_by_key(|row| row.id);
    assert_eq!(
        reply,
        GetRouge2OutsideInfoReply {
            outside_info: Some(Rouge2OutsideInfo {
                genius_point: Some(0),
                genius_ids: Vec::new(),
                total_record_info: Some(Rouge2TotalRecordInfo {
                    max_difficulty: Some(0),
                    pass_layer_id: Vec::new(),
                    pass_event_id: Vec::new(),
                    pass_end_id: Vec::new(),
                    pass_entrust_id: Vec::new(),
                    last_game_time: Some(0),
                    pass_collections: Vec::new(),
                    hotfix_str: Some(String::new()),
                }),
                career_level_info: expected_career_levels,
                reward_info: expected_rewards,
                reward_point: Some(0),
                alchemy_info: Some(Rouge2AlchemyInfo {
                    cur_alchemy_info: None,
                    alchemy_material_info: expected_materials,
                }),
                review: Vec::new(),
                boss_battle_info: Some(Rouge2BossBattleInfo {
                    boss_info: Vec::new(),
                    save_info: Vec::new(),
                    use_save_index: Some(0),
                }),
            }),
        }
    );
    assert!(packets.try_recv().is_err());
}
