pub mod artifact_payload;
pub mod artifact_rules;
pub mod bag;
#[cfg(test)]
mod bonuses_integration;
mod collection_shapes;
pub mod combat;
mod entrust_effects;
pub mod extra_rewards;
#[cfg(test)]
mod extra_rewards_integration;
pub mod flow;
pub mod genius_bonuses;
pub mod map;
pub mod middle;
pub mod shop;

use crate::error::AppError;
use database::db::game::rouge;
use database::models::game::rouge::{RougeRun, RougeRunPosition, RougeStart};
use sonettobuf::{
    GetRouge2InfoReply, GetRouge2OutsideInfoReply, GetRougeInfoReply, GetRougeOutsideInfoReply,
    NodeInfo, Rouge2AlchemyInfo, Rouge2AlchemyMaterialInfo, Rouge2AttrInfo, Rouge2BagInfo,
    Rouge2BossBattleInfo, Rouge2CareerLevelInfo, Rouge2GetUnlockCollectionsReply, Rouge2Info,
    Rouge2OutsideInfo, Rouge2RewardInfo, Rouge2TotalRecordInfo, RougeBattleHero, RougeEntrustInfo,
    RougeGameRecordInfo, RougeHeroInfo, RougeHeroLife, RougeInfo, RougeLayerInfo, RougeMapInfo,
    RougeMiddleLayerInfo, RougeOutsideBonusNo, RougeOutsideInfo, RougePieceInfo, RougeTeamInfo,
};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug)]
pub struct RougeManager {
    player_id: i64,
}

impl RougeManager {
    pub fn new(player_id: i64) -> Self {
        Self { player_id }
    }

