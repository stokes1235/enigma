use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Rouge2State {
    pub user_id: i64,
    pub state: i32,
    pub difficulty: i32,
    pub coin: i32,
    pub end_id: i32,
    pub game_num: i32,
    pub genius_point: i32,
    pub genius_ids: String,
    pub reward_point: i32,
    pub max_difficulty: i32,
    pub pass_layer_ids: String,
    pub pass_event_ids: String,
    pub pass_end_ids: String,
    pub pass_entrust_ids: String,
    pub pass_collections: String,
    pub last_game_time: i64,
    pub hotfix_str: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct Rouge2CareerLevel {
    pub user_id: i64,
    pub career_id: i32,
    pub exp: i32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct Rouge2RewardState {
    pub user_id: i64,
    pub reward_id: i32,
    pub buy_count: i32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct Rouge2MaterialState {
    pub user_id: i64,
    pub material_id: i32,
    pub num: i32,
    pub updated_at: i64,
}

/// Setup state for the original Rouge (A Series of Dusks).
#[derive(Debug, Clone, FromRow)]
pub struct RougeStart {
    pub user_id: i64,
    pub season: i32,
    pub state: i32,
    pub difficulty: i32,
    pub style: i32,
    pub hero_ids: String,
    pub assist_hero_uid: i64,
    pub point_remainder: i32,
    pub genius_remainder: i32,
    pub rewards: String,
}

/// Active exploration run: current layer/position plus the generated layer graph.
#[derive(Debug, Clone, FromRow)]
pub struct RougeRun {
    pub user_id: i64,
    pub season: i32,
    pub layer_index: i32,
    pub layer_id: i32,
    pub stage: i32,
    pub node_id: i32,
    pub seed: i64,
    pub graph: String,
    pub coin: i32,
    pub power: i32,
    pub updated_at: i64,
    pub map_type: i32,
    pub middle_layer_id: i32,
    pub position_index: i32,
    pub choices: String,
    pub finished: String,
    pub bag: String,
    pub shop: String,
    pub spent_coin: i32,
    pub retry_num: i32,
    /// 0 = active, 1 = cleared, 2 = failed.
    pub terminal_state: i32,
    pub minor_count: i32,
    pub danger_count: i32,
    pub crisis_count: i32,
    pub step_count: i32,
    pub team: String,
    pub power_limit: i32,
    pub interactive_index: i32,
    pub interactive_refresh: i32,
    pub battle_interactive: String,
    pub team_exp: i32,
    pub middle_state: String,
    pub details: String,
}

/// Position of a run on either the layer map or the middle-layer board.
#[derive(Debug, Clone, Copy, Default)]
pub struct RougeRunPosition {
    pub map_type: i32,
    pub layer_index: i32,
    pub layer_id: i32,
    pub stage: i32,
    pub node_id: i32,
    pub middle_layer_id: i32,
    pub position_index: i32,
}

/// Meta progression row (刻画的枝芽 and unlocked talent nodes).
#[derive(Debug, Clone, FromRow)]
pub struct RougeOutside {
    pub user_id: i64,
    pub season: i32,
    pub genius_point: i32,
    pub point: i32,
    pub have_get_point: i32,
    pub updated_at: i64,
}
