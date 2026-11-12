use crate::models::game::rouge::{
    Rouge2CareerLevel, Rouge2MaterialState, Rouge2RewardState, Rouge2State, RougeOutside, RougeRun,
    RougeRunPosition, RougeStart,
};
use anyhow::Result;
use sqlx::{Sqlite, SqlitePool, Transaction};

pub struct Rouge2OutsideState {
    pub state: Rouge2State,
    pub career_levels: Vec<Rouge2CareerLevel>,
    pub rewards: Vec<Rouge2RewardState>,
    pub materials: Vec<Rouge2MaterialState>,
}

#[derive(Clone, Copy)]
pub enum Rouge2UnlockKind {
    Relic,
    Buff,
    ActiveSkill,
}

impl Rouge2UnlockKind {
    const fn id(self) -> i32 {
        match self {
            Self::Relic => 1,
            Self::Buff => 2,
            Self::ActiveSkill => 3,
        }
    }
}

pub async fn get_or_create_rouge2_outside(
    pool: &SqlitePool,
    user_id: i64,
    tables: &config::GameDB,
) -> Result<Rouge2OutsideState> {
    let now = common::time::ServerTime::now_ms();

    sqlx::query(
        "INSERT INTO user_rouge2_state (user_id, updated_at)
         VALUES (?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;

    for career in tables.rouge2_career.iter() {
        sqlx::query(
            "INSERT INTO user_rouge2_career_levels (user_id, career_id, updated_at)
             VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(career.id)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for reward in tables.rouge2_reward.iter() {
        sqlx::query(
            "INSERT INTO user_rouge2_rewards (user_id, reward_id, updated_at)
             VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(reward.id)
        .bind(now)
        .execute(pool)
        .await?;
    }

    for material in tables.rouge2_material.iter() {
        sqlx::query(
            "INSERT INTO user_rouge2_materials (user_id, material_id, updated_at)
             VALUES (?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(material.id)
        .bind(now)
        .execute(pool)
        .await?;
    }

    let state = sqlx::query_as::<_, Rouge2State>(
        "SELECT user_id, state, difficulty, coin, end_id, game_num,
                genius_point, genius_ids, reward_point, max_difficulty,
                pass_layer_ids, pass_event_ids, pass_end_ids, pass_entrust_ids,
                pass_collections, last_game_time, hotfix_str, updated_at
         FROM user_rouge2_state
         WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let career_levels = sqlx::query_as::<_, Rouge2CareerLevel>(
        "SELECT user_id, career_id, exp, updated_at
         FROM user_rouge2_career_levels
         WHERE user_id = ?
         ORDER BY career_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let rewards = sqlx::query_as::<_, Rouge2RewardState>(
        "SELECT user_id, reward_id, buy_count, updated_at
         FROM user_rouge2_rewards
         WHERE user_id = ?
         ORDER BY reward_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let materials = sqlx::query_as::<_, Rouge2MaterialState>(
        "SELECT user_id, material_id, num, updated_at
         FROM user_rouge2_materials
         WHERE user_id = ?
         ORDER BY material_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(Rouge2OutsideState {
        state,
        career_levels,
        rewards,
        materials,
    })
}

pub async fn get_or_create_unlock_ids(
    pool: &SqlitePool,
    user_id: i64,
    tables: &config::GameDB,
    kind: Rouge2UnlockKind,
) -> Result<Vec<i32>> {
    let now = common::time::ServerTime::now_ms();
    let ids: Vec<i32> = match kind {
        Rouge2UnlockKind::Relic => tables.rouge2_relics.iter().map(|row| row.id).collect(),
        Rouge2UnlockKind::Buff => tables.rouge2_buff.iter().map(|row| row.id).collect(),
        Rouge2UnlockKind::ActiveSkill => tables
            .rouge2_active_skill
            .iter()
            .map(|row| row.id)
            .collect(),
    };

    for id in ids {
        sqlx::query(
            "INSERT INTO user_rouge2_unlocks (user_id, unlock_type, unlock_id, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(kind.id())
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(sqlx::query_scalar(
        "SELECT unlock_id
         FROM user_rouge2_unlocks
         WHERE user_id = ? AND unlock_type = ?
         ORDER BY unlock_id",
    )
    .bind(user_id)
    .bind(kind.id())
    .fetch_all(pool)
    .await?)
}

/// Ensures the setup row exists and returns it.
pub async fn get_or_create_start(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> Result<RougeStart> {
    sqlx::query(
        "INSERT INTO user_rouge_start (user_id, season) VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(sqlx::query_as::<_, RougeStart>(
        "SELECT user_id, season, state, difficulty, style, hero_ids, assist_hero_uid,
                point_remainder, genius_remainder, rewards
         FROM user_rouge_start
         WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_one(pool)
    .await?)
}

/// Moves the setup row from `from_state` to `to_state`, storing the chosen option.
pub async fn advance_setup(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    from_state: i32,
    to_state: i32,
    difficulty: i32,
    style: i32,
    hero_ids: &[i32],
    assist_hero_uid: i64,
) -> Result<bool> {
    let encoded = serde_json::to_string(hero_ids)?;
    let result = sqlx::query(
        "UPDATE user_rouge_start
         SET state = ?, difficulty = ?, style = ?, hero_ids = ?, assist_hero_uid = ?
         WHERE user_id = ? AND season = ? AND state = ?",
    )
    .bind(to_state)
    .bind(difficulty)
    .bind(style)
    .bind(encoded)
    .bind(assist_hero_uid)
    .bind(user_id)
    .bind(season)
    .bind(from_state)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Forces the setup state (used to enter the post-run reward screen).
pub async fn set_setup_state(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    state: i32,
) -> Result<bool> {
    let result =
        sqlx::query("UPDATE user_rouge_start SET state = ? WHERE user_id = ? AND season = ?")
            .bind(state)
            .bind(user_id)
            .bind(season)
            .execute(pool)
            .await?;

    Ok(result.rows_affected() == 1)
}

/// Resets the setup row back to the empty state.
pub async fn reset_setup(pool: &SqlitePool, user_id: i64, season: i32) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE user_rouge_start
         SET state = 0, difficulty = 0, style = 0, hero_ids = '[]', assist_hero_uid = 0
         WHERE user_id = ? AND season = ? AND state IN (1, 3, 4)",
    )
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Turns an active exploration into a failed terminal run. The run is retained
/// until `RougeEnd` computes and commits its settlement.
pub async fn mark_run_failed(pool: &SqlitePool, user_id: i64, season: i32) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let mut tx = pool.begin().await?;
    let run = sqlx::query(
        "UPDATE user_rouge_run SET terminal_state = 2, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0",
    )
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(&mut *tx)
    .await?;
    let start = sqlx::query(
        "UPDATE user_rouge_start SET state = 5
         WHERE user_id = ? AND season = ? AND state = 4",
    )
    .bind(user_id)
    .bind(season)
    .execute(&mut *tx)
    .await?;
    if run.rows_affected() != 1 || start.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

/// Difficulties already cleared in this season, which gate the official unlock chain.
pub async fn cleared_difficulties(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> Result<Vec<i32>> {
    Ok(sqlx::query_scalar(
        "SELECT difficulty
         FROM user_rouge_progress
         WHERE user_id = ? AND season = ? AND cleared = 1
         ORDER BY difficulty",
    )
    .bind(user_id)
    .bind(season)
    .fetch_all(pool)
    .await?)
}

/// Stores the rewards offered after settlement.
pub async fn update_rewards(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    rewards: &str,
) -> Result<()> {
    sqlx::query("UPDATE user_rouge_start SET rewards = ? WHERE user_id = ? AND season = ?")
        .bind(rewards)
        .bind(user_id)
        .bind(season)
        .execute(pool)
        .await?;
    Ok(())
}

/// Claims the pending settlement-bonus screen exactly once.
pub async fn claim_rewards_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    season: i32,
    expected_rewards: &str,
    selected_rewards: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE user_rouge_start
         SET rewards = ?, state = 0
         WHERE user_id = ? AND season = ? AND state = 2 AND rewards = ?",
    )
    .bind(selected_rewards)
    .bind(user_id)
    .bind(season)
    .bind(expected_rewards)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Stores the score remainders carried to the next run.
pub async fn update_remainders(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    point: i32,
    genius: i32,
) -> Result<()> {
    sqlx::query(
        "UPDATE user_rouge_start SET point_remainder = ?, genius_remainder = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(point)
    .bind(genius)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records a cleared difficulty (used by settlement later on).
pub async fn mark_difficulty_cleared(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    difficulty: i32,
    score: i32,
) -> Result<()> {
    let now = common::time::ServerTime::now_ms();
    sqlx::query(
        "INSERT INTO user_rouge_progress (user_id, season, difficulty, cleared, best_score, updated_at)
         VALUES (?, ?, ?, 1, ?, ?)
         ON CONFLICT (user_id, season, difficulty) DO UPDATE SET
             cleared = 1,
             best_score = MAX(best_score, excluded.best_score),
             updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(season)
    .bind(difficulty)
    .bind(score)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Atomically commits one terminal run's Rouge-owned progression and removes
/// the active run. The next-run bonus is selected in the following step.
#[allow(clippy::too_many_arguments)]
pub async fn settle_run(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    difficulty: i32,
    cleared: bool,
    score: i32,
    point_remainder: i32,
    genius_remainder: i32,
    add_genius_point: i32,
    rewards: &str,
    completed_entrusts: &[i32],
    add_point: i32,
    rouge_info: &str,
    result_info: &str,
    expected_extra_balance: i32,
    extra_add_point: i32,
    expected_run_seed: i64,
    expected_terminal_state: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let mut tx = pool.begin().await?;
    if !super::rouge_extra::consume_in_transaction(
        &mut tx,
        user_id,
        season,
        expected_extra_balance,
        extra_add_point,
    )
    .await?
    {
        tx.rollback().await?;
        return Ok(false);
    }
    for id in completed_entrusts {
        sqlx::query("INSERT INTO user_rouge_entrust_history (user_id, season, entrust_id) VALUES (?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(user_id).bind(season).bind(id).execute(&mut *tx).await?;
    }
    if cleared {
        sqlx::query(
            "INSERT INTO user_rouge_progress
                (user_id, season, difficulty, cleared, best_score, updated_at)
             VALUES (?, ?, ?, 1, ?, ?)
             ON CONFLICT (user_id, season, difficulty) DO UPDATE SET
                 cleared = 1,
                 best_score = MAX(best_score, excluded.best_score),
                 updated_at = excluded.updated_at",
        )
        .bind(user_id)
        .bind(season)
        .bind(difficulty)
        .bind(score)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO user_rouge_outside (user_id, season, updated_at) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE user_rouge_outside SET genius_point = genius_point + ?, point = point + ?, have_get_point = have_get_point + ?, updated_at = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(add_genius_point)
    .bind(add_point.saturating_add(extra_add_point))
    .bind(add_point.saturating_add(extra_add_point))
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(&mut *tx)
    .await?;

    let start = sqlx::query(
        "UPDATE user_rouge_start
         SET state = 2, difficulty = 0, style = 0, hero_ids = '[]', assist_hero_uid = 0,
             point_remainder = ?, genius_remainder = ?, rewards = ?
         WHERE user_id = ? AND season = ? AND state IN (1, 3, 4, 5)",
    )
    .bind(point_remainder)
    .bind(genius_remainder)
    .bind(rewards)
    .bind(user_id)
    .bind(season)
    .execute(&mut *tx)
    .await?;
    let run = sqlx::query("DELETE FROM user_rouge_run WHERE user_id = ? AND season = ? AND seed = ? AND terminal_state = ? AND terminal_state IN (1,2)")
        .bind(user_id)
        .bind(season)
        .bind(expected_run_seed)
        .bind(expected_terminal_state)
        .execute(&mut *tx)
        .await?;
    if start.rows_affected() != 1 || run.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    if cleared
        && let Some(ending) = serde_json::from_str::<serde_json::Value>(rouge_info)
            .ok()
            .and_then(|info| info.get("endId").and_then(|id| id.as_i64()))
            .filter(|id| (101..=104).contains(id))
    {
        super::rouge_history::record_ending_in_transaction(
            &mut tx,
            user_id,
            season,
            ending as i32,
            now,
        )
        .await?;
    }
    sqlx::query("INSERT INTO user_rouge_settlement (user_id, season, rouge_info, result_info) VALUES (?, ?, ?, ?) ON CONFLICT (user_id, season) DO UPDATE SET rouge_info = excluded.rouge_info, result_info = excluded.result_info")
        .bind(user_id).bind(season).bind(rouge_info).bind(result_info).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn completed_entrusts(pool: &SqlitePool, user_id: i64, season: i32) -> Result<Vec<i32>> {
    Ok(sqlx::query_scalar("SELECT entrust_id FROM user_rouge_entrust_history WHERE user_id = ? AND season = ? ORDER BY entrust_id")
        .bind(user_id).bind(season).fetch_all(pool).await?)
}

/// Hero ids owned by the player, used to validate the initial companion selection.
pub async fn owned_hero_ids(pool: &SqlitePool, user_id: i64) -> Result<Vec<i32>> {
    Ok(
        sqlx::query_scalar(
            "SELECT DISTINCT hero_id FROM heroes WHERE user_id = ? ORDER BY hero_id",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?,
    )
}

/// Current exploration run, if the player is inside one.
pub async fn get_run(pool: &SqlitePool, user_id: i64, season: i32) -> Result<Option<RougeRun>> {
    Ok(sqlx::query_as::<_, RougeRun>(
        "SELECT user_id, season, layer_index, layer_id, stage, node_id, seed, graph, coin, power,
                updated_at, map_type, middle_layer_id, position_index, choices, finished, bag, shop,
                spent_coin, retry_num, terminal_state, minor_count, danger_count, crisis_count,
                step_count, team, power_limit, interactive_index, interactive_refresh, battle_interactive, team_exp, middle_state, details
         FROM user_rouge_run
         WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_optional(pool)
    .await?)
}

/// Creates (or replaces) the run row for a freshly started exploration.
#[allow(clippy::too_many_arguments)]
pub async fn create_run(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    layer_index: i32,
    layer_id: i32,
    stage: i32,
    node_id: i32,
    seed: i64,
    graph: &str,
    coin: i32,
    power: i32,
    bag: &str,
    team: &str,
    power_limit: i32,
    expected_rewards: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_rouge_run (
            user_id, season, layer_index, layer_id, stage, node_id, seed, graph, coin, power, updated_at,
            map_type, middle_layer_id, position_index, bag, team, power_limit
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 0, ?, ?, ?)
         ON CONFLICT (user_id, season) DO UPDATE SET
            layer_index = excluded.layer_index,
            layer_id = excluded.layer_id,
            stage = excluded.stage,
            node_id = excluded.node_id,
            seed = excluded.seed,
            graph = excluded.graph,
            coin = excluded.coin,
            power = excluded.power,
            updated_at = excluded.updated_at,
            map_type = 1,
            middle_layer_id = 0,
            position_index = 0,
            bag = excluded.bag,
            team = excluded.team,
            power_limit = excluded.power_limit,
            interactive_index = -1,
            interactive_refresh = 0,
            battle_interactive = '', team_exp = 0, middle_state = '', details = '{}',
            choices = '{}',
            finished = '[]',
            shop = '',
            spent_coin = 0,
            retry_num = 0,
            terminal_state = 0,
            minor_count = 0,
            danger_count = 0,
            crisis_count = 0,
            step_count = 0",
    )
    .bind(user_id)
    .bind(season)
    .bind(layer_index)
    .bind(layer_id)
    .bind(stage)
    .bind(node_id)
    .bind(seed)
    .bind(graph)
    .bind(coin)
    .bind(power)
    .bind(now)
    .bind(bag)
    .bind(team)
    .bind(power_limit)
    .execute(&mut *tx)
    .await?;
    let start = sqlx::query(
        "UPDATE user_rouge_start SET rewards = '[]'
         WHERE user_id = ? AND season = ? AND state = 4 AND rewards = ?",
    )
    .bind(user_id)
    .bind(season)
    .bind(expected_rewards)
    .execute(&mut *tx)
    .await?;
    if start.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

/// Moves the run to another node or board position.
pub async fn update_run_state(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    position: &RougeRunPosition,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET map_type = ?, layer_index = ?, layer_id = ?, stage = ?, node_id = ?,
             middle_layer_id = ?, position_index = ?, interactive_index = -1,
             interactive_refresh = 0, battle_interactive = '', updated_at = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(position.map_type)
    .bind(position.layer_index)
    .bind(position.layer_id)
    .bind(position.stage)
    .bind(position.node_id)
    .bind(position.middle_layer_id)
    .bind(position.position_index)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn move_run_with_entrust(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    position: &RougeRunPosition,
    expected_node: i32,
    old_state: &str,
    state: &str,
) -> Result<bool> {
    let result = sqlx::query("UPDATE user_rouge_run SET map_type = ?, layer_index = ?, layer_id = ?, stage = ?, node_id = ?, middle_layer_id = ?, position_index = ?, interactive_index = -1, interactive_refresh = 0, battle_interactive = '', middle_state = ?, updated_at = ? WHERE user_id = ? AND season = ? AND node_id = ? AND middle_state = ? AND terminal_state = 0")
        .bind(position.map_type).bind(position.layer_index).bind(position.layer_id).bind(position.stage)
        .bind(position.node_id).bind(position.middle_layer_id).bind(position.position_index).bind(state)
        .bind(common::time::ServerTime::now_ms()).bind(user_id).bind(season).bind(expected_node).bind(old_state)
        .execute(pool).await?;
    Ok(result.rows_affected() == 1)
}

/// Atomically archives the completed layer counters and enters a freshly
/// generated layer graph.
#[allow(clippy::too_many_arguments)]
pub async fn advance_run_layer(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected: &RougeRun,
    position: &RougeRunPosition,
    graph: &str,
    details: &str,
    minor_count: i32,
    danger_count: i32,
    crisis_count: i32,
    step_count: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET map_type = ?, layer_index = ?, layer_id = ?, stage = ?, node_id = ?,
             middle_layer_id = ?, position_index = ?, graph = ?, details = ?, choices = '{}', finished = '[]',
             shop = '', retry_num = 0, interactive_index = -1, interactive_refresh = 0, battle_interactive = '',
             minor_count = minor_count + ?,
             danger_count = danger_count + ?, crisis_count = crisis_count + ?,
             step_count = step_count + ?, updated_at = ?
         WHERE user_id = ? AND season = ? AND seed = ? AND layer_id = ?
           AND layer_index = ? AND map_type = 2 AND terminal_state = 0 AND details = ?",
    )
    .bind(position.map_type)
    .bind(position.layer_index)
    .bind(position.layer_id)
    .bind(position.stage)
    .bind(position.node_id)
    .bind(position.middle_layer_id)
    .bind(position.position_index)
    .bind(graph)
    .bind(details)
    .bind(minor_count)
    .bind(danger_count)
    .bind(crisis_count)
    .bind(step_count)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(expected.seed)
    .bind(expected.layer_id)
    .bind(expected.layer_index)
    .bind(&expected.details)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Stores the resolved node ids for the current layer.
pub async fn update_run_finished(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    finished: &[i32],
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET finished = ?, updated_at = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(serde_json::to_string(finished)?)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn finish_node_with_entrust(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected_finished: &str,
    finished: &str,
    expected_middle: &str,
    middle: &str,
) -> Result<bool> {
    let result = sqlx::query("UPDATE user_rouge_run SET finished = ?, middle_state = ?, updated_at = ? WHERE user_id = ? AND season = ? AND finished = ? AND middle_state = ? AND terminal_state = 0")
        .bind(finished).bind(middle).bind(common::time::ServerTime::now_ms()).bind(user_id).bind(season).bind(expected_finished).bind(expected_middle)
        .execute(pool).await?;
    Ok(result.rows_affected() == 1)
}

/// Persists the current combat party and recruited-hero life state.
pub async fn update_run_team(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    team: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET team = ?, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0",
    )
    .bind(team)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Adds to the coins spent during the run (badge telemetry).
pub async fn add_spent_coin(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    delta: i32,
) -> Result<()> {
    sqlx::query(
        "UPDATE user_rouge_run SET spent_coin = spent_coin + ? WHERE user_id = ? AND season = ?",
    )
    .bind(delta)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stores the current shop stock for the run.
pub async fn update_run_shop(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    shop: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET shop = ?, updated_at = ? WHERE user_id = ? AND season = ?",
    )
    .bind(shop)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Stores the run's coin and medium (power).
pub async fn update_run_resources(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    coin: i32,
    power: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET coin = ?, power = ?, updated_at = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(coin)
    .bind(power)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Atomically applies every state change caused by one shop purchase.
#[allow(clippy::too_many_arguments)]
pub async fn apply_shop_purchase(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    bag: &str,
    shop: &str,
    coin: i32,
    power: i32,
    spent_coin: i32,
    expected_bag: &str,
    expected_shop: &str,
    expected_coin: i32,
    expected_middle: &str,
    middle: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET bag = ?, shop = ?, coin = ?, power = ?, spent_coin = spent_coin + ?, middle_state = ?, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0 AND bag = ? AND shop = ? AND coin = ? AND middle_state = ?",
    )
    .bind(bag)
    .bind(shop)
    .bind(coin)
    .bind(power)
    .bind(spent_coin)
    .bind(middle)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(expected_bag)
    .bind(expected_shop)
    .bind(expected_coin)
    .bind(expected_middle)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Stores the backpack placement for the run.
pub async fn update_run_bag(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    bag: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET bag = ?, updated_at = ? WHERE user_id = ? AND season = ?",
    )
    .bind(bag)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Stores the chosen option map for the run.
pub async fn update_run_choices(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    choices: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run SET choices = ?, updated_at = ? WHERE user_id = ? AND season = ?",
    )
    .bind(choices)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Records a choice and starts its first supported interaction atomically.
pub async fn select_run_choice(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected: &RougeRun,
    choices: &str,
    interactive_index: i32,
    team: &str,
    middle: &str,
    coin: i32,
    power: i32,
    exp: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET choices = ?, interactive_index = ?, team = ?, middle_state = ?, coin = ?, power = ?, team_exp = ?, interactive_refresh = 0, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0
           AND choices = ? AND team = ? AND middle_state = ? AND interactive_index = -1
           AND coin = ? AND power = ? AND team_exp = ? AND power_limit = ? AND layer_id = ? AND node_id = ? AND map_type = 1",
    )
    .bind(choices)
    .bind(interactive_index)
    .bind(team)
    .bind(middle)
    .bind(coin)
    .bind(power)
    .bind(exp)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(&expected.choices)
    .bind(&expected.team)
    .bind(&expected.middle_state)
    .bind(expected.coin)
    .bind(expected.power)
    .bind(expected.team_exp)
    .bind(expected.power_limit)
    .bind(expected.layer_id)
    .bind(expected.node_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Applies a team-life change and advances (or clears) its interaction exactly once.
pub async fn consume_team_interactive(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected_team: &str,
    team: &str,
    interactive_index: i32,
    next_interactive_index: i32,
    expected_middle: &str,
    middle: &str,
    expected_node: i32,
    expected_pipeline: &str,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET team = ?, interactive_index = ?, middle_state = ?, interactive_refresh = 0, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0
           AND team = ? AND interactive_index = ? AND middle_state = ? AND node_id = ? AND battle_interactive = ?",
    )
    .bind(team)
    .bind(next_interactive_index)
    .bind(middle)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(expected_team)
    .bind(interactive_index)
    .bind(expected_middle)
    .bind(expected_node)
    .bind(expected_pipeline)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Rerolls the deterministic candidate list of one pending drop interaction.
pub async fn refresh_drop_interactive(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    interactive_index: i32,
    expected_refresh: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET interactive_refresh = interactive_refresh + 1, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0
           AND interactive_index = ? AND interactive_refresh = ?",
    )
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(interactive_index)
    .bind(expected_refresh)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Adds selected drop instances and advances the interaction exactly once.
#[allow(clippy::too_many_arguments)]
pub async fn consume_drop_interactive(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected_bag: &str,
    bag: &str,
    interactive_index: i32,
    interactive_refresh: i32,
    next_interactive_index: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_run
         SET bag = ?, interactive_index = ?, interactive_refresh = 0, updated_at = ?
         WHERE user_id = ? AND season = ? AND terminal_state = 0
           AND bag = ? AND interactive_index = ? AND interactive_refresh = ?",
    )
    .bind(bag)
    .bind(next_interactive_index)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(expected_bag)
    .bind(interactive_index)
    .bind(interactive_refresh)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Claims a battle reward and advances its pipeline in one conditional update.
#[allow(clippy::too_many_arguments)]
pub async fn claim_battle_drop(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected_bag: &str,
    bag: &str,
    index: i32,
    refresh: i32,
    next: i32,
    coin: i32,
    exp: i32,
    expected_middle: &str,
    middle: &str,
    expected_node: i32,
    expected_pipeline: &str,
) -> Result<bool> {
    let result = sqlx::query("UPDATE user_rouge_run SET bag = ?, coin = coin + ?, team_exp = team_exp + ?, interactive_index = ?, interactive_refresh = 0, middle_state = ?, updated_at = ? WHERE user_id = ? AND season = ? AND terminal_state = 0 AND bag = ? AND interactive_index = ? AND interactive_refresh = ? AND battle_interactive = ? AND node_id = ? AND middle_state = ?")
        .bind(bag).bind(coin).bind(exp).bind(next).bind(middle).bind(common::time::ServerTime::now_ms())
        .bind(user_id).bind(season).bind(expected_bag).bind(index).bind(refresh).bind(expected_pipeline).bind(expected_node).bind(expected_middle)
        .execute(pool).await?;
    Ok(result.rows_affected() == 1)
}

#[allow(clippy::too_many_arguments)]
pub async fn consume_drop_with_entrust(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    expected_bag: &str,
    bag: &str,
    index: i32,
    refresh: i32,
    next: i32,
    expected_middle: &str,
    middle: &str,
    expected_node: i32,
    expected_pipeline: &str,
) -> Result<bool> {
    let result = sqlx::query("UPDATE user_rouge_run SET bag = ?, interactive_index = ?, interactive_refresh = 0, middle_state = ?, updated_at = ? WHERE user_id = ? AND season = ? AND terminal_state = 0 AND bag = ? AND interactive_index = ? AND interactive_refresh = ? AND middle_state = ? AND node_id = ? AND battle_interactive = ?")
        .bind(bag).bind(next).bind(middle).bind(common::time::ServerTime::now_ms()).bind(user_id).bind(season)
        .bind(expected_bag).bind(index).bind(refresh).bind(expected_middle).bind(expected_node).bind(expected_pipeline).execute(pool).await?;
    Ok(result.rows_affected() == 1)
}

/// Removes the run, e.g. after settlement.
pub async fn delete_run(pool: &SqlitePool, user_id: i64, season: i32) -> Result<bool> {
    let result = sqlx::query("DELETE FROM user_rouge_run WHERE user_id = ? AND season = ?")
        .bind(user_id)
        .bind(season)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

/// Ensures and returns the meta progression row.
pub async fn get_or_create_outside(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
) -> Result<RougeOutside> {
    let now = common::time::ServerTime::now_ms();
    sqlx::query(
        "INSERT INTO user_rouge_outside (user_id, season, updated_at) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(sqlx::query_as::<_, RougeOutside>(
        "SELECT user_id, season, genius_point, point, have_get_point, updated_at
         FROM user_rouge_outside WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_one(pool)
    .await?)
}

/// Adds (or subtracts) 刻画的枝芽.
pub async fn add_genius_point(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    delta: i32,
) -> Result<()> {
    let now = common::time::ServerTime::now_ms();
    sqlx::query(
        "UPDATE user_rouge_outside SET genius_point = genius_point + ?, updated_at = ?
         WHERE user_id = ? AND season = ?",
    )
    .bind(delta)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .execute(pool)
    .await?;
    Ok(())
}

/// Deducts 刻画的枝芽, returning false when the balance is insufficient.
pub async fn spend_genius_point(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    cost: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "UPDATE user_rouge_outside SET genius_point = genius_point - ?, updated_at = ?
         WHERE user_id = ? AND season = ? AND genius_point >= ?",
    )
    .bind(cost)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(cost)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Activated talent nodes for a season.
pub async fn list_genius(pool: &SqlitePool, user_id: i64, season: i32) -> Result<Vec<i32>> {
    Ok(sqlx::query_scalar(
        "SELECT genius_id FROM user_rouge_genius WHERE user_id = ? AND season = ?
         ORDER BY genius_id",
    )
    .bind(user_id)
    .bind(season)
    .fetch_all(pool)
    .await?)
}

/// Marks a talent node as activated.
pub async fn insert_genius(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    genius_id: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let result = sqlx::query(
        "INSERT INTO user_rouge_genius (user_id, season, genius_id, updated_at)
         VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(genius_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Atomically spends buds and records a newly activated talent node.
pub async fn activate_genius(
    pool: &SqlitePool,
    user_id: i64,
    season: i32,
    genius_id: i32,
    cost: i32,
) -> Result<bool> {
    let now = common::time::ServerTime::now_ms();
    let mut tx = pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO user_rouge_genius (user_id, season, genius_id, updated_at)
         VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .bind(genius_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }

    let spent = sqlx::query(
        "UPDATE user_rouge_outside SET genius_point = genius_point - ?, updated_at = ?
         WHERE user_id = ? AND season = ? AND genius_point >= ?",
    )
    .bind(cost)
    .bind(now)
    .bind(user_id)
    .bind(season)
    .bind(cost)
    .execute(&mut *tx)
    .await?;
    if spent.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}
