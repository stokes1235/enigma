use crate::{
    error::AppError,
    net::{context::ConnectionContext, packet::ClientPacket},
};
use prost::Message;
use sonettobuf::{
    CmdId, EnterRougeSelectDifficultyReply, EnterRougeSelectDifficultyRequest,
    EnterRougeSelectHeroesReply, EnterRougeSelectHeroesRequest, EnterRougeSelectRewardReply,
    EnterRougeSelectRewardRequest, EnterRougeSelectStyleReply, EnterRougeSelectStyleRequest,
    GetRouge2InfoRequest, GetRouge2OutsideInfoRequest, GetRougeInfoRequest,
    GetRougeOutsideInfoRequest, Rouge2GetUnlockCollectionsRequest, RougeAbortReply,
    RougeAbortRequest, RougeActiveGeniusReply, RougeActiveGeniusRequest, RougeAddItemBagPush,
    RougeAddItemWarehousePush, RougeAddToBarReply, RougeAddToBarRequest, RougeBuyGoodsReply,
    RougeBuyGoodsRequest, RougeChoiceEventReply, RougeChoiceEventRequest, RougeDemountReply,
    RougeDemountRequest, RougeEndReply, RougeEndRequest, RougeEndShopEventReply,
    RougeEndShopEventRequest, RougeEntrustInfoPush, RougeGroupChangeReply, RougeGroupChangeRequest,
    RougeInlayReply, RougeInlayRequest, RougeItemUpdatePush, RougeLayerMapInfoPush,
    RougeLeaveMiddleLayerReply, RougeLeaveMiddleLayerRequest, RougeOneKeyAddToBarReply,
    RougeOneKeyAddToBarRequest, RougeOneKeyRemoveFromBarReply, RougeOneKeyRemoveFromBarRequest,
    RougePieceMoveReply, RougePieceMoveRequest, RougePieceTalkSelectReply,
    RougePieceTalkSelectRequest, RougeRecruitHeroReply, RougeRecruitHeroRequest,
    RougeRemoveFromBarReply, RougeRemoveFromBarRequest, RougeRemoveItemBagPush,
    RougeRemoveItemWarehousePush, RougeRoundMoveReply, RougeRoundMoveRequest, RougeSelectDropReply,
    RougeSelectDropRequest, RougeSelectHealReply, RougeSelectHealRequest,
    RougeSelectLostCollectionReply, RougeSelectLostCollectionRequest, RougeSelectReviveReply,
    RougeSelectReviveRequest, RougeShopRefreshReply, RougeShopRefreshRequest,
    RougeTeamHpUpdatePush, RougeTeamInfoPush, RougeUpdateCoinPush, RougeUpdatePowerPush,
    RougeUpdateTeamExpAndLevelPush,
};

