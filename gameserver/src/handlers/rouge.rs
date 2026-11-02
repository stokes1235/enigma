use crate::{
    error::AppError,
    logic::rouge,
    net::{context::ConnectionContext, packet::ClientPacket},
};
use prost::Message;
use sonettobuf::{
    CmdId, EnterRougeSelectDifficultyReply, EnterRougeSelectDifficultyRequest,
    EnterRougeSelectHeroesRequest, EnterRougeSelectStyleReply, EnterRougeSelectStyleRequest,
    GetRouge2InfoRequest, GetRouge2OutsideInfoRequest, GetRougeInfoRequest,
    GetRougeOutsideInfoRequest, Rouge2GetUnlockCollectionsRequest, RougeAbortReply,
    RougeAbortRequest,
};

pub async fn on_get_rouge_outside_info(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = GetRougeOutsideInfoRequest::decode(&req.data[..])?;
    let reply = rouge::rouge_outside_info(msg.season.unwrap_or_default());
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
    let _ = EnterRougeSelectHeroesRequest::decode(&req.data[..])?;
    let _ = ctx.player()?;
    // Do not acknowledge success until map generation and team persistence exist.
    Err(AppError::InvalidRequest)
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