    /// Current setup or exploration state, including resumable leaf transitions.
    pub async fn original_info(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<GetRougeInfoReply, AppError> {
        let start = load_original(db, self.player_id, season).await?;
        let run = match rouge::get_run(db, self.player_id, season).await? {
            Some(mut run) => {
                let mut detail = flow::details(&run);
                if detail.outside_genius.is_none() || detail.safe_zone_recovery.is_none() {
                    let expected = run.clone();
                    if detail.outside_genius.is_none() {
                        detail.outside_genius =
                            Some(rouge::list_genius(db, self.player_id, season).await?);
                    }
                    detail.safe_zone_recovery = config::configs::get()
                        .rouge_style(season, start.style)
                        .map(|style| style.power);
                    run.details = serde_json::to_string(&detail)?;
                    flow::save(db, &expected, &run).await?;
                }
                Some(flow::resume(db, run).await?)
            }
            None => None,
        };
        let init_hero_ids = json_ids(&start.hero_ids);
        let mut info = rouge_info_from(&start, run.as_ref());
        if run.is_none() && start.style > 0 {
            let bonus = genius_bonuses::from_ids(
                config::configs::get(),
                &rouge::list_genius(db, self.player_id, season).await?,
            );
            info.team_size = info.team_size.map(|value| value + bonus.initial_capacity);
            info.coin = info.coin.map(|value| value + bonus.initial_coin);
        }
        Ok(GetRougeInfoReply {
            rouge_info: Some(info),
            init_hero_ids,
        })
    }

    /// Difficulty selection. Gated by the official `preDifficulty` unlock chain.
    pub async fn select_difficulty(
        self,
        db: &SqlitePool,
        season: i32,
        difficulty: i32,
        version: &[i32],
        has_limiter: bool,
    ) -> Result<RougeInfo, AppError> {
        // Expansions and limiters are out of scope for the base game.
        if season != 1 || !version.is_empty() || has_limiter {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        if tables.rouge_difficulty(difficulty).is_none() {
            return Err(AppError::InvalidRequest);
        }
        let cleared = rouge::cleared_difficulties(db, self.player_id, season).await?;
        if !tables.rouge_difficulty_unlocked(difficulty, &cleared) {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        if start.state != 0 {
            return Err(AppError::InvalidRequest);
        }
        if !rouge::advance_setup(db, self.player_id, season, 0, 1, difficulty, 0, &[], 0).await? {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        Ok(rouge_info_from(&start, None))
    }

    pub async fn abort_setup(self, db: &SqlitePool, season: i32) -> Result<RougeInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        if start.state == 4 && rouge::get_run(db, self.player_id, season).await?.is_some() {
            if !rouge::mark_run_failed(db, self.player_id, season).await? {
                return Err(AppError::InvalidRequest);
            }
            let start = load_original(db, self.player_id, season).await?;
            let run = rouge::get_run(db, self.player_id, season).await?;
            return Ok(rouge_info_from(&start, run.as_ref()));
        }
        if !rouge::reset_setup(db, self.player_id, season).await? {
            return Err(AppError::InvalidRequest);
        }
        rouge::delete_run(db, self.player_id, season).await?;
        let start = load_original(db, self.player_id, season).await?;
        Ok(rouge_info_from(&start, None))
    }

    /// Catalyst selection. Only catalysts with `unlockType == 0` are available in the base game.
    pub async fn select_style(
        self,
        db: &SqlitePool,
        season: i32,
        style: i32,
    ) -> Result<RougeInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let Some(catalyst) = tables.rouge_style(season, style) else {
            return Err(AppError::InvalidRequest);
        };
        if !tables.rouge_style_is_default_unlocked(catalyst) {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        if start.state != 1 {
            return Err(AppError::InvalidRequest);
        }
        if !rouge::advance_setup(
            db,
            self.player_id,
            season,
            1,
            3,
            start.difficulty,
            style,
            &[],
            0,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        let mut info = rouge_info_from(&start, None);
        let bonus = genius_bonuses::from_ids(
            tables,
            &rouge::list_genius(db, self.player_id, season).await?,
        );
        info.team_size = info.team_size.map(|value| value + bonus.initial_capacity);
        info.coin = info.coin.map(|value| value + bonus.initial_coin);
        Ok(info)
    }

    /// Initial companion selection, enforcing the noise budget and team size limits.
    pub async fn select_heroes(
        self,
        db: &SqlitePool,
        season: i32,
        heroes: &[i32],
        assist_hero_uid: i64,
    ) -> Result<RougeInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let start = load_original(db, self.player_id, season).await?;
        if start.state != 3 {
            return Err(AppError::InvalidRequest);
        }
        let Some(catalyst) = tables.rouge_style(season, start.style) else {
            return Err(AppError::InvalidRequest);
        };

        if heroes.is_empty() || heroes.len() > tables.rouge_team_size() {
            return Err(AppError::InvalidRequest);
        }
        let mut unique = HashSet::with_capacity(heroes.len());
        if !heroes.iter().all(|hero_id| unique.insert(*hero_id)) {
            return Err(AppError::InvalidRequest);
        }

        let owned = rouge::owned_hero_ids(db, self.player_id).await?;
        let mut noise = 0;
        for hero_id in heroes {
            if !owned.contains(hero_id) {
                return Err(AppError::InvalidRequest);
            }
            let Some(character) = tables.rouge_character(*hero_id) else {
                return Err(AppError::InvalidRequest);
            };
            let Some(cost) = tables.rouge_noise_cost(character.rare) else {
                return Err(AppError::InvalidRequest);
            };
            noise += cost;
        }
        let bonus = genius_bonuses::from_ids(
            tables,
            &rouge::list_genius(db, self.player_id, season).await?,
        );
        if noise > catalyst.capacity + bonus.initial_capacity {
            return Err(AppError::InvalidRequest);
        }

        if !rouge::advance_setup(
            db,
            self.player_id,
            season,
            3,
            4,
            start.difficulty,
            start.style,
            heroes,
            assist_hero_uid,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        let run = start_run(db, self.player_id, &start).await?;
        Ok(rouge_info_from(&start, Some(&run)))
    }

    /// Replaces the current four-slot combat party while retaining roster life.
    pub async fn group_change(
        self,
        db: &SqlitePool,
        season: i32,
        battle_heroes: &[RougeBattleHero],
    ) -> Result<RougeTeamInfo, AppError> {
        if season != 1 || battle_heroes.is_empty() || battle_heroes.len() > 4 {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if start.state != 4 || run.terminal_state != 0 {
            return Err(AppError::InvalidRequest);
        }

        let roster: HashSet<i32> = persisted_team(&run)
            .map(|team| {
                team.hero_life_list
                    .iter()
                    .filter_map(|life| life.hero_id)
                    .collect()
            })
            .unwrap_or_else(|| json_ids(&start.hero_ids).into_iter().collect());
        let owned_equipment: HashSet<i64> =
            database::db::game::equipment::get_user_equipment(db, self.player_id)
                .await?
                .into_iter()
                .filter(|equip| equip.count > 0)
                .map(|equip| equip.uid)
                .collect();
        let mut slots = HashSet::with_capacity(battle_heroes.len());
        let mut used_heroes = HashSet::with_capacity(battle_heroes.len() * 2);
        let mut normalized = Vec::with_capacity(battle_heroes.len());
        for hero in battle_heroes {
            let index = hero.index.unwrap_or_default();
            let hero_id = hero.hero_id.unwrap_or_default();
            let equip_uid = hero.equip_uid.unwrap_or_default();
            let support_hero_id = hero.support_hero_id.unwrap_or_default();
            let support_hero_skill = hero.support_hero_skill.unwrap_or_default();
            if !(1..=4).contains(&index)
                || !slots.insert(index)
                || hero_id <= 0
                || !roster.contains(&hero_id)
                || !used_heroes.insert(hero_id)
                || (equip_uid != 0 && !owned_equipment.contains(&equip_uid))
                || support_hero_skill < 0
            {
                return Err(AppError::InvalidRequest);
            }
            if support_hero_id != 0
                && (!roster.contains(&support_hero_id) || !used_heroes.insert(support_hero_id))
            {
                return Err(AppError::InvalidRequest);
            }
            if support_hero_id == 0 && support_hero_skill != 0 {
                return Err(AppError::InvalidRequest);
            }
            normalized.push(RougeBattleHero {
                index: Some(index),
                hero_id: Some(hero_id),
                equip_uid: Some(equip_uid),
                support_hero_id: Some(support_hero_id),
                support_hero_skill: Some(support_hero_skill),
            });
        }
        normalized.sort_by_key(|hero| hero.index.unwrap_or_default());
        let tables = config::configs::get();
        let half_support = tables
            .rouge_style(season, start.style)
            .is_some_and(|style| style.half_cost == 1);
        let mut noise = 0_i32;
        for hero in &normalized {
            for (id, support) in [
                (hero.hero_id.unwrap_or_default(), false),
                (hero.support_hero_id.unwrap_or_default(), true),
            ] {
                if id == 0 {
                    continue;
                }
                let rare = tables
                    .rouge_character(id)
                    .ok_or(AppError::InvalidRequest)?
                    .rare;
                let cost = if support && half_support {
                    tables.rouge_const.get(9).and_then(|row| {
                        row.value.split('|').find_map(|pair| {
                            let (key, value) = pair.split_once('#')?;
                            (key.parse::<i32>().ok() == Some(rare))
                                .then(|| value.parse::<i32>().ok())
                                .flatten()
                        })
                    })
                } else {
                    tables.rouge_noise_cost(rare)
                }
                .ok_or(AppError::InvalidRequest)?;
                noise = noise.saturating_add(cost);
            }
        }
        if noise > flow::team_capacity(&start, &run) {
            return Err(AppError::InvalidRequest);
        }

        let mut team =
            persisted_team(&run).unwrap_or_else(|| initial_team(&json_ids(&start.hero_ids)));
        team.battle_hero_list = normalized;
        let encoded = serde_json::to_string(&team)?;
        if !rouge::update_run_team(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        Ok(team)
    }

    /// Moves along the generated layer graph. The last node remains on the normal
    /// map until its boss battle is won; settlement then opens the middle layer.
    pub async fn round_move(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
    ) -> Result<RougeMapInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.layer_id != layer || run.map_type != MAP_TYPE_NORMAL {
            return Err(AppError::InvalidRequest);
        }
        let layer_map: map::LayerMap = serde_json::from_str(&run.graph)?;
        let current = layer_map
            .node(run.node_id)
            .ok_or(AppError::InvalidRequest)?;
        if !node_is_resolved(&run, current) {
            return Err(AppError::InvalidRequest);
        }
        if !current.next.contains(&node_id) {
            return Err(AppError::InvalidRequest);
        }
        let target = layer_map.node(node_id).ok_or(AppError::InvalidRequest)?;

        let mut next = run.clone();
        next.stage = target.stage;
        next.node_id = target.node_id;
        next.interactive_index = -1;
        next.interactive_refresh = 0;
        next.battle_interactive.clear();
        let tables = config::configs::get();
        let mut detail = flow::details(&run);
        detail.battle_reward = None;
        let start = load_original(db, self.player_id, season).await?;
        if target.node_type == map::NODE_SHOP {
            next.shop = serde_json::to_string(&shop::generate(
                run.layer_id,
                target.node_id,
                run.seed as u64,
                tables.rouge_goods.iter().cloned(),
            ))?;
            let tree = flow::talent_tree(&start, &run);
            for index in detail.active_talents.clone() {
                if tree.rouge_talent.get(index).and_then(|talent| talent.id) == Some(209)
                    && !detail.triggered_talents.contains(&index)
                {
                    next.coin = next.coin.saturating_add(80);
                    detail.triggered_talents.push(index);
                }
            }
        }
        next.details = serde_json::to_string(&detail)?;
        flow::apply_map_effects(&mut next, artifact_rules::MapTrigger::Step)?;
        next.middle_state = updated_entrust(
            &run,
            EntrustChange {
                entered_type: tables
                    .rouge_event(target.event_id)
                    .map(|event| event.r#type),
                gained_coin: (next.coin - run.coin).max(0),
                gained_power: (next.power - run.power).max(0),
                ..Default::default()
            },
        )?;
        flow::save(db, &run, &next).await?;
        Ok(map_info(&next))
    }

    /// Moves the piece on the middle-layer board.
    pub async fn piece_move(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        middle_layer: i32,
        index: i32,
    ) -> Result<RougeMapInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.map_type != MAP_TYPE_MIDDLE
            || run.layer_id != layer
            || run.middle_layer_id != middle_layer
            || pending_interactive(&run, config::configs::get()).is_some()
        {
            return Err(AppError::InvalidRequest);
        }
        let state = middle_state(&run);
        if index != -1 && !state.pieces.iter().any(|piece| piece.index == Some(index)) {
            return Err(AppError::InvalidRequest);
        }
        let position = RougeRunPosition {
            position_index: index,
            ..run_position(&run)
        };
        if !rouge::update_run_state(db, self.player_id, season, &position).await? {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        Ok(map_info(&run))
    }

    /// Selects a middle-layer piece dialogue and persists its quest state.
    pub async fn piece_talk_select(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        middle: i32,
        index: i32,
        selection: i32,
    ) -> Result<RougeMapInfo, AppError> {
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if season != 1
            || run.map_type != MAP_TYPE_MIDDLE
            || run.layer_id != layer
            || run.middle_layer_id != middle
            || run.position_index != index
            || run.terminal_state != 0
            || pending_interactive(&run, config::configs::get()).is_some()
        {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let mut state = middle_state(&run);
        let piece = state
            .pieces
            .iter_mut()
            .find(|piece| piece.index == Some(index))
            .ok_or(AppError::InvalidRequest)?;
        let talk = tables
            .rouge_piece_talk
            .get(piece.talk_id.unwrap_or_default())
            .ok_or(AppError::InvalidRequest)?;
        if !talk
            .select_ids
            .split('|')
            .any(|id| id.parse::<i32>().ok() == Some(selection))
        {
            return Err(AppError::InvalidRequest);
        }
        let select = tables
            .rouge_piece_select
            .get(selection)
            .ok_or(AppError::InvalidRequest)?;
        if select.unlock_type != 0 || select.active_type != 0 || piece.finish == Some(true) {
            return Err(AppError::InvalidRequest);
        }
        let mut details = flow::details(&run);
        let mut next_run = run.clone();
        let entrust_id = match select.trigger_type {
            1 if state.entrust_id == 0 => {
                let id = select
                    .trigger_param
                    .parse::<i32>()
                    .map_err(|_| AppError::InvalidRequest)?;
                if tables.rouge_entrust.get(id).is_none() {
                    return Err(AppError::InvalidRequest);
                }
                state.entrust_id = id;
                state.entrust_progress = 0;
                piece.select_id = Some(selection);
                piece.finish = Some(true);
                id
            }
            3 => 0,
            0 => {
                piece.select_id = Some(selection);
                piece.finish = Some(true);
                0
            }
            2 => {
                // Effect list before '-' is unconditional; later parts encode
                // conditional alternatives, which are not assumed here.
                for id in select
                    .trigger_param
                    .split('-')
                    .next()
                    .unwrap_or_default()
                    .split('#')
                {
                    if let Ok(id) = id.parse::<i32>() {
                        flow::apply_effect(&mut next_run, id)?;
                    }
                }
                piece.select_id = Some(selection);
                piece.finish = Some(true);
                0
            }
            6 => {
                let params: Vec<i32> = select
                    .trigger_param
                    .split('#')
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if params.len() < 2 || tables.rouge_ending.get(params[1]).is_none() {
                    return Err(AppError::InvalidRequest);
                }
                details.end_episode = params[0];
                details.end_id = params[1];
                piece.select_id = Some(selection);
                0
            }
            _ => return Err(AppError::InvalidRequest),
        };
        piece.trigger_str = Some(serde_json::json!({"displaceMaxNum":0,"displaceNum":0,"dropId":0,"endEpisodeId":details.end_episode,"entrustId":entrust_id,"fightFail":false,"repairRandomNum":0}).to_string());
        let encoded = serde_json::to_string(&state)?;
        next_run.middle_state = encoded;
        if select.trigger_type == 6 {
            next_run.details = serde_json::to_string(&details)?;
        }
        flow::queue_level_recruit(&mut next_run, run.team_exp);
        flow::save(db, &run, &next_run).await?;
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        Ok(map_info(&run))
    }

    /// Leaves the middle layer for a chosen next layer, generating its map.
    pub async fn leave_middle_layer(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        middle_layer: i32,
        next_layer: i32,
    ) -> Result<RougeMapInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.map_type != MAP_TYPE_MIDDLE
            || run.layer_id != layer
            || run.middle_layer_id != middle_layer
            || pending_interactive(&run, config::configs::get()).is_some()
        {
            return Err(AppError::InvalidRequest);
        }
        let board = middle_board(config::configs::get(), run.layer_id)?;
        if run.position_index != -1 || !board.next_layers.contains(&next_layer) {
            return Err(AppError::InvalidRequest);
        }

        let tables = config::configs::get();
        if tables.rouge_layer.iter().all(|row| row.id != next_layer) {
            return Err(AppError::InvalidRequest);
        }

        let layer_map = generate_layer(next_layer, run.seed as u64 + run.layer_index as u64 + 1);
        let start_node = layer_map
            .start_node()
            .ok_or(AppError::InvalidRequest)?
            .clone();
        let graph = serde_json::to_string(&layer_map)?;
        let position = RougeRunPosition {
            map_type: MAP_TYPE_NORMAL,
            layer_index: run.layer_index + 1,
            layer_id: next_layer,
            stage: start_node.stage,
            node_id: start_node.node_id,
            middle_layer_id: 0,
            position_index: 0,
        };
        let stats = current_layer_stats(&run)?;
        if !rouge::advance_run_layer(
            db,
            self.player_id,
            season,
            &run,
            &position,
            &graph,
            stats.minor,
            stats.danger,
            stats.crisis,
            stats.steps,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        Ok(map_info(&run))
    }

    /// Rolls the stock of the shop node the player is standing on.
    pub async fn shop_refresh(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
        event_id: i32,
    ) -> Result<RougeMapInfo, AppError> {
        let run = self.shop_node(db, season, layer, node_id, event_id).await?;
        let tables = config::configs::get();
        let refresh_num = serde_json::from_str::<shop::ShopState>(&run.shop)
            .ok()
            .filter(|shop| shop.node_id == node_id)
            .map(|shop| shop.refresh_num.saturating_add(1))
            .unwrap_or(1);
        let stock = shop::generate_for_refresh(
            run.layer_id,
            node_id,
            run.seed as u64,
            refresh_num,
            tables.rouge_goods.iter().cloned(),
        );
        let cost = shop::refresh_price(tables, refresh_num.saturating_sub(1));
        if run.coin < cost {
            return Err(AppError::InvalidRequest);
        }
        let mut next = run.clone();
        next.shop = serde_json::to_string(&stock)?;
        next.coin -= cost;
        next.spent_coin = next.spent_coin.saturating_add(cost);
        next.middle_state = updated_entrust(
            &run,
            EntrustChange {
                spent_coin: cost,
                ..Default::default()
            },
        )?;
        flow::save(db, &run, &next).await?;
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        Ok(map_info(&run))
    }

    /// Buys the offers at the given stock positions.
    pub async fn buy_goods(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
        event_id: i32,
        positions: &[i32],
    ) -> Result<(RougeMapInfo, Vec<sonettobuf::RougeItem>), AppError> {
        let run = self.shop_node(db, season, layer, node_id, event_id).await?;
        if positions.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        let mut stock: shop::ShopState =
            serde_json::from_str(&run.shop).map_err(|_| AppError::InvalidRequest)?;
        if stock.node_id != node_id {
            return Err(AppError::InvalidRequest);
        }

        let tables = config::configs::get();
        let mut total = 0;
        let mut picks = Vec::new();
        let mut positions_seen = HashSet::new();
        for position in positions {
            if stock.bought.contains(position) || !positions_seen.insert(*position) {
                return Err(AppError::InvalidRequest);
            }
            // Store positions are one-based in the client JSON map.
            let index = usize::try_from(position.saturating_sub(1))
                .map_err(|_| AppError::InvalidRequest)?;
            if *position <= 0 {
                return Err(AppError::InvalidRequest);
            }
            let goods_id = *stock.stock.get(index).ok_or(AppError::InvalidRequest)?;
            let goods = tables
                .rouge_goods
                .get(goods_id)
                .ok_or(AppError::InvalidRequest)?;
            total += flow::shop_price(&run, goods.price);
            picks.push((index, goods.creator));
        }
        if run.coin < total {
            return Err(AppError::InvalidRequest);
        }

        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let mut added_items = Vec::new();
        for (index, creator) in picks {
            if creator > 0 {
                let item = run_bag.add(creator)?;
                added_items.push(bag::RunBag::to_client_item(&item));
            }
            stock.bought.push(index as i32 + 1);
        }

        let bag_json = serde_json::to_string(&run_bag)?;
        let shop_json = serde_json::to_string(&stock)?;
        let middle = updated_entrust(
            &run,
            EntrustChange {
                spent_coin: total,
                collections: added_items.len() as i32,
                ..Default::default()
            },
        )?;
        if !rouge::apply_shop_purchase(
            db,
            self.player_id,
            season,
            &bag_json,
            &shop_json,
            run.coin - total,
            run.power,
            total,
            &run.bag,
            &run.shop,
            run.coin,
            &run.middle_state,
            &middle,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }

        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        Ok((map_info(&run), added_items))
    }

    /// Leaves a shop and resolves its map event so movement may continue.
    pub async fn end_shop_event(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
        event_id: i32,
    ) -> Result<RougeMapInfo, AppError> {
        let run = self.shop_node(db, season, layer, node_id, event_id).await?;
        let mut finished: Vec<i32> = serde_json::from_str(&run.finished).unwrap_or_default();
        if !finished.contains(&node_id) {
            finished.push(node_id);
            let middle = updated_entrust(
                &run,
                EntrustChange {
                    finished_event: Some(event_id),
                    ..Default::default()
                },
            )?;
            if !rouge::finish_node_with_entrust(
                db,
                self.player_id,
                season,
                &run.finished,
                &serde_json::to_string(&finished)?,
                &run.middle_state,
                &middle,
            )
            .await?
            {
                return Err(AppError::InvalidRequest);
            }
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let run = flow::resume(db, run).await?;
        Ok(map_info(&run))
    }

    /// Validates that the player stands on a shop node and returns the run.
    async fn shop_node(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
        event_id: i32,
    ) -> Result<RougeRun, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.map_type != MAP_TYPE_NORMAL || run.layer_id != layer || run.node_id != node_id {
            return Err(AppError::InvalidRequest);
        }
        let layer_map: map::LayerMap = serde_json::from_str(&run.graph)?;
        let node = layer_map.node(node_id).ok_or(AppError::InvalidRequest)?;
        if node.node_type != map::NODE_SHOP || node.event_id != event_id {
            return Err(AppError::InvalidRequest);
        }
        Ok(run)
    }

    /// Meta progression payload.
    pub async fn original_outside_info(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<GetRougeOutsideInfoReply, AppError> {
        rouge_outside_info(db, self.player_id, season).await
    }

    /// Activates one 刻画生长 node, spending 刻画的枝芽.
    pub async fn active_genius(
        self,
        db: &SqlitePool,
        season: i32,
        genius_id: i32,
    ) -> Result<Vec<i32>, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let node = tables
            .rouge_genius_branch
            .iter()
            .find(|row| row.season == season && row.id == genius_id)
            .ok_or(AppError::InvalidRequest)?;

        let activated = rouge::list_genius(db, self.player_id, season).await?;
        if activated.contains(&genius_id) {
            return Err(AppError::InvalidRequest);
        }
        // Every predecessor must already be active.
        for before in node.before.split('|').filter(|id| !id.trim().is_empty()) {
            let id: i32 = before
                .trim()
                .parse()
                .map_err(|_| AppError::InvalidRequest)?;
            if !activated.contains(&id) {
                return Err(AppError::InvalidRequest);
            }
        }

        rouge::get_or_create_outside(db, self.player_id, season).await?;
        if !rouge::activate_genius(db, self.player_id, season, genius_id, node.cost).await? {
            return Err(AppError::InvalidRequest);
        }
        Ok(rouge::list_genius(db, self.player_id, season).await?)
    }

    /// Claims the post-settlement rewards and returns to the setup screen.
    pub async fn select_reward(
        self,
        db: &SqlitePool,
        season: i32,
        reward_ids: &[i32],
    ) -> Result<RougeInfo, AppError> {
        if season != 1 || reward_ids.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        let start = load_original(db, self.player_id, season).await?;
        if start.state != 2 {
            return Err(AppError::InvalidRequest);
        }
        let offered: Vec<i32> = serde_json::from_str(&start.rewards).unwrap_or_default();
        if reward_ids.len() != 1 || !offered.contains(&reward_ids[0]) {
            return Err(AppError::InvalidRequest);
        }

        let tables = config::configs::get();
        for id in reward_ids {
            tables
                .rouge_last_reward
                .iter()
                .find(|row| row.id == *id && row.season == season)
                .ok_or(AppError::InvalidRequest)?;
        }
        let selected_rewards = serde_json::to_string(reward_ids)?;
        let mut tx = db.begin().await?;
        if !rouge::claim_rewards_in_transaction(
            &mut tx,
            self.player_id,
            season,
            &start.rewards,
            &selected_rewards,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        tx.commit().await?;

        let start = load_original(db, self.player_id, season).await?;
        Ok(rouge_info_from(&start, None))
    }

    /// Settles the current run: scores it, unlocks the next difficulty and clears
    /// the run so the player returns to the setup screen.
    pub async fn end(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<(RougeInfo, sonettobuf::RougeResultInfo), AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let start = load_original(db, self.player_id, season).await?;
        let Some(run) = rouge::get_run(db, self.player_id, season).await? else {
            if start.state == 2 {
                let cached: Option<(String, String)> = sqlx::query_as("SELECT rouge_info, result_info FROM user_rouge_settlement WHERE user_id = ? AND season = ?")
                    .bind(self.player_id).bind(season).fetch_optional(db).await?;
                if let Some((info, result)) = cached {
                    return Ok((serde_json::from_str(&info)?, serde_json::from_str(&result)?));
                }
            }
            return Err(AppError::InvalidRequest);
        };
        if run.terminal_state == 0 {
            return Err(AppError::InvalidRequest);
        }

        let run_bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap_or(bag::RunBag {
            rows: 0,
            cols: 0,
            items: Vec::new(),
            uid_counter: 0,
        });

        // Score per node type, straight from `rouge_const` 1011..=1019. Counters
        // archived at each layer transition are combined with the current layer.
        let current = current_layer_stats(&run)?;
        let minor = run.minor_count + current.minor;
        let danger = run.danger_count + current.danger;
        let crisis = run.crisis_count + current.crisis;
        let steps = run.step_count + current.steps;
        let collection_counts = flow::collection_counts(&run_bag);
        let collection_num = collection_counts.values().sum::<i32>();
        let details = flow::details(&run);
        let layers = flow::completed_layers(&run);
        let cleared = run.terminal_state == 1;

        let minor_score = minor * const_i32(tables, 1011, 100);
        let danger_score = danger * const_i32(tables, 1012, 200);
        let crisis_score = crisis * const_i32(tables, 1013, 300);
        let collection_score = collection_num * const_i32(tables, 1014, 25);
        let layer_score = layer_score_for(tables, layers);
        let end_score = if cleared {
            const_i32(tables, 1017, 400)
        } else {
            0
        };
        let completed_entrusts = serde_json::from_str::<MiddleState>(&run.middle_state)
            .map(|state| state.completed_entrusts)
            .unwrap_or_default();
        let entrust_score = completed_entrusts.len() as i32 * const_i32(tables, 1016, 160);
        let badges = earned_badges(
            tables,
            minor + danger + crisis,
            collection_num,
            crisis,
            run.spent_coin,
            details.compositions.values().sum(),
        );
        let badge_score: i32 = badges.iter().map(|badge| badge.score).sum();

        let base = minor_score
            + danger_score
            + crisis_score
            + collection_score
            + layer_score
            + end_score
            + entrust_score
            + badge_score;
        // Difficulty credit efficiency (`rouge_difficulty.scoreReward`, 100..=175%).
        let efficiency = tables
            .rouge_difficulty(start.difficulty)
            .map(|row| row.score_reward)
            .unwrap_or(100)
            .max(1)
            + flow::bonuses(&run).credit_percent;
        // Same-kind credit percentages add to the displayed efficiency. The
        // stacking policy beyond the captured difficulty-I profile is inferred.
        let final_score = (base as i64 * efficiency as i64 / 100).min(i32::MAX as i64) as i32;

        // 金巷信物 = 信用 / 2500, 刻画的枝芽 = 信用 / 700, remainders carry over.
        let point_div = const_i32(tables, 1018, 2500).max(1);
        let genius_div = const_i32(tables, 1019, 700).max(1);
        let point_total = final_score + start.point_remainder;
        let genius_total = final_score + start.genius_remainder;
        let add_point = point_total / point_div;
        let add_genius_point = genius_total / genius_div;
        let point_remainder = point_total % point_div;
        let genius_remainder = genius_total % genius_div;
        let extra_balance = extra_rewards::balance(db, self.player_id, season).await?;
        let extra_add_point = extra_rewards::quote(layers, extra_balance);

        // Official flow: settlement lands on the reward screen (`LastReward`), not
        // straight back on the setup screen.
        let offered = offered_rewards(tables, start.difficulty);
        let rewards = serde_json::to_string(&offered)?;

        let hero_ids = json_ids(&start.hero_ids);
        let (player_name, player_level, _) =
            database::db::game::player_infos::get_user_basic_info(db, self.player_id).await?;
        let portrait: Option<i32> =
            sqlx::query_scalar("SELECT portrait FROM player_info WHERE player_id = ?")
                .bind(self.player_id)
                .fetch_optional(db)
                .await?;
        let result = sonettobuf::RougeResultInfo {
            season: Some(season),
            init_hero_id: hero_ids,
            collection2_num_str: Some(flow::counts_string(&collection_counts)),
            collection2_score: Some(format!("{collection_num}#{collection_score}")),
            compose_res2_num_str: Some(flow::counts_string(&details.compositions)),
            normal_fight2_score: Some(format!("{minor}#{minor_score}")),
            difficult_fight2_score: Some(format!("{danger}#{danger_score}")),
            dangerous_fight2_score: Some(format!("{crisis}#{crisis_score}")),
            layer2_score: Some(format!("{layers}#{layer_score}")),
            entrust2_score: Some(format!("{}#{entrust_score}", completed_entrusts.len())),
            finish_entrust_id: completed_entrusts,
            badge2_score: Some(
                badges
                    .iter()
                    .map(|badge| format!("{}#{}", badge.id, badge.score))
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            end2_score: Some(format!(
                "{}#{end_score}",
                if cleared { details.end_id } else { 0 }
            )),
            // This field drives settlement trigger type 10 ("coins spent"), so
            // report the cumulative spend rather than the remaining balance.
            consume_coin: Some(run.spent_coin),
            consume_power: Some(details.consume_power),
            step_num: Some(steps),
            final_score: Some(final_score),
            add_point: Some(add_point),
            extra_add_point: Some(extra_add_point),
            remain_score2_point: Some(point_remainder),
            add_genius_point: Some(add_genius_point),
            remain_score2_genius_point: Some(genius_remainder),
            score_reward: Some(efficiency),
            before_score: Some(base),
            pre_remain_score2_point: Some(start.point_remainder),
            pre_remain_score2_genius_point: Some(start.genius_remainder),
            review_info: Some(sonettobuf::RougeReviewInfo {
                season: Some(season),
                player_name: Some(player_name),
                player_level: Some(player_level),
                portrait: Some(portrait.unwrap_or(171603)),
                difficulty: Some(start.difficulty),
                style: Some(start.style),
                finish_time: Some(common::time::ServerTime::now_ms() / 1000),
                team_level: Some(team_progress(run.season, run.team_exp).0),
                collection_num: Some(collection_num),
                gain_coin: Some(run.coin + run.spent_coin),
                layouts: artifact_payload::bag_payload(&run, &run_bag).layouts,
                team_info: persisted_team(&run),
                end_id: Some(if cleared { details.end_id } else { 0 }),
                layer_id: Some(run.layer_id),
                middle_layer_id: Some(run.middle_layer_id),
                ..Default::default()
            }),
            ..Default::default()
        };

        // EndReply retains the finished run so the client's ending/result flow
        // can read its ending, team and bag. The next GetInfo opens LastReward.
        let mut snapshot = rouge_info_from(&start, Some(&run));
        snapshot.state = Some(5);
        if !rouge::settle_run(
            db,
            self.player_id,
            season,
            start.difficulty,
            cleared,
            final_score,
            point_remainder,
            genius_remainder,
            add_genius_point,
            &rewards,
            &result.finish_entrust_id,
            add_point,
            &serde_json::to_string(&snapshot)?,
            &serde_json::to_string(&result)?,
            extra_balance,
            extra_add_point,
            run.seed,
            run.terminal_state,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        Ok((snapshot, result))
    }

    /// Places or moves one warehouse artefact on the collection board.
    pub async fn add_to_bar(
        self,
        db: &SqlitePool,
        season: i32,
        target_id: i64,
        row: i32,
        col: i32,
        rotation: i32,
    ) -> Result<(sonettobuf::RougeItemLayout, bool), AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let (item, from_warehouse) = run_bag.place(target_id, row, col, rotation)?;
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        let mut layout = bag::RunBag::to_client_layout(&item);
        artifact_payload::decorate_layouts(&run, &run_bag, std::slice::from_mut(&mut layout));
        Ok((layout, from_warehouse))
    }

    pub async fn remove_from_bar(
        self,
        db: &SqlitePool,
        season: i32,
        target_id: i64,
    ) -> Result<sonettobuf::RougeItem, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let item = run_bag.remove(target_id)?;
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        Ok(artifact_payload::item_payload(&run, &item))
    }

    pub async fn one_key_add_to_bar(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<Vec<sonettobuf::RougeItemLayout>, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let items = run_bag.place_all();
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        let mut layouts: Vec<_> = items.iter().map(bag::RunBag::to_client_layout).collect();
        artifact_payload::decorate_layouts(&run, &run_bag, &mut layouts);
        Ok(layouts)
    }

    pub async fn one_key_remove_from_bar(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<Vec<sonettobuf::RougeItem>, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let items = run_bag.remove_all()?;
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        Ok(items
            .iter()
            .map(|item| artifact_payload::item_payload(&run, item))
            .collect())
    }

    /// Inlays an enchant into one hole of a placed artefact.
    pub async fn inlay(
        self,
        db: &SqlitePool,
        season: i32,
        target_id: i64,
        hole: i32,
        consume_id: i64,
    ) -> Result<
        (
            sonettobuf::RougeItem,
            Option<sonettobuf::RougeItem>,
            bool,
            bool,
            Option<sonettobuf::RougeItem>,
        ),
        AppError,
    > {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;

        if run_bag.item(target_id).is_none() {
            return Err(AppError::InvalidRequest);
        }
        let hole_num = collection_shapes::hole_num(
            run_bag
                .item(target_id)
                .ok_or(AppError::InvalidRequest)?
                .item_id,
        );
        if hole_num == 0 {
            return Err(AppError::InvalidRequest);
        }
        let change = run_bag.inlay(target_id, hole, consume_id, hole_num)?;
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        Ok((
            artifact_payload::item_payload(&run, &change.target),
            change
                .previous_holder
                .as_ref()
                .map(|item| artifact_payload::item_payload(&run, item)),
            change.removed_from_warehouse,
            change.removed_from_bag,
            change
                .returned_to_warehouse
                .as_ref()
                .map(|item| artifact_payload::item_payload(&run, item)),
        ))
    }

    /// Removes an enchant from one hole of a placed artefact.
    pub async fn demount(
        self,
        db: &SqlitePool,
        season: i32,
        target_id: i64,
        hole: i32,
    ) -> Result<(sonettobuf::RougeItem, sonettobuf::RougeItem), AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;

        if run_bag.item(target_id).is_none() {
            return Err(AppError::InvalidRequest);
        }
        let hole_num = collection_shapes::hole_num(
            run_bag
                .item(target_id)
                .ok_or(AppError::InvalidRequest)?
                .item_id,
        );
        if hole_num == 0 {
            return Err(AppError::InvalidRequest);
        }

        let change = run_bag.demount(target_id, hole, hole_num)?;
        let encoded = serde_json::to_string(&run_bag)?;
        if !rouge::update_run_bag(db, self.player_id, season, &encoded).await? {
            return Err(AppError::InvalidRequest);
        }
        let mut run = run;
        run.bag = encoded;
        Ok((
            artifact_payload::item_payload(&run, &change.target),
            artifact_payload::item_payload(&run, &change.enchant),
        ))
    }

    pub async fn choice_event(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node_id: i32,
        event_id: i32,
        choice_id: i32,
    ) -> Result<RougeMapInfo, AppError> {
        self.resolve_choice(db, season, layer, node_id, event_id, choice_id)
            .await
    }

    /// Resolves a pending loss of non-unique collections.
    pub async fn select_lost_collection(
        self,
        db: &SqlitePool,
        season: i32,
        ids: &[i64],
    ) -> Result<(RougeMapInfo, Vec<i64>, Vec<i64>, Vec<sonettobuf::RougeItem>), AppError> {
        if season != 1
            || ids.is_empty()
            || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.map_type != MAP_TYPE_NORMAL || run.terminal_state != 0 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let pending = pending_interactive(&run, tables).ok_or(AppError::InvalidRequest)?;
        if pending.kind != INTERACT_LOSS_NON_UNIQUE || ids.len() != pending.config_id as usize {
            return Err(AppError::InvalidRequest);
        }
        let mut bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let mut placed = Vec::new();
        let mut warehouse = Vec::new();
        let mut updated = Vec::new();
        for uid in ids {
            if let Some(index) = bag.items.iter().position(|item| item.uid == *uid) {
                let item = &bag.items[index];
                if collection_shapes::is_unique(item.item_id)
                    || item.hole_ids.iter().any(|id| ids.contains(id))
                {
                    return Err(AppError::InvalidRequest);
                }
                if bag::RunBag::is_placed(item) {
                    placed.push(*uid);
                } else {
                    warehouse.push(*uid);
                }
                bag.items.remove(index);
            } else {
                let holder = bag
                    .items
                    .iter_mut()
                    .find(|item| item.hole_ids.contains(uid))
                    .ok_or(AppError::InvalidRequest)?;
                let slot = holder
                    .hole_ids
                    .iter()
                    .position(|id| id == uid)
                    .ok_or(AppError::InvalidRequest)?;
                let item_id = *holder.holes.get(slot).ok_or(AppError::InvalidRequest)?;
                if item_id == 0 || collection_shapes::is_unique(item_id) {
                    return Err(AppError::InvalidRequest);
                }
                holder.hole_ids[slot] = 0;
                holder.holes[slot] = 0;
                updated.push(bag::RunBag::to_client_item(holder));
            }
        }
        let encoded = serde_json::to_string(&bag)?;
        let next =
            next_supported_interactive(&pending.pipeline, pending.index, tables).unwrap_or(-1);
        let event_id = (next < 0).then(|| current_node_event(&run)).flatten();
        let middle = updated_entrust(
            &run,
            EntrustChange {
                finished_event: event_id,
                ..Default::default()
            },
        )?;
        if !rouge::consume_drop_with_entrust(
            db,
            self.player_id,
            season,
            &run.bag,
            &encoded,
            pending.index,
            run.interactive_refresh,
            next,
            &run.middle_state,
            &middle,
            run.node_id,
            &run.battle_interactive,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }
        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let run = flow::resume(db, run).await?;
        Ok((map_info(&run), placed, warehouse, updated))
    }

    /// Completes the post-battle recruitment and enters the next map state.
    pub async fn recruit_heroes(
        self,
        db: &SqlitePool,
        season: i32,
        heroes: &[i32],
    ) -> Result<(RougeTeamInfo, RougeMapInfo), AppError> {
        let expected = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run = expected.clone();
        let pending =
            pending_interactive(&run, config::configs::get()).ok_or(AppError::InvalidRequest)?;
        if season != 1
            || run.terminal_state != 0
            || pending.kind != INTERACT_RECRUIT
            || heroes.iter().copied().collect::<HashSet<_>>().len() != heroes.len()
        {
            return Err(AppError::InvalidRequest);
        }
        let owned = rouge::owned_hero_ids(db, self.player_id).await?;
        let mut team = persisted_team(&run).ok_or(AppError::InvalidRequest)?;
        let tables = config::configs::get();
        for hero_id in heroes {
            if !owned.contains(hero_id)
                || team
                    .hero_life_list
                    .iter()
                    .any(|life| life.hero_id == Some(*hero_id))
            {
                return Err(AppError::InvalidRequest);
            }
            team.hero_life_list.push(RougeHeroLife {
                hero_id: Some(*hero_id),
                life: Some(1000),
            });
            team.hero_info_list.push(RougeHeroInfo {
                hero_id: Some(*hero_id),
                ..Default::default()
            });
        }
        let noise: i32 = heroes
            .iter()
            .map(|hero| {
                tables
                    .rouge_character(*hero)
                    .and_then(|hero| tables.rouge_noise_cost(hero.rare))
                    .unwrap_or(i32::MAX / 100)
            })
            .sum();
        // Recruitment has a fresh four-noise budget; the entire companion
        // roster is not constrained by the currently deployed team's capacity.
        if noise > const_i32(tables, 8, 4) {
            return Err(AppError::InvalidRequest);
        }
        run.team = serde_json::to_string(&team)?;
        run.battle_interactive = pending.pipeline;
        flow::advance(&mut run, pending.index as usize + 1)?;
        flow::finish_node(&mut run)?;
        flow::save(db, &expected, &run).await?;
        Ok((
            persisted_team(&run).ok_or(AppError::InvalidRequest)?,
            map_info(&run),
        ))
    }

    /// Applies one pending healing interaction and advances its choice pipeline.
    pub async fn select_heal(
        self,
        db: &SqlitePool,
        season: i32,
        hero_ids: &[i32],
    ) -> Result<(RougeTeamInfo, RougeMapInfo), AppError> {
        self.select_team_interactive(db, season, hero_ids, INTERACT_HEAL)
            .await
    }

    /// Applies one pending revival interaction and advances its choice pipeline.
    pub async fn select_revive(
        self,
        db: &SqlitePool,
        season: i32,
        hero_ids: &[i32],
    ) -> Result<(RougeTeamInfo, RougeMapInfo), AppError> {
        self.select_team_interactive(db, season, hero_ids, INTERACT_REVIVE)
            .await
    }

    /// Claims positions from one pending collection-drop offer.
    pub async fn select_drop(
        self,
        db: &SqlitePool,
        season: i32,
        collection_pos: &[i32],
    ) -> Result<
        (
            RougeMapInfo,
            Vec<i32>,
            Vec<sonettobuf::RougeItem>,
            Vec<sonettobuf::RougeItemLayout>,
        ),
        AppError,
    > {
        if season != 1 || collection_pos.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        let positions: HashSet<i32> = collection_pos.iter().copied().collect();
        if positions.len() != collection_pos.len() || positions.contains(&-1) {
            return Err(AppError::InvalidRequest);
        }

        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if !matches!(run.map_type, MAP_TYPE_NORMAL | MAP_TYPE_MIDDLE) || run.terminal_state != 0 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let pending = pending_interactive(&run, tables).ok_or(AppError::InvalidRequest)?;
        if run.map_type == MAP_TYPE_MIDDLE
            && (pending.kind != INTERACT_DROP || run.battle_interactive.is_empty())
        {
            return Err(AppError::InvalidRequest);
        }
        if !matches!(pending.kind, INTERACT_DROP | INTERACT_ADVANCE_DROP) {
            return Err(AppError::InvalidRequest);
        }
        let drop = drop_state(&run, pending.config_id, tables).ok_or(AppError::InvalidRequest)?;
        if collection_pos.len() != drop.select_count as usize
            || collection_pos
                .iter()
                .any(|position| !(*position >= 0 && (*position as usize) < drop.offers.len()))
        {
            return Err(AppError::InvalidRequest);
        }

        let collection_ids: Vec<i32> = collection_pos
            .iter()
            .map(|position| drop.offers[*position as usize])
            .collect();
        let mut run_bag: bag::RunBag =
            serde_json::from_str(&run.bag).map_err(|_| AppError::InvalidRequest)?;
        let mut items = Vec::with_capacity(collection_ids.len());
        let mut layouts = Vec::new();
        let enter_bag = tables
            .rouge_drop
            .get(pending.config_id)
            .is_some_and(|row| row.enter_bag == 1);
        for item_id in &collection_ids {
            let item = run_bag.add(*item_id)?;
            if enter_bag {
                if let Ok((placed, _)) = run_bag.place(item.uid, -1, -1, 0) {
                    layouts.push(bag::RunBag::to_client_layout(&placed));
                    continue;
                }
            }
            items.push(bag::RunBag::to_client_item(&item));
        }
        let mut next = run.clone();
        next.bag = serde_json::to_string(&run_bag)?;
        next.battle_interactive = pending.pipeline.clone();
        next.interactive_refresh = 0;
        if pending.kind == INTERACT_ADVANCE_DROP {
            let row = tables
                .rouge_drop
                .get(pending.config_id)
                .ok_or(AppError::InvalidRequest)?;
            let reward = flow::reward(&run, row.id);
            next.coin = next.coin.saturating_add(reward.coin);
            next.team_exp = next.team_exp.saturating_add(reward.exp);
        }
        flow::advance(&mut next, pending.index as usize + 1)?;
        flow::queue_level_recruit(&mut next, run.team_exp);
        let finished_event = (next.interactive_index < 0
            || pending_interactive(&next, tables)
                .is_some_and(|item| item.kind == INTERACT_RECRUIT))
        .then(|| {
            serde_json::from_str::<map::LayerMap>(&run.graph)
                .ok()
                .and_then(|graph| graph.node(run.node_id).map(|node| node.event_id))
        })
        .flatten();
        next.middle_state = updated_entrust(
            &run,
            EntrustChange {
                gained_coin: (next.coin - run.coin).max(0),
                collections: collection_ids.len() as i32,
                finished_event,
                ..Default::default()
            },
        )?;
        flow::finish_node(&mut next)?;
        flow::save(db, &run, &next).await?;
        Ok((map_info(&next), collection_ids, items, layouts))
    }

    async fn select_team_interactive(
        self,
        db: &SqlitePool,
        season: i32,
        hero_ids: &[i32],
        expected_kind: i32,
    ) -> Result<(RougeTeamInfo, RougeMapInfo), AppError> {
        if season != 1 || hero_ids.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        let unique: HashSet<i32> = hero_ids.iter().copied().collect();
        if unique.len() != hero_ids.len() || unique.contains(&0) {
            return Err(AppError::InvalidRequest);
        }

        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        if run.map_type != MAP_TYPE_NORMAL || run.terminal_state != 0 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let pending = pending_interactive(&run, tables).ok_or(AppError::InvalidRequest)?;
        if pending.kind != expected_kind {
            return Err(AppError::InvalidRequest);
        }
        let config = tables
            .rouge_interactive
            .get(pending.config_id)
            .filter(|row| row.r#type == expected_kind)
            .ok_or(AppError::InvalidRequest)?;
        let params: Vec<i32> = config
            .type_param
            .split('#')
            .map(str::trim)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| AppError::InvalidRequest)?;

        let mut team = persisted_team(&run).ok_or(AppError::InvalidRequest)?;
        let selected_valid = |life: &RougeHeroLife, should_be_alive: bool| {
            let hero_id = life.hero_id.unwrap_or_default();
            unique.contains(&hero_id)
                && if should_be_alive {
                    life.life.unwrap_or_default() > 0
                } else {
                    life.life.unwrap_or_default() <= 0
                }
        };

        match expected_kind {
            INTERACT_HEAL => {
                if params.len() != 3 || params[1] <= 0 || params[2] <= 0 {
                    return Err(AppError::InvalidRequest);
                }
                let selected: Vec<_> = team
                    .hero_life_list
                    .iter()
                    .filter(|life| unique.contains(&life.hero_id.unwrap_or_default()))
                    .collect();
                if selected.len() != unique.len()
                    || !selected.iter().all(|life| selected_valid(life, true))
                    || unique.len() > params[1] as usize
                {
                    return Err(AppError::InvalidRequest);
                }
                let mode = params[0];
                if mode != HEAL_ALL && mode != HEAL_SELECTED {
                    return Err(AppError::InvalidRequest);
                }
                let amount = params[2];
                for life in &mut team.hero_life_list {
                    let current = life.life.unwrap_or_default();
                    let is_target = current > 0
                        && (mode == HEAL_ALL || unique.contains(&life.hero_id.unwrap_or_default()));
                    if is_target {
                        life.life = Some(current.saturating_add(amount).min(1_000));
                    }
                }
            }
            INTERACT_REVIVE => {
                let revive_num = params.first().copied().unwrap_or_default();
                let selected: Vec<_> = team
                    .hero_life_list
                    .iter()
                    .filter(|life| unique.contains(&life.hero_id.unwrap_or_default()))
                    .collect();
                if revive_num <= 0
                    || selected.len() != unique.len()
                    || !selected.iter().all(|life| selected_valid(life, false))
                    || (revive_num != 999 && unique.len() > revive_num as usize)
                {
                    return Err(AppError::InvalidRequest);
                }
                for life in &mut team.hero_life_list {
                    let hero_id = life.hero_id.unwrap_or_default();
                    let is_target = life.life.unwrap_or_default() <= 0
                        && (revive_num == 999 || unique.contains(&hero_id));
                    if is_target {
                        life.life = Some(1_000);
                    }
                }
            }
            _ => return Err(AppError::InvalidRequest),
        }

        let next_index =
            next_supported_interactive(&pending.pipeline, pending.index, tables).unwrap_or(-1);
        let encoded = serde_json::to_string(&team)?;
        let event_id = (next_index < 0).then(|| current_node_event(&run)).flatten();
        let middle = updated_entrust(
            &run,
            EntrustChange {
                finished_event: event_id,
                ..Default::default()
            },
        )?;
        if !rouge::consume_team_interactive(
            db,
            self.player_id,
            season,
            &run.team,
            &encoded,
            pending.index,
            next_index,
            &run.middle_state,
            &middle,
            run.node_id,
            &run.battle_interactive,
        )
        .await?
        {
            return Err(AppError::InvalidRequest);
        }

        let run = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let run = flow::resume(db, run).await?;
        Ok((
            persisted_team(&run).ok_or(AppError::InvalidRequest)?,
            map_info(&run),
        ))
    }

    pub async fn outside_info(
        self,
        db: &SqlitePool,
    ) -> Result<GetRouge2OutsideInfoReply, AppError> {
        rouge2_outside_info(db, self.player_id).await
    }

    pub async fn info(self, db: &SqlitePool) -> Result<GetRouge2InfoReply, AppError> {
        rouge2_info(db, self.player_id).await
    }

    pub async fn unlock_collections(
        self,
        db: &SqlitePool,
    ) -> Result<Rouge2GetUnlockCollectionsReply, AppError> {
        rouge2_unlock_collections(db, self.player_id).await
    }
}

/// Meta progression screen (刻画的枝芽 / talent nodes / point).
pub async fn rouge_outside_info(
    db: &SqlitePool,
    user_id: i64,
    season: i32,
) -> Result<GetRougeOutsideInfoReply, AppError> {
    let outside = rouge::get_or_create_outside(db, user_id, season).await?;
    let extra_balance = extra_rewards::balance(db, user_id, season).await?;
    let genius_ids = rouge::list_genius(db, user_id, season).await?;
    let pass_entrust_id = rouge::completed_entrusts(db, user_id, season).await?;
    let cleared = rouge::cleared_difficulties(db, user_id, season).await?;
    let cached: Option<String> = sqlx::query_scalar(
        "SELECT result_info FROM user_rouge_settlement WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_optional(db)
    .await?;
    let review: Vec<sonettobuf::RougeReviewInfo> = cached
        .and_then(|raw| serde_json::from_str::<sonettobuf::RougeResultInfo>(&raw).ok())
        .and_then(|result| result.review_info)
        .into_iter()
        .collect();
    Ok(GetRougeOutsideInfoReply {
        rouge_info: Some(RougeOutsideInfo {
            season: Some(season),
            genius_point: Some(outside.genius_point),
            genius_ids,
            point: Some(outside.point),
            have_get_point: Some(outside.have_get_point),
            bonus: Some(RougeOutsideBonusNo::default()),
            review: review.clone(),
            game_record_info: Some(RougeGameRecordInfo {
                pass_entrust_id,
                max_difficulty: Some(cleared.into_iter().max().unwrap_or(0)),
                pass_end_id: review
                    .iter()
                    .filter_map(|review| review.end_id)
                    .filter(|id| *id > 0)
                    .collect(),
                ..Default::default()
            }),
            is_genius_new_stage: Some(false),
            limiter_info: None,
            cur_extra_point: Some(extra_balance),
        }),
    })
}

async fn rouge2_outside_info(
    db: &SqlitePool,
    user_id: i64,
) -> Result<GetRouge2OutsideInfoReply, AppError> {
    let rows = rouge::get_or_create_rouge2_outside(db, user_id, config::configs::get()).await?;
    let state = rows.state;

    Ok(GetRouge2OutsideInfoReply {
        outside_info: Some(Rouge2OutsideInfo {
            genius_point: Some(state.genius_point),
            genius_ids: json_ids(&state.genius_ids),
            total_record_info: Some(Rouge2TotalRecordInfo {
                max_difficulty: Some(state.max_difficulty),
                pass_layer_id: json_ids(&state.pass_layer_ids),
                pass_event_id: json_ids(&state.pass_event_ids),
                pass_end_id: json_ids(&state.pass_end_ids),
                pass_entrust_id: json_ids(&state.pass_entrust_ids),
                last_game_time: Some(state.last_game_time),
                pass_collections: json_ids(&state.pass_collections),
                hotfix_str: Some(state.hotfix_str),
            }),
            career_level_info: rows
                .career_levels
                .into_iter()
                .map(|career| Rouge2CareerLevelInfo {
                    career_id: Some(career.career_id),
                    exp: Some(career.exp),
                })
                .collect(),
            reward_info: rows
                .rewards
                .into_iter()
                .map(|reward| Rouge2RewardInfo {
                    id: Some(reward.reward_id),
                    buy_count: Some(reward.buy_count),
                })
                .collect(),
            reward_point: Some(state.reward_point),
            alchemy_info: Some(Rouge2AlchemyInfo {
                cur_alchemy_info: None,
                alchemy_material_info: rows
                    .materials
                    .into_iter()
                    .map(|material| Rouge2AlchemyMaterialInfo {
                        id: Some(material.material_id),
                        num: Some(material.num),
                    })
                    .collect(),
            }),
            review: Vec::new(),
            boss_battle_info: Some(Rouge2BossBattleInfo {
                boss_info: Vec::new(),
                save_info: Vec::new(),
                use_save_index: Some(0),
            }),
        }),
    })
}

async fn rouge2_info(db: &SqlitePool, user_id: i64) -> Result<GetRouge2InfoReply, AppError> {
    let rows = rouge::get_or_create_rouge2_outside(db, user_id, config::configs::get()).await?;
    let state = rows.state;

    Ok(GetRouge2InfoReply {
        rouge2_info: Some(Rouge2Info {
            state: Some(state.state),
            difficulty: Some(state.difficulty),
            coin: Some(state.coin),
            map_info: None,
            bag_info: Some(Rouge2BagInfo { bags: Vec::new() }),
            end_id: Some(state.end_id),
            game_num: Some(state.game_num),
            leader_info: None,
            attr_info: Some(Rouge2AttrInfo { attr: Vec::new() }),
            alchemy_info: None,
        }),
    })
}

async fn rouge2_unlock_collections(
    db: &SqlitePool,
    user_id: i64,
) -> Result<Rouge2GetUnlockCollectionsReply, AppError> {
    let tables = config::configs::get();

    Ok(Rouge2GetUnlockCollectionsReply {
        unlock_relics_ids: rouge::get_or_create_unlock_ids(
            db,
            user_id,
            tables,
            rouge::Rouge2UnlockKind::Relic,
        )
        .await?,
        unlock_buff_ids: rouge::get_or_create_unlock_ids(
            db,
            user_id,
            tables,
            rouge::Rouge2UnlockKind::Buff,
        )
        .await?,
        unlock_active_skill_ids: rouge::get_or_create_unlock_ids(
            db,
            user_id,
            tables,
            rouge::Rouge2UnlockKind::ActiveSkill,
        )
        .await?,
    })
}

fn json_ids(value: &str) -> Vec<i32> {
    serde_json::from_str(value).unwrap_or_default()
}

fn initial_team(hero_ids: &[i32]) -> RougeTeamInfo {
    RougeTeamInfo {
        battle_hero_list: hero_ids
            .iter()
            .take(4)
            .enumerate()
            .map(|(index, hero_id)| RougeBattleHero {
                index: Some(index as i32 + 1),
                hero_id: Some(*hero_id),
                ..Default::default()
            })
            .collect(),
        hero_life_list: hero_ids
            .iter()
            .map(|hero_id| RougeHeroLife {
                hero_id: Some(*hero_id),
                life: Some(1_000),
            })
            .collect(),
        hero_info_list: hero_ids
            .iter()
            .map(|hero_id| RougeHeroInfo {
                hero_id: Some(*hero_id),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn starting_power(tables: &config::GameDB, season: i32, style: i32, difficulty: i32) -> (i32, i32) {
    let Some(style) = tables.rouge_style(season, style) else {
        return (0, 0);
    };
    let mut power = style.power;
    let mut limit = style.power_limit;
    if let Some(difficulty) = tables.rouge_difficulty(difficulty) {
        for modifier in difficulty.start_view.split('|') {
            let Some((name, delta)) = modifier.split_once('#') else {
                continue;
            };
            let Ok(delta) = delta.trim().parse::<i32>() else {
                continue;
            };
            match name.trim() {
                "power" => power = power.saturating_add(delta),
                "powerLimit" => limit = limit.saturating_add(delta),
                _ => {}
            }
        }
    }
    (power.clamp(0, limit.max(0)), limit.max(0))
}

fn persisted_team(run: &RougeRun) -> Option<RougeTeamInfo> {
    serde_json::from_str::<RougeTeamInfo>(&run.team)
        .ok()
        .filter(|team| !team.hero_info_list.is_empty())
}

async fn load_original(db: &SqlitePool, user_id: i64, season: i32) -> Result<RougeStart, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    Ok(rouge::get_or_create_start(db, user_id, season).await?)
}

/// Builds the setup payload straight from the `rouge_style` row, so every
/// catalyst reports its own medium (power) and noise budget.
fn rouge_info_from(start: &RougeStart, run: Option<&RougeRun>) -> RougeInfo {
    let tables = config::configs::get();
    let style = tables.rouge_style(start.season, start.style);
    let (team_size, style_coin) = style.map(|row| (row.capacity, row.coin)).unwrap_or((0, 0));
    let (style_power, style_power_limit) =
        starting_power(tables, start.season, start.style, start.difficulty);
    let (coin, power) = run
        .map(|row| (row.coin, row.power))
        .unwrap_or((style_coin, style_power));
    let power_limit = run
        .map(|row| row.power_limit)
        .filter(|limit| *limit > 0)
        .unwrap_or(style_power_limit);

    let (bag_payload, warehouse_payload) = run
        .and_then(|row| {
            serde_json::from_str::<bag::RunBag>(&row.bag)
                .ok()
                .map(|bag| (row, bag))
        })
        .map(|(run, bag)| {
            (
                artifact_payload::bag_payload(run, &bag),
                sonettobuf::RougeWarehouse {
                    items: bag
                        .items
                        .iter()
                        .filter(|item| !bag::RunBag::is_placed(item))
                        .map(|item| artifact_payload::item_payload(run, item))
                        .collect(),
                },
            )
        })
        .unwrap_or_default();

    let hero_ids = json_ids(&start.hero_ids);
    let team_info = run
        .and_then(persisted_team)
        .unwrap_or_else(|| initial_team(&hero_ids));

    let offered: Vec<i32> = serde_json::from_str(&start.rewards).unwrap_or_default();
    let last_reward = if start.state == 2 {
        offered
            .iter()
            .map(|id| sonettobuf::RougeLastReward {
                id: Some(*id),
                param: None,
            })
            .collect()
    } else {
        Vec::new()
    };

    RougeInfo {
        season: Some(start.season),
        state: Some(start.state),
        last_reward,
        select_reward_num: Some(i32::from(start.state == 2 && !offered.is_empty())),
        difficulty: Some(start.difficulty),
        style: Some(start.style),
        team_level: Some(
            run.map(|run| team_progress(run.season, run.team_exp).0)
                .unwrap_or(i32::from(start.style > 0)),
        ),
        team_exp: Some(
            run.map(|run| team_progress(run.season, run.team_exp).1)
                .unwrap_or(0),
        ),
        team_size: Some(
            run.map(|run| flow::team_capacity(start, run))
                .unwrap_or(team_size),
        ),
        coin: Some(coin),
        power: Some(power),
        power_limit: Some(power_limit),
        retry_num: Some(run.map(|run| run.retry_num).unwrap_or_default()),
        talent_point: Some(run.map(flow::talent_points).unwrap_or_default()),
        talent_tree: run.map(|run| flow::talent_tree(start, run)),
        end_id: Some(run.map(|run| flow::details(run).end_id).unwrap_or_default()),
        map_info: run.map(map_info),
        team_info: Some(team_info),
        bag: Some(bag_payload),
        warehouse: Some(warehouse_payload),
        ..Default::default()
    }
}

/// Client map types (`RougeMapEnum.MapType`).
const MAP_TYPE_NORMAL: i32 = 1;
const MAP_TYPE_MIDDLE: i32 = 2;
const INTERACT_DROP: i32 = 1;
const INTERACT_ADVANCE_DROP: i32 = 12;
const INTERACT_RECRUIT: i32 = 5;
const INTERACT_LOSS_NON_UNIQUE: i32 = 9;
const INTERACT_HEAL: i32 = 3;
const INTERACT_REVIVE: i32 = 4;
const HEAL_ALL: i32 = 1;
const HEAL_SELECTED: i32 = 2;

/// These direct, cost-free choices have matching reward values in both the
/// shipped drop table and localized descriptions. Other resource-only choices
/// also encode random outcomes/costs not represented by the Drop row alone.
pub fn is_verified_resource_choice(choice_id: i32) -> bool {
    matches!(choice_id, 10610501 | 10613801 | 10613902 | 10614003)
}

fn team_progress(season: i32, exp: i32) -> (i32, i32) {
    let mut level = 1;
    let mut remaining = exp.max(0);
    loop {
        let Some(next) = config::configs::get()
            .rouge_level
            .iter()
            .find(|row| row.season == season && row.level == level + 1)
        else {
            break;
        };
        if next.exp <= 0 || remaining < next.exp {
            break;
        }
        remaining -= next.exp;
        level += 1;
    }
    (level, remaining)
}

/// Builds the response-bearing post-fight pipeline from shipped event data.
pub fn battle_reward_pipeline(event_id: i32, _final_node: bool) -> Option<(String, i32)> {
    let tables = config::configs::get();
    let event = tables.rouge_fight_event.get(event_id)?;
    let pipeline = event.advance_interactive.clone();
    first_supported_interactive(&pipeline, tables).map(|index| (pipeline, index))
}

fn drop_coin(row: &config::rouge_drop::RougeDrop) -> i32 {
    let mut parts = row.coin.split('#');
    let amount = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let probability = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(100);
    if probability == 100 && parts.next().is_none() {
        amount.max(0)
    } else {
        0
    }
}

pub fn battle_reward_preview(event_id: i32) -> (i32, i32, i32, i32) {
    let tables = config::configs::get();
    let drop = tables
        .rouge_fight_event
        .get(event_id)
        .and_then(|event| supported_interactive(&event.advance_interactive))
        .filter(|(kind, _)| *kind == INTERACT_ADVANCE_DROP)
        .and_then(|(_, id)| tables.rouge_drop.get(id));
    drop.map(|row| {
        let count = row
            .drop
            .split('|')
            .filter_map(|entry| entry.split('#').next()?.parse::<i32>().ok())
            .sum();
        (
            drop_coin(row),
            flow::reward_exp(row.id),
            count,
            row.select_count.max(0),
        )
    })
    .unwrap_or_default()
}

pub fn battle_automatic_reward(event_id: i32) -> (i32, i32) {
    let tables = config::configs::get();
    tables
        .rouge_fight_event
        .get(event_id)
        .and_then(|event| supported_interactive(&event.advance_interactive))
        .filter(|(kind, _)| *kind == INTERACT_ADVANCE_DROP)
        .and_then(|(_, id)| tables.rouge_drop.get(id))
        .filter(|row| row.select_count == 0 && row.drop.is_empty())
        .map(|row| (drop_coin(row), row.exp.max(0)))
        .unwrap_or_default()
}

pub fn battle_reward_preview_for(run: &RougeRun, event_id: i32) -> (i32, i32, i32, i32) {
    let tables = config::configs::get();
    let Some(id) = tables
        .rouge_fight_event
        .get(event_id)
        .and_then(|event| supported_interactive(&event.advance_interactive))
        .filter(|(kind, _)| *kind == INTERACT_ADVANCE_DROP)
        .map(|(_, id)| id)
    else {
        return (0, 0, 0, 0);
    };
    let reward = flow::reward(run, id);
    let drop = drop_state(run, id, tables);
    (
        reward.coin,
        reward.exp,
        drop.as_ref()
            .map(|drop| drop.offers.len() as i32)
            .unwrap_or(0),
        drop.map(|drop| drop.select_count).unwrap_or(0),
    )
}

pub fn battle_automatic_reward_for(run: &RougeRun, event_id: i32) -> (i32, i32) {
    let tables = config::configs::get();
    let id = tables
        .rouge_fight_event
        .get(event_id)
        .and_then(|event| supported_interactive(&event.advance_interactive))
        .filter(|(kind, _)| *kind == INTERACT_ADVANCE_DROP)
        .map(|(_, id)| id);
    if let Some(id) = id
        && tables
            .rouge_drop
            .get(id)
            .is_some_and(|drop| drop.select_count == 0 && drop.drop.is_empty())
    {
        let reward = flow::reward(run, id);
        (reward.coin, reward.exp)
    } else {
        (0, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingInteractive {
    index: i32,
    kind: i32,
    config_id: i32,
    raw: String,
    pipeline: String,
}

fn supported_interactive(segment: &str) -> Option<(i32, i32)> {
    if segment.trim() == "5" {
        return Some((INTERACT_RECRUIT, 0));
    }
    let (kind, config_id) = segment.trim().split_once('#')?;
    let kind = kind.trim().parse().ok()?;
    let config_id = config_id.trim().parse().ok()?;
    matches!(
        kind,
        INTERACT_DROP
            | INTERACT_ADVANCE_DROP
            | INTERACT_HEAL
            | INTERACT_REVIVE
            | INTERACT_LOSS_NON_UNIQUE
    )
    .then_some((kind, config_id))
}

fn interactive_requires_response(segment: &str, tables: &config::GameDB) -> bool {
    let Some((kind, config_id)) = supported_interactive(segment) else {
        return false;
    };
    match kind {
        INTERACT_DROP | INTERACT_ADVANCE_DROP => tables
            .rouge_drop
            .get(config_id)
            .is_some_and(|row| row.select_count > 0 && !row.drop.is_empty()),
        INTERACT_HEAL | INTERACT_REVIVE => tables
            .rouge_interactive
            .get(config_id)
            .is_some_and(|row| row.r#type == kind),
        INTERACT_LOSS_NON_UNIQUE => config_id > 0,
        INTERACT_RECRUIT => true,
        _ => false,
    }
}

fn first_supported_interactive(pipeline: &str, tables: &config::GameDB) -> Option<i32> {
    pipeline
        .split('|')
        .position(|segment| interactive_requires_response(segment, tables))
        .map(|index| index as i32)
}

fn next_supported_interactive(
    pipeline: &str,
    current: i32,
    tables: &config::GameDB,
) -> Option<i32> {
    let start = usize::try_from(current).ok()?.saturating_add(1);
    pipeline
        .split('|')
        .enumerate()
        .skip(start)
        .find(|(_, segment)| interactive_requires_response(segment, tables))
        .map(|(index, _)| index as i32)
}

fn pending_interactive(run: &RougeRun, tables: &config::GameDB) -> Option<PendingInteractive> {
    let index = usize::try_from(run.interactive_index).ok()?;
    let pipeline = if !run.battle_interactive.is_empty() {
        run.battle_interactive.clone()
    } else {
        let choices: HashMap<String, i32> = serde_json::from_str(&run.choices).ok()?;
        let choice_id = choices.get(&run.node_id.to_string())?;
        tables.rouge_choice.get(*choice_id)?.interactive.clone()
    };
    let raw = pipeline.split('|').nth(index)?.trim();
    let (kind, config_id) = supported_interactive(raw)?;
    if !interactive_requires_response(raw, tables) {
        return None;
    }
    Some(PendingInteractive {
        index: run.interactive_index,
        kind,
        config_id,
        raw: raw.to_owned(),
        pipeline: pipeline.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DropState {
    offers: Vec<i32>,
    select_count: i32,
}

fn drop_state(run: &RougeRun, drop_id: i32, tables: &config::GameDB) -> Option<DropState> {
    let row = tables.rouge_drop.get(drop_id)?;
    if row.select_count <= 0 || row.drop.is_empty() {
        return None;
    }
    let mut offers = Vec::new();
    let refresh = run.interactive_refresh.max(0) as u64;
    let reward = flow::reward(run, drop_id);
    let last_entry = row.drop.split('|').count().saturating_sub(1);
    for (entry_index, entry) in row.drop.split('|').enumerate() {
        let mut fields = entry.split('#');
        let count: i32 = fields.next()?.trim().parse().ok()?;
        let group_id: i32 = fields.next()?.trim().parse().ok()?;
        let rate: i32 = fields.next()?.trim().parse().ok()?;
        if fields.next().is_some() || count <= 0 || rate <= 0 {
            return None;
        }
        let count = count
            + if entry_index == last_entry {
                reward.extra_drops
            } else {
                0
            };
        for draw in 0..count {
            let base_salt = (drop_id as u64)
                .wrapping_mul(257)
                .wrapping_add(refresh.wrapping_mul(65_537))
                .wrapping_add((entry_index as u64).wrapping_mul(131))
                .wrapping_add(draw as u64);
            let rows = tables.rouge_drop_group.by_group(group_id).count();
            let mut selected = None;
            for attempt in 0..rows.max(1) {
                let item = last_reward_drop(
                    tables,
                    group_id,
                    run.seed as u64,
                    base_salt.wrapping_add((attempt as u64).wrapping_mul(7_919)),
                )?;
                if !offers.contains(&item) || attempt + 1 == rows.max(1) {
                    selected = Some(item);
                    break;
                }
            }
            offers.push(selected?);
        }
    }
    let select_count = row.select_count + reward.extra_select;
    (offers.len() >= select_count as usize).then_some(DropState {
        offers,
        select_count,
    })
}

fn interactive_json(
    run: &RougeRun,
    pending: &PendingInteractive,
    tables: &config::GameDB,
) -> Option<String> {
    if pending.kind == INTERACT_LOSS_NON_UNIQUE {
        return Some(
            serde_json::json!({"type": 9, "lostNum": pending.config_id,
            "paramStr": pending.raw, "fromType": 1})
            .to_string(),
        );
    }
    if pending.kind == INTERACT_RECRUIT {
        return Some(serde_json::json!({"type": 5, "fromType": 6, "paramStr": "5"}).to_string());
    }
    if matches!(pending.kind, INTERACT_DROP | INTERACT_ADVANCE_DROP) {
        let drop = drop_state(run, pending.config_id, tables)?;
        return Some(
            serde_json::json!({
                "dropCollectList": drop.offers,
                "dropSelectNum": drop.select_count,
                "dropRandomNum": run.interactive_refresh.max(0),
                "type": pending.kind,
                "dropId": pending.config_id,
                "paramStr": pending.raw,
                "fromType": 1,
            })
            .to_string(),
        );
    }

    let row = tables.rouge_interactive.get(pending.config_id)?;
    let params: Vec<i32> = row
        .type_param
        .split('#')
        .map(str::trim)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let value = match pending.kind {
        INTERACT_HEAL if params.len() == 3 => serde_json::json!({
            "healType": params[0],
            "healNum": params[1],
            "healRate": params[2],
        }),
        INTERACT_REVIVE if !params.is_empty() => serde_json::json!({
            "reviveNum": params[0],
            "reviveType": params.get(1).copied().unwrap_or_default(),
        }),
        _ => return None,
    };
    Some(value.to_string())
}

fn choice_is_unlocked(run: &RougeRun, choice: &config::rouge_choice::RougeChoice) -> bool {
    let parse = || choice.unlock_param.trim().parse::<i32>().ok();
    let bag = || serde_json::from_str::<bag::RunBag>(&run.bag).ok();
    let team = || persisted_team(run);
    match choice.unlock_type {
        0 => true,
        // PossessCollection
        1 => parse().is_some_and(|item_id| {
            bag().is_some_and(|bag| bag.items.iter().any(|item| item.item_id == item_id))
        }),
        // PossessCoin
        2 => parse().is_some_and(|need| run.coin >= need),
        // PossessHero
        3 => parse().is_some_and(|need| {
            team().is_some_and(|team| team.hero_life_list.len() >= need.max(0) as usize)
        }),
        // PossessPower
        4 => parse().is_some_and(|need| run.power >= need),
        // PossessCollectionCount
        5 => parse()
            .is_some_and(|need| bag().is_some_and(|bag| bag.items.len() >= need.max(0) as usize)),
        // DeadHeroNum
        17 => parse().is_some_and(|need| {
            team().is_some_and(|team| {
                team.hero_life_list
                    .iter()
                    .filter(|life| life.life.unwrap_or_default() <= 0)
                    .count()
                    >= need.max(0) as usize
            })
        }),
        // HadNotUniqueCollectionNum
        18 => parse().is_some_and(|need| {
            bag().is_some_and(|bag| {
                bag.items
                    .iter()
                    .flat_map(|item| {
                        std::iter::once(item.item_id).chain(item.holes.iter().copied())
                    })
                    .filter(|item_id| *item_id != 0 && !collection_shapes::is_unique(*item_id))
                    .count()
                    >= need.max(0) as usize
            })
        }),
        // DLC and persistent-history gates are intentionally unavailable in the
        // base-game run until their corresponding systems are implemented.
        _ => false,
    }
}

fn run_position(run: &RougeRun) -> RougeRunPosition {
    RougeRunPosition {
        map_type: run.map_type,
        layer_index: run.layer_index,
        layer_id: run.layer_id,
        stage: run.stage,
        node_id: run.node_id,
        middle_layer_id: run.middle_layer_id,
        position_index: run.position_index,
    }
}

fn current_node_event(run: &RougeRun) -> Option<i32> {
    serde_json::from_str::<map::LayerMap>(&run.graph)
        .ok()?
        .node(run.node_id)
        .map(|node| node.event_id)
}

fn node_is_resolved(run: &RougeRun, node: &map::MapNode) -> bool {
    if node.event_id == 0 {
        return true;
    }
    if node.node_id == run.node_id && pending_interactive(run, config::configs::get()).is_some() {
        return false;
    }
    let finished: Vec<i32> = serde_json::from_str(&run.finished).unwrap_or_default();
    if finished.contains(&node.node_id) {
        return true;
    }
    let choices: HashMap<String, i32> = serde_json::from_str(&run.choices).unwrap_or_default();
    choices.contains_key(&node.node_id.to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RunStats {
    minor: i32,
    danger: i32,
    crisis: i32,
    steps: i32,
}

fn current_layer_stats(run: &RougeRun) -> Result<RunStats, AppError> {
    let layer_map: map::LayerMap = serde_json::from_str(&run.graph)?;
    let mut resolved: HashSet<i32> = serde_json::from_str::<Vec<i32>>(&run.finished)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let choices: HashMap<String, i32> = serde_json::from_str(&run.choices).unwrap_or_default();
    resolved.extend(
        choices
            .keys()
            .filter_map(|node_id| node_id.parse::<i32>().ok()),
    );

    let mut stats = RunStats {
        steps: resolved.len() as i32,
        ..Default::default()
    };
    for node in &layer_map.nodes {
        if !resolved.contains(&node.node_id) {
            continue;
        }
        match node.node_type {
            map::NODE_MINOR_OBSTACLE => stats.minor += 1,
            map::NODE_PRESENT_DANGER => stats.danger += 1,
            map::NODE_BOSS | 3 => stats.crisis += 1,
            _ => {}
        }
    }
    Ok(stats)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MiddleState {
    pieces: Vec<RougePieceInfo>,
    entrust_id: i32,
    #[serde(default)]
    entrust_progress: i32,
    #[serde(default)]
    completed_entrusts: Vec<i32>,
    #[serde(default)]
    middle_layer_id: i32,
}

fn entrust_completed(entrust: &config::rouge_entrust::RougeEntrust, progress: i32) -> bool {
    let threshold = match entrust.r#type {
        3 => entrust
            .param
            .split('|')
            .nth(1)
            .and_then(|v| v.parse::<i32>().ok()),
        4 => Some(0),
        7 => Some(1),
        1 | 2 | 5 | 6 | 8 | 9 => entrust.param.parse::<i32>().ok(),
        _ => None,
    };
    threshold.is_some_and(|threshold| threshold >= 0 && progress >= threshold)
}

#[derive(Default)]
struct EntrustChange {
    gained_coin: i32,
    spent_coin: i32,
    gained_power: i32,
    spent_power: i32,
    collections: i32,
    entered_type: Option<i32>,
    finished_event: Option<i32>,
}

fn updated_entrust(run: &RougeRun, change: EntrustChange) -> Result<String, AppError> {
    let Ok(mut state) = serde_json::from_str::<MiddleState>(&run.middle_state) else {
        return Ok(run.middle_state.clone());
    };
    let Some(entrust) = config::configs::get().rouge_entrust.get(state.entrust_id) else {
        return Ok(run.middle_state.clone());
    };
    let amount = match entrust.r#type {
        1 => change.gained_coin,
        2 => change.spent_coin,
        3 => {
            let types = entrust.param.split('|').next().unwrap_or_default();
            i32::from(change.entered_type.is_some_and(|kind| {
                types
                    .split('#')
                    .any(|id| id.parse::<i32>().ok() == Some(kind))
            }))
        }
        // Client labels use the opposite power directions for types 5 and 6.
        5 => change.gained_power,
        6 => change.spent_power,
        7 => i32::from(
            change
                .finished_event
                .is_some_and(|id| entrust.param.parse::<i32>().ok() == Some(id)),
        ),
        8 => change.collections,
        _ => 0,
    }
    .max(0);
    if amount == 0 {
        return Ok(run.middle_state.clone());
    }
    state.entrust_progress = state.entrust_progress.saturating_add(amount);
    Ok(serde_json::to_string(&state)?)
}

pub fn automatic_fight_entrust_update(
    run: &RougeRun,
    gained_coin: i32,
    event_id: Option<i32>,
) -> Result<String, AppError> {
    updated_entrust(
        run,
        EntrustChange {
            gained_coin,
            finished_event: event_id,
            ..Default::default()
        },
    )
}

pub fn automatic_fight_resource_update(
    run: &RougeRun,
    gained_coin: i32,
    gained_power: i32,
) -> Result<String, AppError> {
    updated_entrust(
        run,
        EntrustChange {
            gained_coin,
            gained_power,
            ..Default::default()
        },
    )
}

fn middle_state(run: &RougeRun) -> MiddleState {
    let previous = serde_json::from_str::<MiddleState>(&run.middle_state).ok();
    if let Some(state) = &previous {
        if state.middle_layer_id == run.middle_layer_id {
            return MiddleState {
                pieces: state.pieces.clone(),
                entrust_id: state.entrust_id,
                entrust_progress: state.entrust_progress,
                completed_entrusts: state.completed_entrusts.clone(),
                middle_layer_id: state.middle_layer_id,
            };
        }
    }
    let tables = config::configs::get();
    let mut main = Vec::new();
    let mut others = Vec::new();
    for piece in tables
        .rouge_piece
        .iter()
        .filter(|piece| piece.version.is_empty() && piece.id / 10000 == run.middle_layer_id)
    {
        // Unlocked alternate endings must be selected by their story branch;
        // randomly substituting one can leave the final room with no usable
        // boss option. The recorded base route always has ending piece 1510501.
        if run.middle_layer_id == 151 && piece.entrust_type == 5 && piece.id != 1510501 {
            continue;
        }
        if piece.entrust_type == 1 {
            main.push(piece);
        } else {
            others.push(piece);
        }
    }
    main.sort_by_key(|piece| piece.id);
    others.sort_by_key(|piece| piece.id);
    if !others.is_empty() {
        let offset = (run.seed.unsigned_abs() as usize) % others.len();
        others.rotate_left(offset);
        // The base board offers an entrust NPC alongside other piece types.
        if let Some(index) = others.iter().position(|piece| piece.entrust_type == 2) {
            others.swap(0, index);
        }
    }
    let pieces = main
        .into_iter()
        .take(1)
        .chain({
            // One piece per role keeps story/final-boss interactions available
            // instead of filling every slot with copies of quest NPCs.
            let mut roles = HashSet::new();
            others
                .into_iter()
                .filter(move |piece| roles.insert(piece.entrust_type))
                .take(4)
        })
        .enumerate()
        .map(|(i, piece)| RougePieceInfo {
            index: Some(i as i32 + 1),
            id: Some(piece.id),
            talk_id: Some(piece.talk_id),
            select_id: Some(0),
            trigger_str: None,
            finish: Some(false),
        })
        .collect();
    MiddleState {
        pieces,
        entrust_id: previous.as_ref().map(|state| state.entrust_id).unwrap_or(0),
        entrust_progress: previous
            .as_ref()
            .map(|state| state.entrust_progress)
            .unwrap_or(0),
        completed_entrusts: previous
            .as_ref()
            .map(|state| state.completed_entrusts.clone())
            .unwrap_or_default(),
        middle_layer_id: run.middle_layer_id,
    }
}

/// Builds the client-facing map payload for the run's current map type.
pub fn map_info(run: &RougeRun) -> RougeMapInfo {
    if run.map_type == MAP_TYPE_MIDDLE {
        let state = middle_state(run);
        let pending = pending_interactive(run, config::configs::get());
        return RougeMapInfo {
            map_type: Some(MAP_TYPE_MIDDLE),
            middle_layer_info: Some(RougeMiddleLayerInfo {
                layer_id: Some(run.layer_id),
                middle_layer_id: Some(run.middle_layer_id),
                position_index: Some(run.position_index),
                piece_info: state.pieces,
                layer_choice_info: Vec::new(),
            }),
            rouge_entrust: (state.entrust_id != 0).then_some(RougeEntrustInfo {
                id: Some(state.entrust_id),
                count: Some(state.entrust_progress),
            }),
            cur_interactive_index: pending.as_ref().map(|item| item.index),
            cur_interactive: pending.as_ref().map(|item| item.raw.clone()),
            interactive_json: pending
                .as_ref()
                .and_then(|item| interactive_json(run, item, config::configs::get())),
            ..Default::default()
        };
    }

    let layer_map: map::LayerMap = serde_json::from_str(&run.graph).unwrap_or(map::LayerMap {
        layer_id: run.layer_id,
        stages: 0,
        nodes: Vec::new(),
    });
    let finished: HashSet<i32> = serde_json::from_str::<Vec<i32>>(&run.finished)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let choices: HashMap<String, i32> = serde_json::from_str(&run.choices).unwrap_or_default();
    let tables = config::configs::get();
    let pending = pending_interactive(run, tables);
    let node_info = layer_map
        .nodes
        .iter()
        .map(|node| NodeInfo {
            layer: Some(run.layer_id),
            stage: Some(node.stage),
            node_id: Some(node.node_id),
            last_node_list: node.last.clone(),
            next_node_list: node.next.clone(),
            event_id: Some(node.event_id),
            event_data: Some(node_event_data(run, node, &finished, &choices, tables)),
            path_way: Some(false),
            interactive9drop: Some(0),
            interactive10drop: Some(0),
            fog: Some(false),
        })
        .collect();

    RougeMapInfo {
        map_type: Some(MAP_TYPE_NORMAL),
        rouge_entrust: serde_json::from_str::<MiddleState>(&run.middle_state)
            .ok()
            .filter(|state| state.entrust_id != 0)
            .map(|state| RougeEntrustInfo {
                id: Some(state.entrust_id),
                count: Some(state.entrust_progress),
            }),
        layer_info: Some(RougeLayerInfo {
            layer_id: Some(run.layer_id),
            cur_stage: Some(run.stage),
            cur_node: Some(run.node_id),
            node_info,
            ..Default::default()
        }),
        cur_interactive_index: pending.as_ref().map(|item| item.index),
        cur_interactive: pending.as_ref().map(|item| item.raw.clone()),
        interactive_json: pending
            .as_ref()
            .and_then(|item| interactive_json(run, item, tables)),
        ..Default::default()
    }
}

fn node_event_data(
    run: &RougeRun,
    node: &map::MapNode,
    finished: &HashSet<i32>,
    choices: &HashMap<String, i32>,
    tables: &config::GameDB,
) -> String {
    if node.event_id == 0 {
        return "{}".to_owned();
    }

    let choice_select = choices.get(&node.node_id.to_string()).copied();
    let state = if node.node_id == run.node_id
        && !run.battle_interactive.is_empty()
        && pending_interactive(run, tables)
            .is_some_and(|pending| pending.kind == INTERACT_ADVANCE_DROP)
    {
        1
    } else if finished.contains(&node.node_id) || choice_select.is_some() {
        2 // RougeMapEnum.EventState.Finish
    } else if node.node_id == run.node_id {
        1 // RougeMapEnum.EventState.Start
    } else {
        0 // RougeMapEnum.EventState.Init
    };
    let event_type = tables
        .rouge_event(node.event_id)
        .map(|event| event.r#type)
        .unwrap_or(node.node_type);
    let can_choice_set: Vec<i32> = tables
        .rouge_choices_for_event(node.event_id)
        .iter()
        .filter(|choice| choice.version.is_empty())
        .map(|choice| choice.id)
        .collect();

    let mut bought_goods = Vec::new();
    let mut goods_map = serde_json::Map::new();
    if node.node_type == map::NODE_SHOP
        && let Ok(shop) = serde_json::from_str::<shop::ShopState>(&run.shop)
        && shop.node_id == node.node_id
    {
        bought_goods.clone_from(&shop.bought);
        for (index, goods_id) in shop.stock.iter().copied().enumerate() {
            let position = index as i32 + 1;
            if let Some(goods) = tables.rouge_goods.get(goods_id) {
                goods_map.insert(
                    position.to_string(),
                    serde_json::json!({
                        "collectionId": goods.creator,
                        "discountRate": if flow::shop_price(run,goods.price) < goods.price { flow::shop_price(run,goods.price) * 1000 / goods.price.max(1) } else { -1 },
                        "originalPrice": goods.price,
                        "price": flow::shop_price(run,goods.price),
                    }),
                );
            }
        }
    }

    serde_json::json!({
        "state": state,
        "eventId": node.event_id,
        "type": event_type,
        "fightFail": node.node_id == run.node_id && run.retry_num > 0,
        "surpriseAttackList": [],
        "canChoiceSet": can_choice_set,
        "choiceSelect": choice_select.unwrap_or_default(),
        "boughtGoodsPosSet": bought_goods,
        "posGoodMap": goods_map,
        "refreshNum": serde_json::from_str::<shop::ShopState>(&run.shop)
            .ok()
            .filter(|shop| shop.node_id == node.node_id)
            .map(|shop| shop.refresh_num)
            .unwrap_or_default(),
        "refreshNeedCoin": shop::refresh_price(tables,
            serde_json::from_str::<shop::ShopState>(&run.shop).ok()
                .filter(|shop| shop.node_id == node.node_id).map(|shop| shop.refresh_num).unwrap_or(0)),
    })
    .to_string()
}

/// Parses the middle-layer board attached to a layer.
fn middle_board(tables: &config::GameDB, layer_id: i32) -> Result<middle::MiddleBoard, AppError> {
    let layer = tables
        .rouge_layer
        .iter()
        .find(|row| row.id == layer_id)
        .ok_or(AppError::InvalidRequest)?;
    let row = tables
        .rouge_middle_layer
        .get(layer.middle_layer_id)
        .ok_or(AppError::InvalidRequest)?;
    middle::parse(row)
}

/// Generates a layer map with the configured stage count.
fn generate_layer(layer_id: i32, seed: u64) -> map::LayerMap {
    let tables = config::configs::get();
    // Captured base route: 5/6/7/8/6 columns, including the starting point.
    // Parallel areas share the count; their unrecorded distributions are seeded
    // approximations rather than a replay of this player's node ids.
    let area = layer_id / 10 % 10;
    let stages = match area {
        1 => 5,
        2 => 6,
        3 => 7,
        4 => 8,
        5 => 6,
        _ => map::DEFAULT_STAGES,
    };
    let mut pool = event_pool(tables);
    for kind in [1, 2, 3, 4] {
        if let Some(events) = pool.get_mut(&kind) {
            let local: Vec<i32> = events
                .iter()
                .copied()
                .filter(|id| id / 100 % 10 == area)
                .collect();
            if !local.is_empty() {
                *events = local;
            }
        }
    }
    let mut graph = map::generate(layer_id, stages, seed, &pool);
    // The first area ends in a danger, the final exploration in respite; the
    // ending boss lives in the following safe zone.
    let terminal_type = match area {
        1 => 2,
        5 => 8,
        _ => 3,
    };
    if let Some(ids) = pool.get(&terminal_type).filter(|ids| !ids.is_empty()) {
        for node in graph.nodes.iter_mut().filter(|node| node.next.is_empty()) {
            node.node_type = terminal_type;
            node.event_id = ids[seed as usize % ids.len()];
        }
    }
    graph
}

/// Groups `rouge_event` rows by node type so the generator can pick real encounters.
///
/// Only a minority of events carry `rouge_choice` options, so interactive events are
/// preferred per type; types without any interactive event fall back to the full pool.
fn event_pool(tables: &config::GameDB) -> HashMap<i32, Vec<i32>> {
    let with_choices: std::collections::HashSet<i32> = tables
        .rouge_event
        .iter()
        .filter(|row| !tables.rouge_choices_for_event(row.id).is_empty())
        .map(|row| row.id)
        .collect();

    // Battle nodes must resolve to a real `rouge_fight_event` (they carry the episode).
    let fight_ids: std::collections::HashSet<i32> = tables
        .rouge_fight_event
        .iter()
        .filter(|row| row.episode_id != 0)
        .map(|row| row.id)
        .collect();

    let mut preferred: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut fallback: HashMap<i32, Vec<i32>> = HashMap::new();
    for row in tables
        .rouge_event
        .iter()
        .filter(|row| row.version.is_empty())
    {
        let interactive = with_choices.contains(&row.id) || fight_ids.contains(&row.id);
        if interactive {
            preferred.entry(row.r#type).or_default().push(row.id);
        } else {
            fallback.entry(row.r#type).or_default().push(row.id);
        }
    }

    let mut pool = fallback;
    for (node_type, ids) in preferred {
        pool.insert(node_type, ids);
    }
    for ids in pool.values_mut() {
        ids.sort_unstable();
    }
    pool
}

/// Next-run bonuses offered on the post-run screen. These IDs belong to
/// `rouge_last_reward`; sending `rouge_reward` progression IDs makes the client
/// look up a missing config row and breaks the selection view.
fn offered_rewards(tables: &config::GameDB, difficulty: i32) -> Vec<i32> {
    let group = match difficulty {
        ..=4 => 2,
        5..=9 => 3,
        10..=14 => 4,
        _ => 5,
    };
    let mut ids: Vec<i32> = tables
        .rouge_last_reward
        .iter()
        .filter(|row| row.season == 1 && row.id / 10 == group)
        .map(|row| row.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Badges earned this run.
///
/// Only the triggers that can be derived from run state are evaluated:
/// 1 = battles fought, 2 = artefacts obtained, 8 = crisis battles won.
/// The remaining triggers need combat telemetry that is not tracked yet.
fn earned_badges(
    tables: &config::GameDB,
    battles: i32,
    collections: i32,
    crisis_battles: i32,
    spent_coin: i32,
    compositions: i32,
) -> Vec<&config::rouge_badge::RougeBadge> {
    let mut earned = Vec::new();
    for badge in tables.rouge_badge.iter() {
        if badge.season != 1 {
            continue;
        }
        let need: i32 = badge.trigger_param.trim().parse().unwrap_or(i32::MAX);
        let have = match badge.trigger {
            1 => battles,
            2 => collections,
            3 => compositions,
            5 => spent_coin,
            8 => crisis_battles,
            _ => continue,
        };
        if have >= need {
            earned.push(badge);
        }
    }
    earned
}

/// Layer bonus from `rouge_const` 1015 (`1#750|2#1750|...`).
fn layer_score_for(tables: &config::GameDB, layers: i32) -> i32 {
    let raw = tables
        .rouge_const
        .get(1015)
        .map(|row| row.value.clone())
        .unwrap_or_default();
    raw.split('|')
        .filter_map(|entry| {
            let (key, value) = entry.split_once('#')?;
            (key.trim().parse::<i32>().ok()? == layers)
                .then(|| value.trim().parse::<i32>().ok())
                .flatten()
        })
        .next()
        .unwrap_or(0)
}

fn const_i32(tables: &config::GameDB, id: i32, fallback: i32) -> i32 {
    tables
        .rouge_const
        .get(id)
        .and_then(|row| row.value.trim().parse::<i32>().ok())
        .unwrap_or(fallback)
}

fn last_reward_drop(tables: &config::GameDB, group_id: i32, seed: u64, salt: u64) -> Option<i32> {
    let rows: Vec<_> = tables.rouge_drop_group.by_group(group_id).collect();
    let total: u64 = rows.iter().map(|row| row.rate.max(1) as u64).sum();
    if total == 0 {
        return None;
    }
    let mixed = seed
        .wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(group_id as u64)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mut roll = (mixed ^ (mixed >> 31)) % total;
    for row in rows {
        let weight = row.rate.max(1) as u64;
        if roll < weight {
            return Some(row.collection_id);
        }
        roll -= weight;
    }
    None
}

fn apply_last_reward(
    tables: &config::GameDB,
    reward_id: i32,
    seed: u64,
    coin: &mut i32,
    power_limit: &mut i32,
    bag: &mut bag::RunBag,
) -> Result<(), AppError> {
    match reward_id % 10 {
        // Obtain 50 Goldfinch Coins.
        1 => *coin = coin.saturating_add(50),
        // Seven-Day Winder.
        2 => {
            bag.add(8_130_804)?;
        }
        // One random Common artefact (`rouge_drop_group` 3).
        3 => {
            let item = last_reward_drop(tables, 3, seed, 3).ok_or(AppError::InvalidRequest)?;
            bag.add(item)?;
        }
        // One random Medication (`rouge_drop_group` 1001).
        4 => {
            let item = last_reward_drop(tables, 1001, seed, 4).ok_or(AppError::InvalidRequest)?;
            bag.add(item)?;
        }
        // Two random Ritual Disks (`rouge_drop_group` 1).
        5 => {
            for salt in [5, 6] {
                let item =
                    last_reward_drop(tables, 1, seed, salt).ok_or(AppError::InvalidRequest)?;
                bag.add(item)?;
            }
        }
        // Max Stimulus +10.
        6 => *power_limit = power_limit.saturating_add(10),
        _ => return Err(AppError::InvalidRequest),
    }
    Ok(())
}

/// Creates the first layer and stores the run.
async fn start_run(
    db: &SqlitePool,
    user_id: i64,
    start: &RougeStart,
) -> Result<RougeRun, AppError> {
    let tables = config::configs::get();
    // `rouge_const` 47 = first layer id (111), 45 = stages per layer (12).
    let layer_id = const_i32(tables, 47, 111);
    let seed = common::time::ServerTime::now_ms() as u64 ^ (user_id as u64) << 17;

    let layer_map = generate_layer(layer_id, seed);
    let start_node = layer_map
        .start_node()
        .ok_or(AppError::InvalidRequest)?
        .clone();
    let graph = serde_json::to_string(&layer_map)?;

    let style = tables.rouge_style(start.season, start.style);
    let genius_ids = rouge::list_genius(db, user_id, start.season).await?;
    let growth = genius_bonuses::from_ids(tables, &genius_ids);
    let mut coin = style.map(|row| row.coin).unwrap_or_default() + growth.initial_coin;
    let (power, mut power_limit) =
        starting_power(tables, start.season, start.style, start.difficulty);

    // The catalyst's `layoutId` selects the backpack layout and its starting artefact.
    let mut bag = match style.and_then(|row| tables.rouge_backpack(row.layout_id)) {
        Some(layout) => bag::RunBag::from_layout(&layout.layout, &layout.initial_collection)?,
        None => bag::RunBag {
            rows: 0,
            cols: 0,
            items: Vec::new(),
            uid_counter: 0,
        },
    };
    let selected_rewards: Vec<i32> = json_ids(&start.rewards);
    if let Some(reward_id) = selected_rewards.first().copied() {
        if selected_rewards.len() != 1
            || tables
                .rouge_last_reward
                .get(reward_id)
                .is_none_or(|reward| reward.season != start.season)
        {
            return Err(AppError::InvalidRequest);
        }
        apply_last_reward(
            tables,
            reward_id,
            seed,
            &mut coin,
            &mut power_limit,
            &mut bag,
        )?;
    }
    let bag_json = serde_json::to_string(&bag)?;
    let team_json = serde_json::to_string(&initial_team(&json_ids(&start.hero_ids)))?;

    if !rouge::create_run(
        db,
        user_id,
        start.season,
        0,
        layer_id,
        start_node.stage,
        start_node.node_id,
        seed as i64,
        &graph,
        coin,
        power,
        &bag_json,
        &team_json,
        power_limit,
        &start.rewards,
    )
    .await?
    {
        return Err(AppError::InvalidRequest);
    }

    let mut run = rouge::get_run(db, user_id, start.season)
        .await?
        .ok_or(AppError::InvalidRequest)?;
    let before = run.clone();
    let mut details = flow::details(&run);
    details.outside_genius = Some(genius_ids);
    details.safe_zone_recovery = style.map(|style| style.power);
    run.details = serde_json::to_string(&details)?;
    flow::save(db, &before, &run).await?;
    Ok(run)
}

#[cfg(test)]
mod original_tests {
    use super::*;

    pub(super) fn init_config() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
    }

    pub(super) async fn test_db() -> SqlitePool {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query("INSERT INTO users (id, username, created_at, updated_at) VALUES (1, 'rouge-test', 0, 0), (2, 'rouge-other', 0, 0)")
            .execute(&db).await.unwrap();
        db
    }

    pub(super) async fn insert_hero(db: &SqlitePool, user_id: i64, uid: i64, hero_id: i32) {
        sqlx::query(
            "INSERT INTO heroes (
                uid, user_id, hero_id, create_time, level, exp, rank, breakthrough,
                skin, faith, active_skill_level, ex_skill_level
             ) VALUES (?, ?, ?, 0, 1, 0, 1, 0, 0, 0, 1, 0)",
        )
        .bind(uid)
        .bind(user_id)
        .bind(hero_id)
        .execute(db)
        .await
        .unwrap();
    }

    async fn set_terminal_state(db: &SqlitePool, state: i32) {
        sqlx::query(
            "UPDATE user_rouge_run SET terminal_state = ? WHERE user_id = 1 AND season = 1",
        )
        .bind(state)
        .execute(db)
        .await
        .unwrap();
    }

    #[test]
    fn difficulty_unlock_chain_matches_official_progression() {
        init_config();
        let tables = config::configs::get();
        // 音序 1 is always open; 2..5 need 1 cleared; 6..10 need 5; 11+ are sequential.
        assert!(tables.rouge_difficulty_unlocked(1, &[]));
        assert!(!tables.rouge_difficulty_unlocked(2, &[]));
        assert!(tables.rouge_difficulty_unlocked(2, &[1]));
        assert!(tables.rouge_difficulty_unlocked(5, &[1]));
        assert!(!tables.rouge_difficulty_unlocked(6, &[1]));
        assert!(tables.rouge_difficulty_unlocked(6, &[1, 5]));
        assert!(!tables.rouge_difficulty_unlocked(11, &[1, 5]));
        assert!(tables.rouge_difficulty_unlocked(11, &[1, 5, 10]));
        assert!(!tables.rouge_difficulty_unlocked(20, &[1, 5, 10]));
        assert!(!tables.rouge_difficulty_unlocked(21, &[1, 5, 10]));
    }

    #[test]
    fn noise_cost_follows_the_shipped_rarity_table() {
        init_config();
        let tables = config::configs::get();
        // Data stores `rare` one below the star rating: 6★ -> 5, 5★ -> 4, <=4★ -> <=3.
        assert_eq!(tables.rouge_noise_cost(5), Some(4));
        assert_eq!(tables.rouge_noise_cost(4), Some(2));
        assert_eq!(tables.rouge_noise_cost(3), Some(1));
        assert_eq!(tables.rouge_noise_cost(2), Some(1));
        assert_eq!(tables.rouge_noise_cost(1), Some(1));
        assert_eq!(tables.rouge_team_size(), 8);
        assert_eq!(starting_power(tables, 1, 1, 1), (40, 40));
        assert_eq!(starting_power(tables, 1, 1, 8), (25, 40));
        assert_eq!(starting_power(tables, 1, 1, 13), (25, 25));
    }

    #[test]
    fn last_rewards_apply_their_next_run_bonus() {
        init_config();
        let tables = config::configs::get();
        let layout = tables.rouge_backpack(11011).unwrap();
        let fresh_bag =
            || bag::RunBag::from_layout(&layout.layout, &layout.initial_collection).unwrap();

        let mut coin = 50;
        let mut limit = 40;
        let mut bag = fresh_bag();
        apply_last_reward(tables, 21, 7, &mut coin, &mut limit, &mut bag).unwrap();
        assert_eq!(coin, 100);

        let mut coin = 50;
        let mut limit = 40;
        let mut bag = fresh_bag();
        let initial_len = bag.items.len();
        apply_last_reward(tables, 22, 7, &mut coin, &mut limit, &mut bag).unwrap();
        assert_eq!(bag.items.len(), initial_len + 1);
        assert_eq!(bag.items.last().unwrap().item_id, 8_130_804);

        for reward_id in [23, 24] {
            let mut coin = 50;
            let mut limit = 40;
            let mut bag = fresh_bag();
            apply_last_reward(tables, reward_id, 7, &mut coin, &mut limit, &mut bag).unwrap();
            assert_eq!(bag.items.len(), initial_len + 1);
            assert!(collection_shapes::get(bag.items.last().unwrap().item_id).is_some());
        }

        let mut coin = 50;
        let mut limit = 40;
        let mut bag = fresh_bag();
        apply_last_reward(tables, 25, 7, &mut coin, &mut limit, &mut bag).unwrap();
        assert_eq!(bag.items.len(), initial_len + 2);
        assert!(
            bag.items[initial_len..]
                .iter()
                .all(|item| collection_shapes::is_enchant_item(item.item_id))
        );

        apply_last_reward(tables, 36, 7, &mut coin, &mut limit, &mut bag).unwrap();
        assert_eq!(limit, 50);
    }

    #[tokio::test]
    async fn setup_persists_and_rejects_out_of_order_or_unsupported_requests() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        assert_eq!(
            player
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap()
                .state,
            Some(0)
        );
        assert!(player.select_style(&db, 1, 1).await.is_err());
        assert!(
            player
                .select_difficulty(&db, 1, 2, &[], false)
                .await
                .is_err()
        );
        assert!(
            player
                .select_difficulty(&db, 1, 1, &[101], false)
                .await
                .is_err()
        );
        assert!(
            player
                .select_difficulty(&db, 1, 1, &[], true)
                .await
                .is_err()
        );
        assert_eq!(
            player
                .select_difficulty(&db, 1, 1, &[], false)
                .await
                .unwrap()
                .state,
            Some(1)
        );
        assert!(
            player
                .select_difficulty(&db, 1, 1, &[], false)
                .await
                .is_err()
        );
        // 9 is gated behind meta progression, 2 is available from the start.
        assert!(player.select_style(&db, 1, 9).await.is_err());
        assert!(player.select_style(&db, 1, 99).await.is_err());
        let info = player.select_style(&db, 1, 2).await.unwrap();
        assert_eq!(
            (
                info.state,
                info.style,
                info.team_size,
                info.coin,
                info.power
            ),
            (Some(3), Some(2), Some(20), Some(50), Some(50))
        );
        assert_eq!(
            RougeManager::new(1)
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap(),
            info
        );
        assert_eq!(
            RougeManager::new(2)
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap()
                .state,
            Some(0)
        );
        assert_eq!(player.abort_setup(&db, 1).await.unwrap().state, Some(0));
        assert!(player.abort_setup(&db, 1).await.is_err());
        assert_eq!(
            player
                .select_difficulty(&db, 1, 1, &[], false)
                .await
                .unwrap()
                .state,
            Some(1)
        );
    }

    #[tokio::test]
    async fn hero_selection_enforces_noise_budget_team_size_and_ownership() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);

        // Six 6★ (noise 4) and three 5★ (noise 2).
        for (uid, hero_id) in [3003, 3004, 3007, 3009, 3025, 3032].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        for (uid, hero_id) in [3010, 3011, 3016].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 100, hero_id).await;
        }

        assert!(player.select_heroes(&db, 1, &[3003], 0).await.is_err());
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        assert!(player.select_heroes(&db, 1, &[3003], 0).await.is_err());
        player.select_style(&db, 1, 1).await.unwrap();

        // Empty team, duplicate picks, unowned hero and too many companions are rejected.
        assert!(player.select_heroes(&db, 1, &[], 0).await.is_err());
        assert!(
            player
                .select_heroes(&db, 1, &[3003, 3003], 0)
                .await
                .is_err()
        );
        assert!(
            player
                .select_heroes(&db, 1, &[3003, 9999], 0)
                .await
                .is_err()
        );
        assert!(
            player
                .select_heroes(
                    &db,
                    1,
                    &[3003, 3004, 3007, 3009, 3025, 3032, 3010, 3011, 3016],
                    0
                )
                .await
                .is_err()
        );
        // Catalyst 1 has a noise budget of 15: four 6★ cost 16.
        assert!(
            player
                .select_heroes(&db, 1, &[3003, 3004, 3007, 3009], 0)
                .await
                .is_err()
        );

        // Three 6★ (12) plus one 5★ (2) fit exactly into 15.
        let info = player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        assert_eq!(info.state, Some(4));
        assert_eq!(info.difficulty, Some(1));
        assert_eq!(info.style, Some(1));
        let team = info.team_info.unwrap();
        assert_eq!(team.battle_hero_list.len(), 4);
        assert_eq!(team.battle_hero_list[0].hero_id, Some(3003));
        assert_eq!(team.battle_hero_list[0].index, Some(1));
        assert_eq!(team.hero_info_list.len(), 4);
        assert_eq!(team.hero_life_list.len(), 4);
        assert!(
            team.hero_life_list
                .iter()
                .all(|hero| hero.life == Some(1_000))
        );

        // Battle slots are one-based, validated against the exploration roster,
        // and survive a reconnect together with every hero's current life.
        let changed = player
            .group_change(
                &db,
                1,
                &[
                    RougeBattleHero {
                        index: Some(2),
                        hero_id: Some(3004),
                        ..Default::default()
                    },
                    RougeBattleHero {
                        index: Some(1),
                        hero_id: Some(3003),
                        support_hero_id: Some(3007),
                        support_hero_skill: Some(1),
                        ..Default::default()
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(changed.battle_hero_list[0].index, Some(1));
        assert_eq!(changed.battle_hero_list[0].support_hero_id, Some(3007));
        assert_eq!(changed.battle_hero_list[1].index, Some(2));
        let reconnected = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap()
            .team_info
            .unwrap();
        assert_eq!(reconnected.battle_hero_list, changed.battle_hero_list);
        assert_eq!(reconnected.hero_life_list, changed.hero_life_list);
        assert!(
            player
                .group_change(
                    &db,
                    1,
                    &[RougeBattleHero {
                        index: Some(0),
                        hero_id: Some(3003),
                        ..Default::default()
                    }],
                )
                .await
                .is_err()
        );
        assert!(
            player
                .group_change(
                    &db,
                    1,
                    &[RougeBattleHero {
                        index: Some(1),
                        hero_id: Some(3011),
                        ..Default::default()
                    }],
                )
                .await
                .is_err()
        );

        // Selection is not repeatable once the team is locked in.
        assert!(
            player
                .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
                .await
                .is_err()
        );

        let info = player.original_info(&db, 1).await.unwrap();
        assert_eq!(info.init_hero_ids, vec![3003, 3004, 3007, 3010]);
    }

    #[tokio::test]
    async fn starting_a_run_builds_a_walkable_map_and_moves_between_nodes() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let info = player.original_info(&db, 1).await.unwrap();
        let map = info
            .rouge_info
            .unwrap()
            .map_info
            .expect("run must expose a map");
        let layer = map.layer_info.expect("layer info");
        assert_eq!(layer.layer_id, Some(111));
        assert_eq!(layer.cur_stage, Some(0));
        assert!(layer.node_info.len() > 2);

        // The start node is the only node without predecessors.
        let starts = layer
            .node_info
            .iter()
            .filter(|node| node.last_node_list.is_empty())
            .count();
        assert_eq!(starts, 1);

        let current = layer.cur_node.unwrap();
        let start = layer
            .node_info
            .iter()
            .find(|node| node.node_id == Some(current))
            .unwrap();
        let target = start.next_node_list[0];

        let moved = player.round_move(&db, 1, 111, target).await.unwrap();
        let moved_layer = moved.layer_info.unwrap();
        assert_eq!(moved_layer.cur_node, Some(target));
        assert_eq!(moved_layer.cur_stage, Some(1));

        // The current encounter must be resolved before another move.
        let target_info = moved_layer
            .node_info
            .iter()
            .find(|node| node.node_id == Some(target))
            .unwrap();
        if let Some(next) = target_info.next_node_list.first() {
            assert!(player.round_move(&db, 1, 111, *next).await.is_err());
        }

        // Non-adjacent nodes and unknown layers are rejected.
        assert!(player.round_move(&db, 1, 111, 9999).await.is_err());
        assert!(player.round_move(&db, 1, 999, target).await.is_err());
        assert!(player.round_move(&db, 1, 111, current).await.is_err());
    }

    #[tokio::test]
    async fn final_battle_opens_the_middle_board_and_advances() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        // Resolve the intermediate encounters and walk to the final battle.
        let mut map = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap()
            .map_info
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let resolved: Vec<i32> = graph
            .nodes
            .iter()
            .filter(|node| node.event_id != 0 && !node.next.is_empty())
            .map(|node| node.node_id)
            .collect();
        rouge::update_run_finished(&db, 1, 1, &resolved)
            .await
            .unwrap();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 64, "map walk did not terminate");
            let layer = map.layer_info.clone().unwrap();
            let current = layer.cur_node.unwrap();
            let node = layer
                .node_info
                .iter()
                .find(|node| node.node_id == Some(current))
                .unwrap();
            if node.next_node_list.is_empty() {
                break;
            }
            let target = node.next_node_list[0];
            map = player.round_move(&db, 1, 111, target).await.unwrap();
        }

        // Reaching the boss does not skip its fight.
        assert_eq!(map.map_type, Some(1));
        let final_layer = map.layer_info.as_ref().unwrap();
        assert_eq!(final_layer.cur_stage, Some(4));

        // Battle settlement performs this state transition transactionally; use
        // the same target position here to continue testing the middle board.
        let board = middle_board(config::configs::get(), 111).unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let position = RougeRunPosition {
            map_type: MAP_TYPE_MIDDLE,
            middle_layer_id: board.id,
            position_index: 0,
            ..run_position(&run)
        };
        rouge::update_run_state(&db, 1, 1, &position).await.unwrap();
        map = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap()
            .map_info
            .unwrap();

        let middle = map.middle_layer_info.clone().expect("middle layer info");
        assert_eq!(map.map_type, Some(2));
        assert_eq!(middle.layer_id, Some(111));
        assert_eq!(middle.middle_layer_id, Some(111));
        let entry = middle.position_index.unwrap();

        // Cannot leave before reaching the exit, and unknown targets are rejected.
        assert!(
            player
                .leave_middle_layer(&db, 1, 111, 111, 121)
                .await
                .is_err()
        );

        // Movement uses piece indices (0 = entrance, -1 = exit), not path points.
        assert_eq!(entry, 0);
        assert!(player.piece_move(&db, 1, 111, 111, 999).await.is_err());
        player.piece_move(&db, 1, 111, 111, 1).await.unwrap();
        player.piece_move(&db, 1, 111, 111, -1).await.unwrap();

        // Unknown next layer is rejected; the real one advances and regenerates the map.
        assert!(
            player
                .leave_middle_layer(&db, 1, 111, 111, 999)
                .await
                .is_err()
        );
        let next = player
            .leave_middle_layer(&db, 1, 111, 111, 121)
            .await
            .unwrap();
        assert_eq!(next.map_type, Some(1));
        let layer = next.layer_info.unwrap();
        assert_eq!(layer.layer_id, Some(121));
        assert_eq!(layer.cur_stage, Some(0));
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(run.step_count, resolved.len() as i32);
        assert!(run.minor_count + run.danger_count + run.crisis_count > 0);
    }

    #[tokio::test]
    async fn choice_events_accept_only_options_of_the_current_node() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let tables = config::configs::get();
        let info = player.original_info(&db, 1).await.unwrap();
        let layer = info
            .rouge_info
            .unwrap()
            .map_info
            .unwrap()
            .layer_info
            .unwrap();
        let nodes = layer.node_info.clone();
        let start = nodes
            .iter()
            .find(|node| node.last_node_list.is_empty())
            .unwrap()
            .node_id
            .unwrap();

        // BFS from the start until an interactive node is reached.
        let mut parents: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        parents.insert(start, start);
        let mut target = None;
        while let Some(current) = queue.pop_front() {
            let node = nodes
                .iter()
                .find(|node| node.node_id == Some(current))
                .unwrap();
            if current != start
                && !tables
                    .rouge_choices_for_event(node.event_id.unwrap_or(0))
                    .is_empty()
            {
                target = Some((current, node.event_id.unwrap()));
                break;
            }
            for next in &node.next_node_list {
                if !parents.contains_key(next) {
                    parents.insert(*next, current);
                    queue.push_back(*next);
                }
            }
        }
        let (target_node, target_event) = target.expect("map must contain an interactive node");

        // Walk there.
        let mut path = vec![target_node];
        while let Some(prev) = parents.get(path.last().unwrap()).copied() {
            if prev == *path.last().unwrap() {
                break;
            }
            path.push(prev);
        }
        path.reverse();
        let resolved = path[1..path.len().saturating_sub(1)].to_vec();
        rouge::update_run_finished(&db, 1, 1, &resolved)
            .await
            .unwrap();
        for step in path.into_iter().skip(1) {
            player.round_move(&db, 1, 111, step).await.unwrap();
        }

        let choices = tables.rouge_choices_for_event(target_event);
        assert!(!choices.is_empty());
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let choice_id = choices
            .iter()
            .find(|choice| choice.version.is_empty() && choice_is_unlocked(&run, choice))
            .expect("event must expose an unlocked base-game option")
            .id;
        // One past the last shipped option: inside the event's id block but not a real row.
        let foreign_choice = choices.last().unwrap().id + 1;

        // Wrong event id, a foreign option and a non-current node are rejected.
        assert!(
            player
                .choice_event(&db, 1, 111, target_node, target_event + 1, choice_id)
                .await
                .is_err()
        );
        assert!(
            player
                .choice_event(&db, 1, 111, target_node, target_event, foreign_choice)
                .await
                .is_err()
        );
        assert!(
            player
                .choice_event(&db, 1, 111, start, target_event, choice_id)
                .await
                .is_err()
        );

        let map = player
            .choice_event(&db, 1, 111, target_node, target_event, choice_id)
            .await
            .unwrap();
        assert_eq!(map.map_type, Some(1));
        let event_data = map
            .layer_info
            .as_ref()
            .unwrap()
            .node_info
            .iter()
            .find(|node| node.node_id == Some(target_node))
            .unwrap()
            .event_data
            .as_deref()
            .unwrap();
        let event_data: serde_json::Value = serde_json::from_str(event_data).unwrap();
        assert_eq!(event_data["state"], 2);
        assert_eq!(event_data["choiceSelect"], choice_id);
        assert!(
            event_data["canChoiceSet"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == choice_id)
        );

        // The option is recorded and cannot be taken twice.
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let chosen: HashMap<String, i32> = serde_json::from_str(&run.choices).unwrap();
        assert_eq!(chosen.get(&target_node.to_string()), Some(&choice_id));
        assert!(
            player
                .choice_event(&db, 1, 111, target_node, target_event, choice_id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn entrust_collect_progress_and_reward_wait_for_next_safe_zone() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let state = MiddleState {
            pieces: Vec::new(),
            entrust_id: 1110207,
            entrust_progress: 0,
            completed_entrusts: Vec::new(),
            middle_layer_id: 111,
        };
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.event_id = 101104;
        node.node_type = map::NODE_MINOR_OBSTACLE;
        // Keep this node non-terminal while accumulating collection progress.
        sqlx::query("UPDATE user_rouge_run SET graph = ?, middle_state = ? WHERE user_id = 1 AND season = 1")
            .bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&state).unwrap()).execute(&db).await.unwrap();
        for count in 1..=4 {
            sqlx::query("UPDATE user_rouge_run SET battle_interactive = '12#1001', interactive_index = 0 WHERE user_id = 1 AND season = 1")
                .execute(&db).await.unwrap();
            let (map, _, _, _) = player.select_drop(&db, 1, &[0]).await.unwrap();
            assert_eq!(map.rouge_entrust.unwrap().count, Some(count));
            assert!(player.select_drop(&db, 1, &[0]).await.is_err());
        }
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let map = map_info(&run);
        assert!(
            map.cur_interactive.is_none(),
            "reward must wait for the safe zone"
        );
        assert_eq!(map.rouge_entrust.unwrap().count, Some(4));
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap()
            .next
            .clear();
        sqlx::query(
            "UPDATE user_rouge_run SET graph = ?, finished = ? WHERE user_id = 1 AND season = 1",
        )
        .bind(serde_json::to_string(&graph).unwrap())
        .bind(serde_json::to_string(&vec![run.node_id]).unwrap())
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("UPDATE user_rouge_run SET battle_interactive = '5', interactive_index = 0 WHERE user_id = 1 AND season = 1").execute(&db).await.unwrap();
        let (_, map) = player.recruit_heroes(&db, 1, &[]).await.unwrap();
        assert_eq!(map.map_type, Some(MAP_TYPE_MIDDLE));
        assert_eq!(map.cur_interactive.as_deref(), Some("1#11110015"));
        assert!(map.rouge_entrust.is_none());
        assert!(player.piece_move(&db, 1, 111, 111, -1).await.is_err());
        let (map, ids, items, layouts) = player.select_drop(&db, 1, &[0]).await.unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(items.len() + layouts.len(), 1);
        assert!(map.cur_interactive.is_none());
        assert!(player.select_drop(&db, 1, &[0]).await.is_err());
        set_terminal_state(&db, 2).await;
        let (_, result) = player.end(&db, 1).await.unwrap();
        assert_eq!(result.finish_entrust_id, vec![1110207]);
        assert_eq!(result.entrust2_score.as_deref(), Some("1#160"));
        let outside = player.original_outside_info(&db, 1).await.unwrap();
        assert!(
            outside
                .rouge_info
                .unwrap()
                .game_record_info
                .unwrap()
                .pass_entrust_id
                .contains(&1110207)
        );
    }

    #[tokio::test]
    async fn entrust_progress_respects_configured_event_and_resource_types() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        for (id, change, expected) in [
            (
                1110201,
                EntrustChange {
                    gained_coin: 50,
                    spent_coin: 20,
                    ..Default::default()
                },
                50,
            ),
            (
                1110202,
                EntrustChange {
                    gained_coin: 50,
                    spent_coin: 20,
                    ..Default::default()
                },
                20,
            ),
            (
                1110203,
                EntrustChange {
                    entered_type: Some(2),
                    ..Default::default()
                },
                1,
            ),
            (
                1110203,
                EntrustChange {
                    entered_type: Some(1),
                    ..Default::default()
                },
                0,
            ),
            (
                1110205,
                EntrustChange {
                    spent_power: 12,
                    gained_power: 8,
                    ..Default::default()
                },
                12,
            ),
            (
                1110206,
                EntrustChange {
                    spent_power: 12,
                    gained_power: 8,
                    ..Default::default()
                },
                8,
            ),
        ] {
            run.middle_state = serde_json::to_string(&MiddleState {
                pieces: Vec::new(),
                entrust_id: id,
                entrust_progress: 0,
                completed_entrusts: Vec::new(),
                middle_layer_id: 111,
            })
            .unwrap();
            let state = updated_entrust(&run, change).unwrap();
            assert_eq!(
                serde_json::from_str::<MiddleState>(&state)
                    .unwrap()
                    .entrust_progress,
                expected
            );
        }
    }

    #[tokio::test]
    async fn battle_reward_recruit_and_middle_dialogue_survive_reconnect() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010, 3011].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.event_id = 101104;
        node.node_type = map::NODE_MINOR_OBSTACLE;
        node.next.clear();
        sqlx::query("UPDATE user_rouge_run SET graph = ?, finished = ?, team_exp = 8, battle_interactive = '12#1001', interactive_index = 0 WHERE user_id = 1 AND season = 1")
            .bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&vec![run.node_id]).unwrap())
            .execute(&db).await.unwrap();
        let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let map = map_info(&before);
        assert_eq!(map.cur_interactive.as_deref(), Some("12#1001"));
        let event_state = |map: &RougeMapInfo| -> i64 {
            let node = map
                .layer_info
                .as_ref()
                .unwrap()
                .node_info
                .iter()
                .find(|node| node.node_id == Some(run.node_id))
                .unwrap();
            serde_json::from_str::<serde_json::Value>(node.event_data.as_deref().unwrap()).unwrap()
                ["state"]
                .as_i64()
                .unwrap()
        };
        assert_eq!(event_state(&map), 1);
        let offer: serde_json::Value =
            serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
        assert_eq!(offer["dropCollectList"].as_array().unwrap().len(), 3);
        assert!(player.select_drop(&db, 1, &[99]).await.is_err());
        let (map, _, _, _) = player.select_drop(&db, 1, &[0]).await.unwrap();
        assert_eq!(map.cur_interactive.as_deref(), Some("5"));
        assert_eq!(event_state(&map), 2);
        let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(after.coin, before.coin + 50);
        assert_eq!(after.team_exp, before.team_exp + 12);
        assert_eq!(team_progress(1, 28), (2, 8));
        assert_eq!(team_progress(1, 60), (3, 0));
        assert!(player.select_drop(&db, 1, &[0]).await.is_err());
        assert!(player.recruit_heroes(&db, 1, &[3003]).await.is_err());
        assert!(player.recruit_heroes(&db, 1, &[9999]).await.is_err());
        let (team, map) = player.recruit_heroes(&db, 1, &[3011]).await.unwrap();
        assert!(
            team.hero_life_list
                .iter()
                .any(|life| life.hero_id == Some(3011) && life.life == Some(1000))
        );
        assert_eq!(map.middle_layer_info.unwrap().position_index, Some(0));
        assert!(player.recruit_heroes(&db, 1, &[3011]).await.is_err());
        player
            .group_change(
                &db,
                1,
                &[RougeBattleHero {
                    index: Some(1),
                    hero_id: Some(3011),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let state = middle_state(&run);
        let quest = state
            .pieces
            .iter()
            .find(|piece| {
                config::configs::get()
                    .rouge_piece
                    .get(piece.id.unwrap())
                    .unwrap()
                    .entrust_type
                    == 2
            })
            .unwrap();
        let index = quest.index.unwrap();
        let talk = config::configs::get()
            .rouge_piece_talk
            .get(quest.talk_id.unwrap())
            .unwrap();
        let selection: i32 = talk.select_ids.split('|').next().unwrap().parse().unwrap();
        player.piece_move(&db, 1, 111, 111, index).await.unwrap();
        let map = player
            .piece_talk_select(&db, 1, 111, 111, index, selection)
            .await
            .unwrap();
        assert!(map.rouge_entrust.is_some());
        assert!(
            player
                .piece_talk_select(&db, 1, 111, 111, index, selection)
                .await
                .is_err()
        );
        let map = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap()
            .map_info
            .unwrap();
        assert!(map.rouge_entrust.is_some());
        player.piece_move(&db, 1, 111, 111, -1).await.unwrap();
        let map = player
            .leave_middle_layer(&db, 1, 111, 111, 122)
            .await
            .unwrap();
        assert_eq!(map.layer_info.unwrap().layer_id, Some(122));
    }

    #[tokio::test]
    async fn old_run_life_values_migrate_to_per_mille() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut team = persisted_team(&run).unwrap();
        for (life, value) in team
            .hero_life_list
            .iter_mut()
            .zip([100_000, 25_000, 100, 0])
        {
            life.life = Some(value);
        }
        rouge::update_run_team(&db, 1, 1, &serde_json::to_string(&team).unwrap())
            .await
            .unwrap();
        sqlx::query(include_str!(
            "../../../database/migrations/115_rouge_life_scale.sql"
        ))
        .execute(&db)
        .await
        .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let team = persisted_team(&run).unwrap();
        assert_eq!(
            team.hero_life_list
                .iter()
                .map(|life| life.life.unwrap())
                .collect::<Vec<_>>(),
            vec![1000, 250, 1, 0]
        );
    }

    #[tokio::test]
    async fn verified_resource_choices_apply_once_and_count_actual_recovery() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        for (choice, quest, expected_coin, expected_power, expected_exp, expected_progress) in [
            (10610501, 1110206, 50, 40, 0, 20),
            (10613801, 1110206, 50, 35, 0, 15),
            (10613902, 1110201, 50, 20, 20, 0),
            (10614003, 1110201, 150, 20, 0, 100),
        ] {
            let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
            let event_id = config::configs::get()
                .rouge_event
                .iter()
                .find(|event| {
                    config::configs::get()
                        .rouge_choices_for_event(event.id)
                        .iter()
                        .any(|row| row.id == choice)
                })
                .unwrap()
                .id;
            let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.node_id == run.node_id)
                .unwrap();
            node.node_type = map::NODE_DISCOVERY;
            node.event_id = event_id;
            let state = MiddleState {
                pieces: Vec::new(),
                entrust_id: quest,
                entrust_progress: 0,
                completed_entrusts: Vec::new(),
                middle_layer_id: 111,
            };
            sqlx::query("UPDATE user_rouge_run SET graph = ?, choices = '{}', coin = 50, power = 20, power_limit = 40, team_exp = 0, middle_state = ?, battle_interactive = '', interactive_index = -1 WHERE user_id = 1 AND season = 1")
                .bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&state).unwrap()).execute(&db).await.unwrap();
            assert!(
                player
                    .choice_event(&db, 1, 111, run.node_id, event_id + 1, choice)
                    .await
                    .is_err()
            );
            let map = player
                .choice_event(&db, 1, 111, run.node_id, event_id, choice)
                .await
                .unwrap();
            assert_eq!(
                map.cur_interactive.as_deref(),
                (expected_exp >= 20).then_some("5")
            );
            assert_eq!(map.rouge_entrust.unwrap().count, Some(expected_progress));
            assert!(
                player
                    .choice_event(&db, 1, 111, run.node_id, event_id, choice)
                    .await
                    .is_err()
            );
            let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
            assert_eq!(
                (after.coin, after.power, after.team_exp),
                (expected_coin, expected_power, expected_exp)
            );
        }
    }

    #[tokio::test]
    async fn safe_zone_delivers_resource_rewards_once_and_preserves_unsupported_effects() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap()
            .next
            .clear();
        for (quest, progress, expected_resources, completed, retained) in [
            (1110201, 200, (150, 40, 40), true, false),
            (1210202, 250, (200, 40, 40), true, false),
            (1110205, 30, (50, 40, 40), true, false),
            (1110206, 25, (50, 40, 65), true, false),
            (1110207, 3, (50, 40, 40), false, false),
            (1110231, 1, (50, 40, 40), false, true),
        ] {
            let state = MiddleState {
                pieces: Vec::new(),
                entrust_id: quest,
                entrust_progress: progress,
                completed_entrusts: Vec::new(),
                middle_layer_id: 111,
            };
            sqlx::query("UPDATE user_rouge_run SET graph = ?, middle_state = ?, map_type = 1, middle_layer_id = 0, coin = 50, power = 10, power_limit = 40, battle_interactive = '5', interactive_index = 0 WHERE user_id = 1 AND season = 1")
                .bind(serde_json::to_string(&graph).unwrap()).bind(serde_json::to_string(&state).unwrap()).execute(&db).await.unwrap();
            let (_, map) = player.recruit_heroes(&db, 1, &[]).await.unwrap();
            assert_eq!(map.map_type, Some(MAP_TYPE_MIDDLE));
            assert!(player.recruit_heroes(&db, 1, &[]).await.is_err());
            let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
            assert_eq!(
                (after.coin, after.power, after.power_limit),
                expected_resources
            );
            let state: MiddleState = serde_json::from_str(&after.middle_state).unwrap();
            assert_eq!(state.completed_entrusts.contains(&quest), completed);
            assert_eq!(state.entrust_id == quest, retained);
        }
    }

    #[tokio::test]
    async fn captured_rest_choice_heals_without_a_followup_request() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.node_type = map::NODE_REST;
        node.event_id = 106103;
        let mut team = persisted_team(&run).unwrap();
        team.hero_life_list[0].life = Some(200);
        team.hero_life_list[1].life = Some(0);
        sqlx::query(
            "UPDATE user_rouge_run SET graph = ?, team = ? WHERE user_id = 1 AND season = 1",
        )
        .bind(serde_json::to_string(&graph).unwrap())
        .bind(serde_json::to_string(&team).unwrap())
        .execute(&db)
        .await
        .unwrap();
        let map = player
            .choice_event(&db, 1, 111, run.node_id, 106103, 10610301)
            .await
            .unwrap();
        assert!(map.cur_interactive.is_none());
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let team = persisted_team(&run).unwrap();
        assert_eq!(team.hero_life_list[0].life, Some(700));
        assert_eq!(team.hero_life_list[1].life, Some(0));
        assert!(
            player
                .choice_event(&db, 1, 111, run.node_id, 106103, 10610301)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn captured_non_unique_loss_is_checked_and_consumed_once() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.node_type = map::NODE_REST;
        node.event_id = 106144;
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        let item = bag.add(8110021).unwrap();
        bag.place(item.uid, -1, -1, 0).unwrap();
        let unique = bag.add(8170001).unwrap();
        sqlx::query(
            "UPDATE user_rouge_run SET graph = ?, bag = ? WHERE user_id = 1 AND season = 1",
        )
        .bind(serde_json::to_string(&graph).unwrap())
        .bind(serde_json::to_string(&bag).unwrap())
        .execute(&db)
        .await
        .unwrap();
        let map = player
            .choice_event(&db, 1, 111, run.node_id, 106144, 10614401)
            .await
            .unwrap();
        assert_eq!(map.cur_interactive.as_deref(), Some("9#1"));
        let data: serde_json::Value =
            serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
        assert_eq!(data["lostNum"], 1);
        assert!(
            player
                .select_lost_collection(&db, 1, &[unique.uid])
                .await
                .is_err()
        );
        assert!(
            player
                .select_lost_collection(&db, 1, &[item.uid, item.uid])
                .await
                .is_err()
        );
        assert!(
            player
                .select_lost_collection(&db, 1, &[99999])
                .await
                .is_err()
        );
        let (map, placed, warehouse, _) = player
            .select_lost_collection(&db, 1, &[item.uid])
            .await
            .unwrap();
        assert!(map.cur_interactive.is_none());
        assert_eq!(placed, vec![item.uid]);
        assert!(warehouse.is_empty());
        assert!(
            player
                .select_lost_collection(&db, 1, &[item.uid])
                .await
                .is_err()
        );
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        assert!(bag.item(item.uid).is_none());
        assert!(bag.item(unique.uid).is_some());
    }

    #[tokio::test]
    async fn revive_and_heal_interactions_are_validated_consumed_and_chained() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        // Put the current node on the shipped "revive one, then heal all 10%"
        // choice so the real choice validation and interaction pipeline are used.
        let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let current = graph.node(run.node_id).unwrap().clone();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.node_type = map::NODE_REST;
        node.event_id = 108101;
        run.graph = serde_json::to_string(&graph).unwrap();
        sqlx::query("UPDATE user_rouge_run SET graph = ? WHERE user_id = 1 AND season = 1")
            .bind(&run.graph)
            .execute(&db)
            .await
            .unwrap();

        // DeadHeroNum(1) is a server-side gate too; forging the locked option
        // while every companion is alive must not record the choice.
        assert!(
            player
                .choice_event(&db, 1, 111, run.node_id, 108101, 10810102)
                .await
                .is_err()
        );

        let mut team = persisted_team(&run).unwrap();
        for life in &mut team.hero_life_list {
            life.life = Some(if life.hero_id == Some(3003) { 0 } else { 200 });
        }
        let encoded = serde_json::to_string(&team).unwrap();
        assert!(rouge::update_run_team(&db, 1, 1, &encoded).await.unwrap());

        let map = player
            .choice_event(&db, 1, 111, run.node_id, 108101, 10810102)
            .await
            .unwrap();
        assert_eq!(map.cur_interactive_index, Some(0));
        assert_eq!(map.cur_interactive.as_deref(), Some("4#403"));
        let interactive: serde_json::Value =
            serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
        assert_eq!(interactive["reviveNum"], 1);

        // A pending interaction blocks map movement and cannot be answered by a
        // different command, an alive hero, an unknown hero or duplicate ids.
        assert!(
            player
                .round_move(&db, 1, 111, current.next[0])
                .await
                .is_err()
        );
        assert!(player.select_heal(&db, 1, &[3004]).await.is_err());
        assert!(player.select_revive(&db, 1, &[3004]).await.is_err());
        assert!(player.select_revive(&db, 1, &[9999]).await.is_err());
        assert!(player.select_revive(&db, 1, &[3003, 3003]).await.is_err());

        let (team, map) = player.select_revive(&db, 1, &[3003]).await.unwrap();
        assert_eq!(
            team.hero_life_list
                .iter()
                .find(|life| life.hero_id == Some(3003))
                .unwrap()
                .life,
            Some(1_000)
        );
        assert_eq!(map.cur_interactive_index, Some(1));
        assert_eq!(map.cur_interactive.as_deref(), Some("3#304"));
        let interactive: serde_json::Value =
            serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
        assert_eq!(interactive["healNum"], 99);
        assert_eq!(interactive["healRate"], 100);
        assert!(player.select_revive(&db, 1, &[3003]).await.is_err());

        // Type 1 healing is an all-companion effect. The selected id only
        // acknowledges the UI request; every living companion receives 10% HP.
        let (team, map) = player.select_heal(&db, 1, &[3004]).await.unwrap();
        assert!(map.cur_interactive_index.is_none());
        assert!(map.cur_interactive.is_none());
        for life in &team.hero_life_list {
            let expected = if life.hero_id == Some(3003) {
                1_000
            } else {
                300
            };
            assert_eq!(life.life, Some(expected));
        }
        assert!(player.select_heal(&db, 1, &[3004]).await.is_err());

        // Completion survives reconnect and unlocks movement off the node.
        let info = player.original_info(&db, 1).await.unwrap();
        assert!(
            info.rouge_info
                .unwrap()
                .team_info
                .unwrap()
                .hero_life_list
                .iter()
                .any(|life| life.hero_id == Some(3004) && life.life == Some(300))
        );
        assert!(
            player
                .round_move(&db, 1, 111, current.next[0])
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn collection_drop_positions_are_validated_and_claimed_once() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        insert_hero(&db, 1, 1, 3003).await;
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player.select_heroes(&db, 1, &[3003], 0).await.unwrap();
        let mut run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        // Use a genuine multi-candidate battle reward. Single fixed-size event
        // grants are automatic in 3.9 and have no SelectDrop request.
        run.battle_interactive = "12#1001".to_owned();
        run.interactive_index = 0;
        let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        flow::save(&db, &before, &run).await.unwrap();
        let map = map_info(&run);
        let interactive: serde_json::Value =
            serde_json::from_str(map.interactive_json.as_deref().unwrap()).unwrap();
        let offers = interactive["dropCollectList"].as_array().unwrap();
        assert_eq!(offers.len(), 3);
        let offered_id = offers[0].as_i64().unwrap() as i32;
        for bad in [vec![], vec![-1], vec![3], vec![0, 0], vec![0, 1]] {
            assert!(player.select_drop(&db, 1, &bad).await.is_err());
        }
        let (map, ids, items, layouts) = player.select_drop(&db, 1, &[0]).await.unwrap();
        assert_eq!(ids, vec![offered_id]);
        assert_eq!(items.len() + layouts.len(), 1);
        assert!(map.cur_interactive.is_none());
        assert!(player.select_drop(&db, 1, &[0]).await.is_err());
        let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!((after.coin, after.team_exp), (run.coin + 50, 12));
    }

    #[test]
    fn generated_battle_nodes_carry_a_real_fight_episode() {
        init_config();
        let tables = config::configs::get();
        for seed in 0..16u64 {
            let map = map::generate(111, map::DEFAULT_STAGES, seed, &event_pool(tables));
            for node in &map.nodes {
                if node.event_id != 0 {
                    let event = tables.rouge_event(node.event_id).unwrap();
                    assert!(
                        event.version.is_empty(),
                        "base run generated DLC event {event:?}"
                    );
                }
                if (1..=3).contains(&node.node_type) {
                    assert!(
                        tables.rouge_fight_episode(node.event_id).is_some(),
                        "battle node {node:?} has no fight episode"
                    );
                }
            }
        }
    }

    #[test]
    fn every_catalyst_resolves_its_backpack_layout() {
        init_config();
        let tables = config::configs::get();
        for style in tables.rouge_style.iter() {
            let layout = tables
                .rouge_backpack(style.layout_id)
                .unwrap_or_else(|| panic!("style {} has no backpack", style.id));
            assert_eq!(layout.layout, "8#8");
            assert!(
                bag::RunBag::from_layout(&layout.layout, &layout.initial_collection).is_ok(),
                "style {} backpack does not parse",
                style.id
            );
        }
    }

    #[tokio::test]
    async fn run_starts_with_the_catalyst_backpack() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        // Catalyst 1 -> layout 11011 -> starting artefact 8230024 at (3, 3).
        player.select_style(&db, 1, 1).await.unwrap();
        let info = player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let bag = info.bag.expect("run must expose a bag");
        assert_eq!(bag.layouts.len(), 1);
        let layout = &bag.layouts[0];
        assert_eq!(layout.item.as_ref().unwrap().item_id, Some(8230024));
        assert_eq!(layout.pos.as_ref().unwrap().row, Some(3));
        assert_eq!(layout.pos.as_ref().unwrap().col, Some(3));

        // The bag survives a reconnect.
        let info = player.original_info(&db, 1).await.unwrap();
        let bag = info.rouge_info.unwrap().bag.unwrap();
        assert_eq!(bag.layouts.len(), 1);
        assert_eq!(bag.layouts[0].item.as_ref().unwrap().item_id, Some(8230024));
    }

    #[tokio::test]
    async fn bar_placement_persists_between_board_and_warehouse() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let removed = player.remove_from_bar(&db, 1, 1).await.unwrap();
        assert_eq!(removed.item_id, Some(8230024));
        let info = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        assert!(info.bag.unwrap().layouts.is_empty());
        assert_eq!(info.warehouse.unwrap().items.len(), 1);

        let (layout, from_warehouse) = player.add_to_bar(&db, 1, 1, 0, 0, 1).await.unwrap();
        assert!(from_warehouse);
        assert_eq!(layout.rotation, Some(1));
        let info = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        let layout = &info.bag.unwrap().layouts[0];
        assert_eq!(layout.pos.as_ref().unwrap().row, Some(0));
        assert_eq!(layout.pos.as_ref().unwrap().col, Some(0));
        assert_eq!(layout.rotation, Some(1));
        assert!(info.warehouse.unwrap().items.is_empty());
    }

    #[tokio::test]
    async fn inlay_and_demount_manage_artefact_slots() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        // Style 3 starts with collection 8310206, which has two editor-defined holes.
        player.select_style(&db, 1, 3).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut run_bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        let enchant1 = run_bag.add(8140101).unwrap();
        let enchant2 = run_bag.add(8140201).unwrap();
        rouge::update_run_bag(&db, 1, 1, &serde_json::to_string(&run_bag).unwrap())
            .await
            .unwrap();

        let info = player.original_info(&db, 1).await.unwrap();
        let info = info.rouge_info.unwrap();
        let bag = info.bag.unwrap();
        assert_eq!(info.warehouse.unwrap().items.len(), 2);
        let uid = bag.layouts[0].item.as_ref().unwrap().id.unwrap();

        // Unknown artefact, unknown enchant and out-of-range slot are rejected.
        assert!(player.inlay(&db, 1, 999, 0, enchant1.uid).await.is_err());
        assert!(player.inlay(&db, 1, uid, 0, 999_999).await.is_err());
        assert!(player.inlay(&db, 1, uid, 2, enchant1.uid).await.is_err());

        let (post, pre, removed, removed_from_bag, returned) =
            player.inlay(&db, 1, uid, 0, enchant1.uid).await.unwrap();
        assert!(pre.is_none());
        assert!(removed);
        assert!(!removed_from_bag);
        assert!(returned.is_none());
        assert_eq!(post.hold_items[0], enchant1.item_id);
        assert_eq!(post.hold_ids[0], enchant1.uid);
        assert_eq!(post.hold_items.len(), 2);

        // Second slot is independent.
        let (post2, _, removed, removed_from_bag, _) =
            player.inlay(&db, 1, uid, 1, enchant2.uid).await.unwrap();
        assert!(removed);
        assert!(!removed_from_bag);
        assert_eq!(post2.hold_items, vec![enchant1.item_id, enchant2.item_id]);
        assert_eq!(post2.hold_ids, vec![enchant1.uid, enchant2.uid]);

        // Both instances leave the warehouse and the inlay survives a reconnect.
        let info = player.original_info(&db, 1).await.unwrap();
        let info = info.rouge_info.unwrap();
        assert!(info.warehouse.unwrap().items.is_empty());
        let bag = info.bag.unwrap();
        assert_eq!(
            bag.layouts[0].item.as_ref().unwrap().hold_ids[0],
            enchant1.uid
        );

        let (after, demounted) = player.demount(&db, 1, uid, 0).await.unwrap();
        assert_eq!(after.hold_items[0], 0);
        assert_eq!(after.hold_items[1], enchant2.item_id);
        assert_eq!(demounted.id, Some(enchant1.uid));
        assert_eq!(demounted.item_id, Some(enchant1.item_id));
        let info = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        assert_eq!(info.warehouse.unwrap().items[0].id, Some(enchant1.uid));

        // The enchant view also lists Ritual Disks already placed on the grid.
        // Such a disk is consumed from the bag area, rather than the warehouse.
        let (_, from_warehouse) = player
            .add_to_bar(&db, 1, enchant1.uid, -1, -1, 0)
            .await
            .unwrap();
        assert!(from_warehouse);
        let (after, _, removed_from_warehouse, removed_from_bag, _) =
            player.inlay(&db, 1, uid, 0, enchant1.uid).await.unwrap();
        assert!(!removed_from_warehouse);
        assert!(removed_from_bag);
        assert_eq!(after.hold_ids[0], enchant1.uid);
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let run_bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        assert!(run_bag.items.iter().all(|item| item.uid != enchant1.uid));

        // Demounting an empty slot is rejected.
        player.demount(&db, 1, uid, 0).await.unwrap();
        assert!(player.demount(&db, 1, uid, 0).await.is_err());
        assert!(player.demount(&db, 1, uid, 5).await.is_err());
    }

    #[tokio::test]
    async fn ending_a_run_scores_it_and_unlocks_the_next_difficulty() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        // A live run cannot be settled by forging RougeEnd.
        assert!(player.end(&db, 1).await.is_err());
        sqlx::query("UPDATE user_rouge_run SET coin = 456, spent_coin = 123 WHERE user_id = 1 AND season = 1")
            .execute(&db)
            .await
            .unwrap();
        set_terminal_state(&db, 1).await;

        // A cleared run receives the starting artefact (20), first-layer score
        // (750), and ending score (400), at difficulty 1 (100% efficiency).
        // NOTE: the wiki lists 25 per artefact, but `rouge_const` 1014 ships 20;
        // the data wins.
        let (info, result) = player.end(&db, 1).await.unwrap();
        // EndReply preserves the run for the ending flow; the next GetInfo
        // returns the new-run bonus screen.
        assert_eq!(info.state, Some(5));
        assert!(info.map_info.is_some());
        assert_eq!(result.final_score, Some(1170));
        assert_eq!(result.collection2_score.as_deref(), Some("1#20"));
        assert_eq!(result.collection2_num_str.as_deref(), Some("8230024#1"));
        assert_eq!(result.layer2_score.as_deref(), Some("1#750"));
        assert_eq!(result.normal_fight2_score.as_deref(), Some("0#0"));
        assert_eq!(result.consume_coin, Some(123));
        assert_eq!(result.score_reward, Some(100));
        // 1170 / 700 = 1 bud, 470 carried; / 2500 = 0 token, 1170 carried.
        assert_eq!(result.add_genius_point, Some(1));
        assert_eq!(result.remain_score2_genius_point, Some(470));
        assert_eq!(result.add_point, Some(0));
        assert_eq!(result.remain_score2_point, Some(1170));

        // The run is gone; the reward screen is up.
        let info = player.original_info(&db, 1).await.unwrap();
        let pending = info.rouge_info.unwrap();
        assert_eq!(pending.state, Some(2));
        assert!(pending.map_info.is_none());

        // Claiming a reward that was not offered is rejected.
        assert!(player.select_reward(&db, 1, &[999_999]).await.is_err());

        // Claiming an offered reward returns to the setup screen.
        let reward_id = pending.last_reward[0].id.unwrap();
        assert_eq!(reward_id, 21);
        assert!(
            player
                .select_reward(&db, 1, &[reward_id, reward_id])
                .await
                .is_err()
        );
        let info = player.select_reward(&db, 1, &[reward_id]).await.unwrap();
        assert_eq!(info.state, Some(0));
        assert!(info.last_reward.is_empty());
        assert!(player.select_reward(&db, 1, &[reward_id]).await.is_err());
        player
            .select_difficulty(&db, 1, 2, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        let next_run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(next_run.coin, 100);
        assert_eq!(next_run.power_limit, 40);
        assert_eq!(load_original(&db, 1, 1).await.unwrap().rewards, "[]");
    }

    #[tokio::test]
    async fn shop_nodes_roll_stock_and_sell_artefacts() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        // Walk to a shop node (type 7).
        let tables = config::configs::get();
        let info = player.original_info(&db, 1).await.unwrap();
        let layer = info
            .rouge_info
            .unwrap()
            .map_info
            .unwrap()
            .layer_info
            .unwrap();
        let nodes = layer.node_info.clone();
        let start = nodes
            .iter()
            .find(|node| node.last_node_list.is_empty())
            .unwrap()
            .node_id
            .unwrap();

        let mut parents: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        parents.insert(start, start);
        let mut target = None;
        while let Some(current) = queue.pop_front() {
            let node = nodes
                .iter()
                .find(|node| node.node_id == Some(current))
                .unwrap();
            if current != start
                && tables
                    .rouge_fight_episode(node.event_id.unwrap_or(0))
                    .is_none()
                && !tables
                    .rouge_choices_for_event(node.event_id.unwrap_or(0))
                    .is_empty()
                && is_shop_event(tables, node.event_id.unwrap_or(0))
            {
                target = Some((current, node.event_id.unwrap()));
                break;
            }
            for next in &node.next_node_list {
                if !parents.contains_key(next) {
                    parents.insert(*next, current);
                    queue.push_back(*next);
                }
            }
        }
        let (shop_node, shop_event) = match target {
            Some(found) => found,
            // No shop on this seed: nothing to assert.
            None => return,
        };

        let mut path = vec![shop_node];
        while let Some(prev) = parents.get(path.last().unwrap()).copied() {
            if prev == *path.last().unwrap() {
                break;
            }
            path.push(prev);
        }
        path.reverse();
        let resolved = path[1..path.len().saturating_sub(1)].to_vec();
        rouge::update_run_finished(&db, 1, 1, &resolved)
            .await
            .unwrap();
        for step in path.into_iter().skip(1) {
            player.round_move(&db, 1, 111, step).await.unwrap();
        }

        // Wrong node/event pairs are rejected.
        assert!(
            player
                .shop_refresh(&db, 1, 111, shop_node, shop_event + 1)
                .await
                .is_err()
        );

        player
            .shop_refresh(&db, 1, 111, shop_node, shop_event)
            .await
            .unwrap();
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let stock: shop::ShopState = serde_json::from_str(&run.shop).unwrap();
        assert_eq!(stock.stock.len(), shop::SHOP_SLOTS);

        // Make the run able to afford the cheapest offer.
        let cheapest = stock
            .stock
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                tables
                    .rouge_goods
                    .get(*id)
                    .map(|goods| (index as i32 + 1, goods.price))
            })
            .min_by_key(|(_, price)| *price);
        let Some((position, price)) = cheapest else {
            return;
        };
        rouge::update_run_resources(&db, 1, 1, price.max(1), run.power)
            .await
            .unwrap();
        let state = MiddleState {
            pieces: Vec::new(),
            entrust_id: 1110202,
            entrust_progress: 0,
            completed_entrusts: Vec::new(),
            middle_layer_id: 111,
        };
        sqlx::query("UPDATE user_rouge_run SET middle_state = ? WHERE user_id = 1 AND season = 1")
            .bind(serde_json::to_string(&state).unwrap())
            .execute(&db)
            .await
            .unwrap();
        let before = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let bag_before: bag::RunBag = serde_json::from_str(&before.bag).unwrap();

        let (map, added_items) = player
            .buy_goods(&db, 1, 111, shop_node, shop_event, &[position])
            .await
            .unwrap();
        assert_eq!(added_items.len(), 1);
        assert_eq!(map.map_type, Some(1));
        let shop_data = map
            .layer_info
            .as_ref()
            .unwrap()
            .node_info
            .iter()
            .find(|node| node.node_id == Some(shop_node))
            .unwrap()
            .event_data
            .as_deref()
            .unwrap();
        let shop_data: serde_json::Value = serde_json::from_str(shop_data).unwrap();
        assert!(
            shop_data["boughtGoodsPosSet"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(position))
        );
        assert!(shop_data["posGoodMap"].is_object());

        let after = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let bag_after: bag::RunBag = serde_json::from_str(&after.bag).unwrap();
        assert_eq!(bag_after.items.len(), bag_before.items.len() + 1);
        assert!(!bag::RunBag::is_placed(bag_after.items.last().unwrap()));
        assert_eq!(after.coin, before.coin - price);
        assert_eq!(
            serde_json::from_str::<MiddleState>(&after.middle_state)
                .unwrap()
                .entrust_progress,
            price
        );

        // The offer is sold out and cannot be bought twice.
        assert!(
            player
                .buy_goods(&db, 1, 111, shop_node, shop_event, &[position])
                .await
                .is_err()
        );
        // Unknown position is rejected.
        assert!(
            player
                .buy_goods(&db, 1, 111, shop_node, shop_event, &[99])
                .await
                .is_err()
        );

        let map = player
            .end_shop_event(&db, 1, 111, shop_node, shop_event)
            .await
            .unwrap();
        let event_data = map
            .layer_info
            .unwrap()
            .node_info
            .into_iter()
            .find(|node| node.node_id == Some(shop_node))
            .unwrap()
            .event_data
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&event_data).unwrap()["state"],
            2
        );
    }

    /// Shop events are the ones whose `rouge_event.type` is 7.
    fn is_shop_event(tables: &config::GameDB, event_id: i32) -> bool {
        tables
            .rouge_event(event_id)
            .is_some_and(|row| row.r#type == map::NODE_SHOP)
    }

    #[tokio::test]
    async fn genius_tree_spends_buds_and_requires_predecessors() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        // A failure after clearing area one still receives its layer score.
        // This fixture isolates spending the earned bud rather than map travel.
        sqlx::query("UPDATE user_rouge_run SET map_type = 2, middle_layer_id = 111 WHERE user_id = 1 AND season = 1")
            .execute(&db).await.unwrap();
        set_terminal_state(&db, 2).await;
        player.end(&db, 1).await.unwrap();
        assert!(
            rouge::cleared_difficulties(&db, 1, 1)
                .await
                .unwrap()
                .is_empty()
        );

        let outside = player.original_outside_info(&db, 1).await.unwrap();
        let outside = outside.rouge_info.unwrap();
        assert_eq!(outside.genius_point, Some(1));
        assert!(outside.genius_ids.is_empty());

        // Node 103 requires 102, which is not active yet.
        assert!(player.active_genius(&db, 1, 103).await.is_err());
        // Unknown node.
        assert!(player.active_genius(&db, 1, 999_999).await.is_err());

        // Root node 101 costs 1 bud.
        let ids = player.active_genius(&db, 1, 101).await.unwrap();
        assert_eq!(ids, vec![101]);

        // The only bud is spent.
        assert!(player.active_genius(&db, 1, 102).await.is_err());

        // Activating twice is rejected.
        assert!(player.active_genius(&db, 1, 101).await.is_err());

        let outside = player.original_outside_info(&db, 1).await.unwrap();
        let outside = outside.rouge_info.unwrap();
        assert_eq!(outside.genius_point, Some(0));
        assert_eq!(outside.genius_ids, vec![101]);
    }

    #[test]
    fn badges_follow_the_supported_triggers() {
        init_config();
        let tables = config::configs::get();
        // No battles, one artefact: only the 40-artefact badge is out of reach.
        assert!(earned_badges(tables, 0, 0, 0, 0, 0).is_empty());
        // 12 battles satisfies the "kick the door" badge (trigger 1, param 12).
        let badges = earned_badges(tables, 12, 0, 0, 0, 0);
        assert!(badges.iter().any(|badge| badge.trigger == 1));
        // 40 artefacts satisfies trigger 2.
        assert!(
            earned_badges(tables, 0, 40, 0, 0, 0)
                .iter()
                .any(|badge| badge.trigger == 2)
        );
        // 3 crisis battles satisfies trigger 8.
        assert!(
            earned_badges(tables, 0, 0, 3, 0, 0)
                .iter()
                .any(|badge| badge.trigger == 8)
        );
        // 1000 spent coins satisfies trigger 5.
        assert!(
            earned_badges(tables, 0, 0, 0, 1000, 0)
                .iter()
                .any(|badge| badge.trigger == 5)
        );
    }

    /// Walks the whole player loop in one go: setup -> map -> event -> settlement
    /// -> reward -> meta progression.
    #[tokio::test]
    async fn full_loop_from_setup_to_genius_activation() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }

        // 1. Setup.
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        let info = player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();
        assert_eq!(info.state, Some(4));

        // 2. Map is generated and walkable.
        let tables = config::configs::get();
        let layer = info.map_info.clone().unwrap().layer_info.unwrap();
        let nodes = layer.node_info.clone();
        let start = nodes
            .iter()
            .find(|node| node.last_node_list.is_empty())
            .unwrap()
            .node_id
            .unwrap();

        // 3. Resolve a choice event somewhere on the map.
        let mut parents: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        parents.insert(start, start);
        let mut choice_node = None;
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        while let Some(current) = queue.pop_front() {
            let node = nodes
                .iter()
                .find(|node| node.node_id == Some(current))
                .unwrap();
            if current != start {
                if let Some(choice) = tables
                    .rouge_choices_for_event(node.event_id.unwrap_or(0))
                    .into_iter()
                    .find(|choice| choice.version.is_empty() && choice_is_unlocked(&run, choice))
                {
                    choice_node = Some((current, node.event_id.unwrap(), choice.id));
                    break;
                }
            }
            for next in &node.next_node_list {
                if !parents.contains_key(next) {
                    parents.insert(*next, current);
                    queue.push_back(*next);
                }
            }
        }
        let (node_id, event_id, choice) =
            choice_node.expect("map must contain an unlocked choice node");
        let mut path = vec![node_id];
        while let Some(prev) = parents.get(path.last().unwrap()).copied() {
            if prev == *path.last().unwrap() {
                break;
            }
            path.push(prev);
        }
        path.reverse();
        let resolved = path[1..path.len().saturating_sub(1)].to_vec();
        rouge::update_run_finished(&db, 1, 1, &resolved)
            .await
            .unwrap();
        for step in path.into_iter().skip(1) {
            player.round_move(&db, 1, 111, step).await.unwrap();
        }
        player
            .choice_event(&db, 1, 111, node_id, event_id, choice)
            .await
            .unwrap();

        // 4. Settlement returns the final snapshot, then GetInfo offers a bonus.
        set_terminal_state(&db, 1).await;
        let (info, result) = player.end(&db, 1).await.unwrap();
        assert_eq!(info.state, Some(5));
        assert!(result.final_score.unwrap() > 0);

        // 5. Claim a reward and return to setup.
        let info = player
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        let reward_id = info.last_reward[0].id.unwrap();
        let info = player.select_reward(&db, 1, &[reward_id]).await.unwrap();
        assert_eq!(info.state, Some(0));

        // 6. The earned bud is spendable on the talent tree.
        let outside = player.original_outside_info(&db, 1).await.unwrap();
        assert_eq!(
            outside.rouge_info.unwrap().genius_point,
            result.add_genius_point
        );
        assert!(result.add_genius_point.unwrap_or_default() >= 1);
        let ids = player.active_genius(&db, 1, 101).await.unwrap();
        assert_eq!(ids, vec![101]);

        // 7. The next difficulty is unlocked for the following run.
        assert!(
            player
                .select_difficulty(&db, 1, 2, &[], false)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn aborting_a_run_enters_failed_settlement_without_unlocking() {
        init_config();
        let db = test_db().await;
        let player = RougeManager::new(1);
        for (uid, hero_id) in [3003, 3004, 3007, 3010].into_iter().enumerate() {
            insert_hero(&db, 1, uid as i64 + 1, hero_id).await;
        }
        player
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        player.select_style(&db, 1, 1).await.unwrap();
        player
            .select_heroes(&db, 1, &[3003, 3004, 3007, 3010], 0)
            .await
            .unwrap();

        let info = player.abort_setup(&db, 1).await.unwrap();
        assert_eq!(info.state, Some(5));
        assert!(info.map_info.is_some());
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(run.terminal_state, 2);
        assert!(player.abort_setup(&db, 1).await.is_err());

        let (info, result) = player.end(&db, 1).await.unwrap();
        assert_eq!(info.state, Some(5));
        assert_eq!(result.end2_score.as_deref(), Some("0#0"));
        assert!(rouge::get_run(&db, 1, 1).await.unwrap().is_none());
        assert!(
            rouge::cleared_difficulties(&db, 1, 1)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