pub async fn on_get_rouge_outside_info(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = GetRougeOutsideInfoRequest::decode(&req.data[..])?;
    let reply = ctx
        .player()?
        .rouge
        .original_outside_info(ctx.state.db, msg.season.unwrap_or_default())
        .await?;
    ctx.send_reply(CmdId::GetRougeOutsideInfoCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_get_rouge2_outside_info(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let _ = GetRouge2OutsideInfoRequest::decode(&req.data[..])?;
    let reply = ctx.player()?.rouge.outside_info(ctx.state.db).await?;
    ctx.send_reply(CmdId::GetRouge2OutsideInfoCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_rouge_receive_point_bonus(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeReceivePointBonusRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let player_id = ctx.player()?.id;
    let claim = ctx
        .player()?
        .rouge
        .receive_point_bonus(
            ctx.state.db,
            season,
            msg.bonus_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    crate::util::push::send_applied_reward_pushes(
        ctx,
        player_id,
        claim.outcome.rewards,
        claim.outcome.material_changes,
        None,
    )
    .await?;
    ctx.notify(
        CmdId::RougeUpdatePointPushCmd,
        sonettobuf::RougeUpdatePointPush {
            season: Some(season),
            point: Some(claim.point),
        },
    )
    .await?;
    ctx.send_reply(
        CmdId::RougeReceivePointBonusCmd,
        claim.outcome.reply,
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_mark_bonus_stage(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeMarkBonusNewStageRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    ctx.player()?
        .rouge
        .mark_bonus_stage(ctx.state.db, season)
        .await?;
    ctx.send_reply(
        CmdId::RougeMarkBonusNewStageCmd,
        sonettobuf::RougeMarkBonusNewStageReply {
            season: Some(season),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_unlock_collections(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeGetUnlockCollectionsRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let genius =
        database::db::game::rouge::list_genius(ctx.state.db, ctx.player()?.id, season).await?;
    let reply = crate::logic::rouge::favorites::unlock_collections_reply(
        config::configs::get(),
        season,
        &genius,
    )?;
    ctx.send_reply(CmdId::RougeGetUnlockCollectionsCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_rouge_get_reddots(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeGetNewReddotInfoRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let progress =
        crate::logic::rouge::favorites::load(ctx.state.db, ctx.player()?.id, season).await?;
    let reports =
        crate::logic::rouge::reviews::load(ctx.state.db, ctx.player()?.id, season).await?;
    let mut dots = progress.reddots();
    if let Some(cg) = dots.iter_mut().find(|dot| dot.r#type == Some(3)) {
        cg.ids = reports.new_illustration_ids;
    }
    ctx.send_reply(
        CmdId::RougeGetNewReddotInfoCmd,
        sonettobuf::RougeGetNewReddotInfoReply {
            season: Some(season),
            new_reddots: dots,
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_mark_reddot(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeMarkNewReddotRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let kind = msg.r#type.ok_or(AppError::InvalidRequest)?;
    let id = msg.id.ok_or(AppError::InvalidRequest)?;
    let marked = if kind == 3 {
        crate::logic::rouge::reviews::mark_illustration_seen(
            ctx.state.db,
            ctx.player()?.id,
            season,
            id,
        )
        .await?
    } else {
        database::db::game::rouge_favorites::mark_seen(
            ctx.state.db,
            ctx.player()?.id,
            season,
            kind,
            id,
        )
        .await?
    };
    if !marked {
        return Err(AppError::InvalidRequest);
    }
    ctx.send_reply(
        CmdId::RougeMarkNewReddotCmd,
        sonettobuf::RougeMarkNewReddotReply {
            season: Some(season),
            r#type: Some(kind),
            id: Some(id),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_unlock_story(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeUnlockStoryRequest::decode(&req.data[..])?;
    let reply = crate::logic::rouge::favorites::unlock_story(
        ctx.state.db,
        ctx.player()?.id,
        msg.season.ok_or(AppError::InvalidRequest)?,
        msg.story_id.ok_or(AppError::InvalidRequest)?,
    )
    .await?;
    ctx.send_reply(CmdId::RougeUnlockStoryCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_get_rouge2_info(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let _ = GetRouge2InfoRequest::decode(&req.data[..])?;
    let reply = ctx.player()?.rouge.info(ctx.state.db).await?;
    ctx.send_reply(CmdId::GetRouge2InfoCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_rouge2_get_unlock_collections(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let _ = Rouge2GetUnlockCollectionsRequest::decode(&req.data[..])?;
    let reply = ctx.player()?.rouge.unlock_collections(ctx.state.db).await?;
    ctx.send_reply(CmdId::Rouge2GetUnlockCollectionsCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_get_rouge_info(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = GetRougeInfoRequest::decode(&req.data[..])?;
    let info = ctx
        .player()?
        .rouge
        .original_info(ctx.state.db, msg.season.ok_or(AppError::InvalidRequest)?)
        .await?;
    ctx.send_reply(CmdId::GetRougeInfoCmd, info, 0, req.up_tag)
        .await
}

pub async fn on_enter_rouge_select_difficulty(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = EnterRougeSelectDifficultyRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let has_limiter = msg
        .limiter_no
        .as_ref()
        .is_some_and(|v| !v.limit_ids.is_empty() || !v.limit_buff_ids.is_empty());
    let info = ctx
        .player()?
        .rouge
        .select_difficulty(
            ctx.state.db,
            season,
            msg.difficulty.ok_or(AppError::InvalidRequest)?,
            &msg.version,
            has_limiter,
        )
        .await?;
    ctx.send_reply(
        CmdId::EnterRougeSelectDifficultyCmd,
        EnterRougeSelectDifficultyReply {
            season: Some(season),
            rouge_info: Some(info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_enter_rouge_select_style(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = EnterRougeSelectStyleRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let info = ctx
        .player()?
        .rouge
        .select_style(
            ctx.state.db,
            season,
            msg.style.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    ctx.send_reply(
        CmdId::EnterRougeSelectStyleCmd,
        EnterRougeSelectStyleReply {
            season: Some(season),
            rouge_info: Some(info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_enter_rouge_select_heroes(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = EnterRougeSelectHeroesRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let info = ctx
        .player()?
        .rouge
        .select_heroes(
            ctx.state.db,
            season,
            &msg.heroes_list,
            msg.assist_hero_uid.unwrap_or_default(),
        )
        .await?;
    ctx.send_reply(
        CmdId::EnterRougeSelectHeroesCmd,
        EnterRougeSelectHeroesReply {
            season: Some(season),
            rouge_info: Some(info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_group_change(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeGroupChangeRequest::decode(&req.data[..])?;
    let team_info = ctx
        .player()?
        .rouge
        .group_change(
            ctx.state.db,
            msg.season.ok_or(AppError::InvalidRequest)?,
            &msg.battle_hero_list,
        )
        .await?;
    ctx.send_reply(
        CmdId::RougeGroupChangeCmd,
        RougeGroupChangeReply {
            team_info: Some(team_info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_abort(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeAbortRequest::decode(&req.data[..])?;
    let info = ctx
        .player()?
        .rouge
        .abort_setup(ctx.state.db, msg.season.ok_or(AppError::InvalidRequest)?)
        .await?;
    ctx.send_reply(
        CmdId::RougeAbortCmd,
        RougeAbortReply {
            rouge_info: Some(info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_round_move(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeRoundMoveRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .round_move(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.node_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeRoundMoveCmd,
        RougeRoundMoveReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_piece_move(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougePieceMoveRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .piece_move(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.middle_layer.ok_or(AppError::InvalidRequest)?,
            msg.index.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    ctx.send_reply(
        CmdId::RougePieceMoveCmd,
        RougePieceMoveReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_piece_talk_select(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougePieceTalkSelectRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .piece_talk_select(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.middle_layer.ok_or(AppError::InvalidRequest)?,
            msg.index.ok_or(AppError::InvalidRequest)?,
            msg.select.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    if let Some(entrust_info) = map.rouge_entrust {
        ctx.notify(
            CmdId::RougeEntrustInfoPushCmd,
            RougeEntrustInfoPush {
                season: Some(season),
                entrust_info: Some(entrust_info),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougePieceTalkSelectCmd,
        RougePieceTalkSelectReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_leave_middle_layer(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeLeaveMiddleLayerRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .leave_middle_layer(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.middle_layer.ok_or(AppError::InvalidRequest)?,
            msg.next_layer.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    ctx.send_reply(
        CmdId::RougeLeaveMiddleLayerCmd,
        RougeLeaveMiddleLayerReply {
            create_event_map: Some(map),
            final_map: None,
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_choice_event(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeChoiceEventRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .choice_event(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.node_id.ok_or(AppError::InvalidRequest)?,
            msg.event_id.ok_or(AppError::InvalidRequest)?,
            msg.choice_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    if msg
        .choice_id
        .and_then(|id| config::configs::get().rouge_choice.get(id))
        .is_some_and(|choice| choice.interactive == "3#301")
    {
        let info = ctx
            .player()?
            .rouge
            .original_info(ctx.state.db, season)
            .await?;
        let team = info
            .rouge_info
            .and_then(|info| info.team_info)
            .ok_or(AppError::InvalidRequest)?;
        ctx.notify(
            CmdId::RougeTeamHpUpdatePushCmd,
            RougeTeamHpUpdatePush {
                season: Some(season),
                hero_life_list: team.hero_life_list,
            },
        )
        .await?;
    }
    if msg
        .choice_id
        .is_some_and(logic::rouge::is_verified_resource_choice)
    {
        notify_run_resources(ctx, season).await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeChoiceEventCmd,
        RougeChoiceEventReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_select_heal(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeSelectHealRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (team_info, map) = ctx
        .player()?
        .rouge
        .select_heal(ctx.state.db, season, &msg.hero_ids)
        .await?;
    ctx.notify(
        CmdId::RougeTeamInfoPushCmd,
        RougeTeamInfoPush {
            season: Some(season),
            team_info: Some(team_info.clone()),
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeLayerMapInfoPushCmd,
        RougeLayerMapInfoPush {
            season: Some(season),
            map: Some(map),
        },
    )
    .await?;
    notify_run_resources(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeSelectHealCmd,
        RougeSelectHealReply {
            team_info: Some(team_info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_select_lost_collection(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeSelectLostCollectionRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (map, placed, warehouse, updated) = ctx
        .player()?
        .rouge
        .select_lost_collection(ctx.state.db, season, &msg.collection_uid)
        .await?;
    if !placed.is_empty() {
        ctx.notify(
            CmdId::RougeRemoveItemBagPushCmd,
            RougeRemoveItemBagPush {
                season: Some(season),
                ids: placed,
            },
        )
        .await?;
    }
    if !warehouse.is_empty() {
        ctx.notify(
            CmdId::RougeRemoveItemWarehousePushCmd,
            RougeRemoveItemWarehousePush {
                season: Some(season),
                ids: warehouse,
            },
        )
        .await?;
    }
    if !updated.is_empty() {
        ctx.notify(
            CmdId::RougeItemUpdatePushCmd,
            RougeItemUpdatePush {
                season: Some(season),
                items: updated,
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeSelectLostCollectionCmd,
        RougeSelectLostCollectionReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_select_drop(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeSelectDropRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (map, collection_id, items, layouts) = ctx
        .player()?
        .rouge
        .select_drop(ctx.state.db, season, &msg.collection_pos)
        .await?;
    if !items.is_empty() {
        ctx.notify(
            CmdId::RougeAddItemWarehousePushCmd,
            RougeAddItemWarehousePush {
                season: Some(season),
                items,
                reason: Some(0),
            },
        )
        .await?;
    }
    if !layouts.is_empty() {
        ctx.notify(
            CmdId::RougeAddItemBagPushCmd,
            RougeAddItemBagPush {
                season: Some(season),
                layouts,
                reason: Some(0),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeRougeSelectDropCmd,
        RougeSelectDropReply {
            map: Some(map),
            collection_id,
        },
        0,
        req.up_tag,
    )
    .await
}

async fn notify_run_resources(ctx: &mut ConnectionContext, season: i32) -> Result<(), AppError> {
    let info = ctx
        .player()?
        .rouge
        .original_info(ctx.state.db, season)
        .await?
        .rouge_info
        .ok_or(AppError::InvalidRequest)?;
    ctx.notify(
        CmdId::RougeUpdateCoinPushCmd,
        RougeUpdateCoinPush {
            season: Some(season),
            coin: info.coin,
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeUpdatePowerPushCmd,
        RougeUpdatePowerPush {
            season: Some(season),
            power: info.power,
            power_limit: info.power_limit,
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeUpdateTeamExpAndLevelPushCmd,
        RougeUpdateTeamExpAndLevelPush {
            season: Some(season),
            team_level: info.team_level,
            team_exp: info.team_exp,
            team_size: info.team_size,
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeUpdateTalentPointPushCmd,
        sonettobuf::RougeUpdateTalentPointPush {
            season: Some(season),
            talent_point: info.talent_point,
        },
    )
    .await
}

async fn notify_full_run(ctx: &mut ConnectionContext, season: i32) -> Result<(), AppError> {
    let rouge_info = ctx
        .player()?
        .rouge
        .original_info(ctx.state.db, season)
        .await?
        .rouge_info;
    ctx.notify(
        CmdId::RougeInfoPushCmd,
        sonettobuf::RougeInfoPush { rouge_info },
    )
    .await?;
    notify_run_resources(ctx, season).await
}

pub async fn on_active_talent(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::ActiveTalentRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let tree = ctx
        .player()?
        .rouge
        .active_talent(
            ctx.state.db,
            season,
            msg.index.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::ActiveTalentCmd,
        sonettobuf::ActiveTalentReply {
            rouge_talent_tree: Some(tree),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_monster_fix_attr(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeMonsterFixAttrRequest::decode(&req.data[..])?;
    if msg.season != Some(1) {
        return Err(AppError::InvalidRequest);
    }
    let player_id = ctx.player()?.id;
    let run = database::db::game::rouge::get_run(ctx.state.db, player_id, 1).await?;
    let hp_rate = if let Some(run) = run {
        let start =
            database::db::game::rouge::get_or_create_start(ctx.state.db, player_id, 1).await?;
        logic::rouge::difficulty::hp_rate_for_run(config::configs::get(), &start, &run)?.max(0)
    } else {
        0
    };
    // The client uses 1 + fixHpRate on its own monster HP baseline. Keep the
    // exact per-mille increment from battle setup; client and server baseline
    // monster values can still differ for other reasons.
    let rate = if hp_rate == 0 {
        "0".to_owned()
    } else {
        let decimal = format!("{}.{:03}", hp_rate / 1000, hp_rate % 1000);
        decimal
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    };
    ctx.send_reply(
        CmdId::RougeMonsterFixAttrCmd,
        sonettobuf::RougeMonsterFixAttrReply {
            season: msg.season,
            fix_hp_rate: Some(rate),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_compose(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeComposeRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let item = ctx
        .player()?
        .rouge
        .compose(
            ctx.state.db,
            season,
            msg.compose_id.ok_or(AppError::InvalidRequest)?,
            &msg.consume_ids,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeComposeCmd,
        sonettobuf::RougeComposeReply {
            season: Some(season),
            compose_id: msg.compose_id,
            item: Some(item),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_recruit_hero(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeRecruitHeroRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (team, map) = ctx
        .player()?
        .rouge
        .recruit_heroes(ctx.state.db, season, &msg.hero_ids)
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.notify(
        CmdId::RougeLayerMapInfoPushCmd,
        RougeLayerMapInfoPush {
            season: Some(season),
            map: Some(map),
        },
    )
    .await?;
    ctx.send_reply(
        CmdId::RougeRecruitHeroCmd,
        RougeRecruitHeroReply {
            team_info: Some(team),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_select_revive(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeSelectReviveRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (team_info, map) = ctx
        .player()?
        .rouge
        .select_revive(ctx.state.db, season, &msg.hero_ids)
        .await?;
    ctx.notify(
        CmdId::RougeTeamInfoPushCmd,
        RougeTeamInfoPush {
            season: Some(season),
            team_info: Some(team_info.clone()),
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeLayerMapInfoPushCmd,
        RougeLayerMapInfoPush {
            season: Some(season),
            map: Some(map),
        },
    )
    .await?;
    notify_run_resources(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeSelectReviveCmd,
        RougeSelectReviveReply {
            team_info: Some(team_info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_add_to_bar(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeAddToBarRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let target_id = msg.target_id.ok_or(AppError::InvalidRequest)?;
    let pos = msg.pos.ok_or(AppError::InvalidRequest)?;
    let (layout, from_warehouse) = ctx
        .player()?
        .rouge
        .add_to_bar(
            ctx.state.db,
            season,
            target_id,
            pos.row.ok_or(AppError::InvalidRequest)?,
            pos.col.ok_or(AppError::InvalidRequest)?,
            msg.rotation.ok_or(AppError::InvalidRequest)?,
        )
        .await?;

    if from_warehouse {
        ctx.notify(
            CmdId::RougeRemoveItemWarehousePushCmd,
            RougeRemoveItemWarehousePush {
                season: Some(season),
                ids: vec![target_id],
            },
        )
        .await?;
    }
    ctx.notify(
        CmdId::RougeAddItemBagPushCmd,
        RougeAddItemBagPush {
            season: Some(season),
            layouts: vec![layout],
            reason: Some(0),
        },
    )
    .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeAddToBarCmd,
        RougeAddToBarReply {
            season: Some(season),
            id: Some(target_id),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_remove_from_bar(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeRemoveFromBarRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let target_id = msg.id.ok_or(AppError::InvalidRequest)?;
    let item = ctx
        .player()?
        .rouge
        .remove_from_bar(ctx.state.db, season, target_id)
        .await?;

    ctx.notify(
        CmdId::RougeRemoveItemBagPushCmd,
        RougeRemoveItemBagPush {
            season: Some(season),
            ids: vec![target_id],
        },
    )
    .await?;
    ctx.notify(
        CmdId::RougeAddItemWarehousePushCmd,
        RougeAddItemWarehousePush {
            season: Some(season),
            items: vec![item],
            reason: Some(0),
        },
    )
    .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeRemoveFromBarCmd,
        RougeRemoveFromBarReply {
            season: Some(season),
            id: Some(target_id),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_one_key_add_to_bar(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeOneKeyAddToBarRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let layouts = ctx
        .player()?
        .rouge
        .one_key_add_to_bar(ctx.state.db, season)
        .await?;
    let ids: Vec<i64> = layouts
        .iter()
        .filter_map(|layout| layout.item.as_ref()?.id)
        .collect();

    if !ids.is_empty() {
        ctx.notify(
            CmdId::RougeRemoveItemWarehousePushCmd,
            RougeRemoveItemWarehousePush {
                season: Some(season),
                ids: ids.clone(),
            },
        )
        .await?;
        ctx.notify(
            CmdId::RougeAddItemBagPushCmd,
            RougeAddItemBagPush {
                season: Some(season),
                layouts,
                reason: Some(0),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeOneKeyAddToBarCmd,
        RougeOneKeyAddToBarReply {
            season: Some(season),
            ids,
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_one_key_remove_from_bar(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeOneKeyRemoveFromBarRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let items = ctx
        .player()?
        .rouge
        .one_key_remove_from_bar(ctx.state.db, season)
        .await?;
    let ids: Vec<i64> = items.iter().filter_map(|item| item.id).collect();

    if !ids.is_empty() {
        ctx.notify(
            CmdId::RougeRemoveItemBagPushCmd,
            RougeRemoveItemBagPush {
                season: Some(season),
                ids,
            },
        )
        .await?;
        ctx.notify(
            CmdId::RougeAddItemWarehousePushCmd,
            RougeAddItemWarehousePush {
                season: Some(season),
                items,
                reason: Some(0),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeOneKeyRemoveFromBarCmd,
        RougeOneKeyRemoveFromBarReply {
            season: Some(season),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_inlay(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeInlayRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let consume_id = msg.consume_id.ok_or(AppError::InvalidRequest)?;
    let (item, pre_item, removed_from_warehouse, removed_from_bag, returned_to_warehouse) = ctx
        .player()?
        .rouge
        .inlay(
            ctx.state.db,
            season,
            msg.target_id.ok_or(AppError::InvalidRequest)?,
            msg.hold_id.ok_or(AppError::InvalidRequest)?,
            consume_id,
        )
        .await?;
    if removed_from_warehouse {
        ctx.notify(
            CmdId::RougeRemoveItemWarehousePushCmd,
            RougeRemoveItemWarehousePush {
                season: Some(season),
                ids: vec![consume_id],
            },
        )
        .await?;
    }
    if removed_from_bag {
        ctx.notify(
            CmdId::RougeRemoveItemBagPushCmd,
            RougeRemoveItemBagPush {
                season: Some(season),
                ids: vec![consume_id],
            },
        )
        .await?;
    }
    if let Some(returned) = returned_to_warehouse {
        ctx.notify(
            CmdId::RougeAddItemWarehousePushCmd,
            RougeAddItemWarehousePush {
                season: Some(season),
                items: vec![returned],
                reason: Some(0),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeInlayCmd,
        RougeInlayReply {
            season: Some(season),
            item: Some(item),
            pre_item,
            reason: Some(0),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_demount(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeDemountRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (item, enchant) = ctx
        .player()?
        .rouge
        .demount(
            ctx.state.db,
            season,
            msg.target_id.ok_or(AppError::InvalidRequest)?,
            msg.hold_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    ctx.notify(
        CmdId::RougeAddItemWarehousePushCmd,
        RougeAddItemWarehousePush {
            season: Some(season),
            items: vec![enchant],
            reason: Some(0),
        },
    )
    .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeDemountCmd,
        RougeDemountReply {
            season: Some(season),
            item: Some(item),
            reason: Some(0),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_end(ctx: &mut ConnectionContext, req: ClientPacket) -> Result<(), AppError> {
    let msg = RougeEndRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (info, result) = ctx.player()?.rouge.end(ctx.state.db, season).await?;
    ctx.send_reply(
        CmdId::RougeEndCmd,
        RougeEndReply {
            rouge_info: Some(info),
            result_info: Some(result),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_repair_shop_buy(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeRepairShopBuyRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let piece = ctx
        .player()?
        .rouge
        .repair_buy(
            ctx.state.db,
            season,
            msg.collection_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeRepairShopBuyCmd,
        sonettobuf::RougeRepairShopBuyReply {
            piece_info: Some(piece),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_repair_shop_random(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeRepairShopRandomRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let piece = ctx
        .player()?
        .rouge
        .repair_random(ctx.state.db, season)
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeRepairShopRandomCmd,
        sonettobuf::RougeRepairShopRandomReply {
            piece_info: Some(piece),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_displace(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = sonettobuf::RougeDisplaceRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (piece, id) = ctx
        .player()?
        .rouge
        .displace(
            ctx.state.db,
            season,
            msg.collection_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeDisplaceCmd,
        sonettobuf::RougeDisplaceReply {
            piece_info: Some(piece),
            collection_id: Some(id),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_shop_refresh(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeShopRefreshRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .shop_refresh(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.node_id.ok_or(AppError::InvalidRequest)?,
            msg.event_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_run_resources(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeShopRefreshCmd,
        RougeShopRefreshReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_buy_goods(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeBuyGoodsRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let (map, added_items) = ctx
        .player()?
        .rouge
        .buy_goods(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.node_id.ok_or(AppError::InvalidRequest)?,
            msg.event_id.ok_or(AppError::InvalidRequest)?,
            &msg.goods_pos,
        )
        .await?;
    if !added_items.is_empty() {
        ctx.notify(
            CmdId::RougeAddItemWarehousePushCmd,
            RougeAddItemWarehousePush {
                season: Some(season),
                items: added_items,
                reason: Some(0),
            },
        )
        .await?;
    }
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeBuyGoodsCmd,
        RougeBuyGoodsReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_end_shop_event(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeEndShopEventRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let map = ctx
        .player()?
        .rouge
        .end_shop_event(
            ctx.state.db,
            season,
            msg.layer.ok_or(AppError::InvalidRequest)?,
            msg.node_id.ok_or(AppError::InvalidRequest)?,
            msg.event_id.ok_or(AppError::InvalidRequest)?,
        )
        .await?;
    notify_full_run(ctx, season).await?;
    ctx.send_reply(
        CmdId::RougeEndShopEventCmd,
        RougeEndShopEventReply { map: Some(map) },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_enter_rouge_select_reward(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = EnterRougeSelectRewardRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let info = ctx
        .player()?
        .rouge
        .select_reward(ctx.state.db, season, &msg.reward_id)
        .await?;
    ctx.send_reply(
        CmdId::EnterRougeSelectRewardCmd,
        EnterRougeSelectRewardReply {
            season: Some(season),
            rouge_info: Some(info),
        },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_rouge_active_genius(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = RougeActiveGeniusRequest::decode(&req.data[..])?;
    let season = msg.season.ok_or(AppError::InvalidRequest)?;
    let genius_id = msg.genius_id.ok_or(AppError::InvalidRequest)?;
    let genius_ids = ctx
        .player()?
        .rouge
        .active_genius(ctx.state.db, season, genius_id)
        .await?;
    ctx.send_reply(
        CmdId::RougeActiveGeniusCmd,
        RougeActiveGeniusReply {
            season: Some(season),
            genius_id: Some(genius_id),
            genius_ids,
        },
        0,
        req.up_tag,
    )
    .await
}
