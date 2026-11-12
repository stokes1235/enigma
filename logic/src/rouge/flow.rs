//! Rules shared by map interactions and battle settlement. Random choices are
//! seeded per run; no captured account data is used to construct a run.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    /// Snapshot permanent growth at run creation. None marks a pre-upgrade run.
    pub outside_genius: Option<Vec<i32>>,
    pub passed_endings: Option<Vec<i32>>,
    /// Legacy runs keep their previous seeded candidates. New runs enforce
    /// the captured outside-growth snapshot in every random collection pool.
    pub collection_pool_version: i32,
    pub battle_reward: Option<BattleReward>,
    pub artifact_state: super::artifact_rules::MapState,
    pub artifact_power_limit_bonus: i32,
    pub safe_zone_recovery: Option<i32>,
    pub active_talents: Vec<usize>,
    pub end_episode: i32,
    pub end_id: i32,
    pub consume_power: i32,
    pub repair_shop_num: i32,
    pub displace_num: i32,
    pub special_events: super::special_events::State,
    pub achievements: super::badges::Progress,
    pub compositions: BTreeMap<i32, i32>,
    pub triggered_talents: Vec<usize>,
    pub effects: Vec<i32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BattleReward {
    pub node_id: i32,
    pub drop_id: i32,
    pub coin: i32,
    pub exp: i32,
    pub extra_drops: i32,
    pub extra_select: i32,
    #[serde(default)]
    pub disk_choices: i32,
}

pub fn details(run: &RougeRun) -> Details {
    serde_json::from_str(&run.details).unwrap_or_default()
}

pub fn bonuses(run: &RougeRun) -> super::genius_bonuses::Bonuses {
    let state = details(run);
    super::genius_bonuses::from_ids(
        config::configs::get(),
        state.outside_genius.as_deref().unwrap_or_default(),
    )
}

pub fn eligible_collections(run: &RougeRun) -> Option<std::collections::BTreeSet<i32>> {
    let detail = details(run);
    (detail.collection_pool_version > 0).then(|| {
        super::favorites::unlock_collection_ids(
            config::configs::get(),
            detail.outside_genius.as_deref().unwrap_or_default(),
        )
        .into_iter()
        .collect()
    })
}

pub fn completed_layers(run: &RougeRun) -> i32 {
    // Entering an area is not clearing it. The first battle loss in area two
    // is captured as one cleared area (750 credit and two bonus Keepsakes).
    run.layer_index.max(0) + i32::from(run.map_type == MAP_TYPE_MIDDLE || run.terminal_state == 1)
}

pub fn reward(run: &RougeRun, drop_id: i32) -> BattleReward {
    if let Some(quote) = details(run).battle_reward
        && quote.node_id == run.node_id
        && quote.drop_id == drop_id
    {
        return quote;
    }
    let growth = bonuses(run);
    let extra = growth.extra_drops(drop_id);
    let item_effects = serde_json::from_str::<bag::RunBag>(&run.bag)
        .ok()
        .map(|bag| {
            super::artifact_rules::evaluate_battle_with_state(
                config::configs::get(),
                &bag,
                run.coin,
                &details(run).artifact_state,
            )
        })
        .unwrap_or_default();
    BattleReward {
        node_id: run.node_id,
        drop_id,
        coin: config::configs::get()
            .rouge_drop
            .get(drop_id)
            .map(|row| {
                ((drop_coin(row) as i64
                    * (1000
                        + 10 * (growth.coin_percent + growth.battle_coin_percent)
                        + item_effects.coin_drop_bonus_per_mille)
                        .max(0) as i64
                    / 1000)
                    .min(i32::MAX as i64)) as i32
            })
            .unwrap_or(0),
        exp: super::genius_bonuses::scale(reward_exp(drop_id), growth.essence_percent),
        extra_drops: extra.0,
        extra_select: extra.1,
        disk_choices: if (1001..=1015).contains(&drop_id) {
            item_effects.extra_disk_choices
        } else {
            0
        },
    }
}

pub fn freeze_battle_reward(run: &mut RougeRun, event_id: i32) -> Result<(), AppError> {
    let tables = config::configs::get();
    let mut detail = details(run);
    detail.battle_reward = None;
    run.details = serde_json::to_string(&detail)?;
    if let Some(drop_id) = tables
        .rouge_fight_event
        .get(event_id)
        .and_then(|event| supported_interactive(&event.advance_interactive))
        .filter(|(kind, _)| *kind == INTERACT_ADVANCE_DROP)
        .map(|(_, id)| id)
    {
        detail.battle_reward = Some(reward(run, drop_id));
    }
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

/// The settlement includes inlaid Ritual Disks as owned collections too.
pub fn collection_counts(bag: &bag::RunBag) -> BTreeMap<i32, i32> {
    let mut result = BTreeMap::new();
    for item in &bag.items {
        *result.entry(item.item_id).or_default() += 1;
        for id in item.holes.iter().filter(|id| **id > 0) {
            *result.entry(*id).or_default() += 1;
        }
    }
    result
}

pub fn counts_string(counts: &BTreeMap<i32, i32>) -> String {
    counts
        .iter()
        .map(|(id, count)| format!("{id}#{count}"))
        .collect::<Vec<_>>()
        .join("|")
}

pub fn apply_team_life(
    fight: &mut sonettobuf::Fight,
    roster: &RougeTeamInfo,
) -> Result<(), AppError> {
    if let Some(team) = &mut fight.attacker {
        for entity in team.entitys.iter_mut().chain(team.sub_entitys.iter_mut()) {
            let Some(hero) = entity.model_id else {
                continue;
            };
            let life = roster
                .hero_life_list
                .iter()
                .find(|life| life.hero_id == Some(hero))
                .and_then(|life| life.life)
                .filter(|life| *life > 0)
                .ok_or(AppError::InvalidRequest)?;
            let hp = entity
                .attr
                .as_ref()
                .and_then(|attr| attr.hp)
                .ok_or(AppError::InvalidRequest)?;
            entity.current_hp =
                Some(((hp as i64 * life.clamp(0, 1000) as i64 / 1000).max(1)) as i32);
        }
    }
    Ok(())
}

/// Attribute entries use the same per-mille values as the hero builder.
/// Only explicit numeric attributes are interpreted here; scripted talent
/// effects still require their combat implementation.
pub fn attribute_rates(start: &RougeStart, run: &RougeRun) -> BTreeMap<i32, i32> {
    let tables = config::configs::get();
    let tree = talent_tree(start, run);
    let mut rates = bonuses(run).attributes;
    for talent in tree
        .rouge_talent
        .iter()
        .filter(|talent| talent.is_active == Some(1))
    {
        if let Some(row) = talent.id.and_then(|id| tables.rouge_style_talent.get(id)) {
            for raw in row.attribute.split('|') {
                if let Some((id, rate)) = raw.split_once('#')
                    && let (Ok(id), Ok(rate)) = (id.parse::<i32>(), rate.parse::<i32>())
                {
                    *rates.entry(id).or_default() += rate;
                }
            }
        }
    }
    rates
}

pub fn team_capacity(start: &RougeStart, run: &RougeRun) -> i32 {
    config::configs::get()
        .rouge_style(start.season, start.style)
        .map(|style| style.capacity)
        .unwrap_or_default()
        + bonuses(run).initial_capacity
        + (team_progress(run.season, run.team_exp).0 - 1).max(0) * 2
}

pub fn talent_tree(start: &RougeStart, run: &RougeRun) -> sonettobuf::RougeTalentTree {
    let tables = config::configs::get();
    let personal: Vec<i32> = tables
        .rouge_style(start.season, start.style)
        .map(|style| {
            style
                .talent_skill
                .split('#')
                .filter_map(|id| id.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let active = details(run).active_talents;
    let mut talent = Vec::new();
    for pair in 0..9 {
        let kind = if pair == 3 {
            1
        } else if pair == 1 || pair == 5 {
            2
        } else {
            3
        };
        let mut pool: Vec<i32> = if kind == 1 {
            personal.clone()
        } else {
            tables
                .rouge_style_talent
                .iter()
                .filter(|row| {
                    row.r#type == kind
                        && row.version.is_empty()
                        && row.unlock_type == 0
                        && !row
                            .ban
                            .split('#')
                            .any(|id| id.parse::<i32>().ok() == Some(start.style))
                })
                .map(|row| row.id)
                .collect()
        };
        pool.sort_unstable();
        if pool.is_empty() {
            continue;
        }
        let offset =
            (run.seed.unsigned_abs().wrapping_add(pair as u64 * 7919) % pool.len() as u64) as usize;
        for side in 0..2 {
            let index = pair * 2 + side;
            talent.push(sonettobuf::RougeTalent {
                id: Some(pool[(offset + side) % pool.len()]),
                is_active: Some(i32::from(active.contains(&index))),
            });
        }
    }
    sonettobuf::RougeTalentTree {
        rouge_talent: talent,
    }
}

pub fn talent_points(run: &RougeRun) -> i32 {
    (team_progress(run.season, run.team_exp).0 - 1 - details(run).active_talents.len() as i32)
        .max(0)
}

/// Optimistic update of one complete interaction. In particular, item removal,
/// resource payment, recruitment and map transitions cannot partially commit.
pub async fn save<'e>(
    db: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
    expected: &RougeRun,
    run: &RougeRun,
) -> Result<(), AppError> {
    let changed = sqlx::query("UPDATE user_rouge_run SET graph = ?, retry_num = ?, bag = ?, team = ?, coin = ?, power = ?, power_limit = ?, team_exp = ?, choices = ?, finished = ?, middle_state = ?, details = ?, shop = ?, battle_interactive = ?, interactive_index = ?, interactive_refresh = ?, map_type = ?, middle_layer_id = ?, position_index = ?, terminal_state = ?, spent_coin = ?, stage = ?, node_id = ? WHERE user_id = ? AND season = ? AND bag = ? AND team = ? AND coin = ? AND power = ? AND power_limit = ? AND team_exp = ? AND choices = ? AND finished = ? AND middle_state = ? AND details = ? AND shop = ? AND battle_interactive = ? AND interactive_index = ? AND interactive_refresh = ? AND map_type = ? AND middle_layer_id = ? AND position_index = ? AND terminal_state = ? AND layer_id = ? AND node_id = ? AND seed = ? AND graph = ? AND retry_num = ?")
        .bind(&run.graph).bind(run.retry_num)
        .bind(&run.bag).bind(&run.team).bind(run.coin).bind(run.power).bind(run.power_limit)
        .bind(run.team_exp).bind(&run.choices).bind(&run.finished).bind(&run.middle_state)
        .bind(&run.details).bind(&run.shop).bind(&run.battle_interactive).bind(run.interactive_index)
        .bind(run.interactive_refresh).bind(run.map_type).bind(run.middle_layer_id)
        .bind(run.position_index).bind(run.terminal_state).bind(run.spent_coin)
        .bind(run.stage).bind(run.node_id)
        .bind(expected.user_id).bind(expected.season).bind(&expected.bag).bind(&expected.team)
        .bind(expected.coin).bind(expected.power).bind(expected.power_limit).bind(expected.team_exp)
        .bind(&expected.choices).bind(&expected.finished).bind(&expected.middle_state).bind(&expected.details)
        .bind(&expected.shop).bind(&expected.battle_interactive).bind(expected.interactive_index)
        .bind(expected.interactive_refresh).bind(expected.map_type).bind(expected.middle_layer_id)
        .bind(expected.position_index).bind(expected.terminal_state).bind(expected.layer_id).bind(expected.node_id)
        .bind(expected.seed)
        .bind(&expected.graph).bind(expected.retry_num)
        .execute(db).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::InvalidRequest);
    }
    Ok(())
}

pub async fn save_action<'e>(
    db: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
    expected: &RougeRun,
    run: &mut RougeRun,
) -> Result<(), AppError> {
    super::artifact_lifecycle::prepare(expected, run)?;
    super::badges::observe_roster(run)?;
    save(db, expected, run).await
}

pub fn place_special_events(run: &mut RougeRun) -> Result<(), AppError> {
    if run.map_type != MAP_TYPE_NORMAL {
        return Ok(());
    }
    let mut detail = details(run);
    if detail.special_events.pending.is_empty() {
        return Ok(());
    }
    let mut graph: map::LayerMap = serde_json::from_str(&run.graph)?;
    let mut resolved: std::collections::BTreeSet<i32> =
        json_ids(&run.finished).into_iter().collect();
    let choices: HashMap<String, i32> = serde_json::from_str(&run.choices)?;
    resolved.extend(choices.keys().filter_map(|id| id.parse::<i32>().ok()));
    super::special_events::place_pending(
        config::configs::get(),
        &mut detail.special_events,
        &mut graph,
        Some(run.node_id),
        &resolved,
    )
    .map_err(|_| AppError::InvalidRequest)?;
    run.graph = serde_json::to_string(&graph)?;
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

pub fn apply_special_reward(
    run: &mut RougeRun,
    reward: super::special_events::FixedReward,
) -> Result<(), AppError> {
    if run.coin < reward.coin_cost {
        return Err(AppError::InsufficientCurrency);
    }
    run.coin -= reward.coin_cost;
    run.spent_coin = run.spent_coin.saturating_add(reward.coin_cost);
    run.coin = run.coin.saturating_add(super::genius_bonuses::scale(
        reward.coin_gain,
        bonuses(run).coin_percent,
    ));
    let spent = run.power.min(reward.power_cost_up_to.max(0));
    run.power -= spent;
    let mut detail = details(run);
    detail.consume_power = detail.consume_power.saturating_add(spent);
    run.details = serde_json::to_string(&detail)?;
    if reward.current_life_cost_per_mille > 0 {
        let mut team: RougeTeamInfo = serde_json::from_str(&run.team)?;
        for life in &mut team.hero_life_list {
            if let Some(hp) = life.life.filter(|hp| *hp > 0) {
                life.life = Some(
                    (hp * (1000 - reward.current_life_cost_per_mille.clamp(0, 1000)) / 1000).max(1),
                );
            }
        }
        run.team = serde_json::to_string(&team)?;
    }
    let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
    for id in reward.grant_items {
        // Ending keys are unique; another source must not create a second copy.
        if !collection_shapes::is_unique(*id)
            || !super::artifact_lifecycle::owned(&bag)
                .values()
                .any(|owned| owned == id)
        {
            bag.add(*id)?;
        }
    }
    run.bag = serde_json::to_string(&bag)?;
    Ok(())
}

pub fn special_battle_victory(run: &mut RougeRun, event_id: i32) -> Result<(), AppError> {
    let mut detail = details(run);
    let mut middle = serde_json::from_str::<MiddleState>(&run.middle_state)
        .unwrap_or_else(|_| middle_state(run));
    let Some(reward) = super::special_events::complete_battle(
        &mut detail.special_events,
        super::special_events::BattleSource {
            layer_id: run.layer_id,
            node_id: run.node_id,
            event_id,
        },
        middle.entrust_id,
        true,
    ) else {
        return Ok(());
    };
    run.details = serde_json::to_string(&detail)?;
    apply_special_reward(run, reward.reward)?;
    if let Some(entrust) = reward.complete_entrust {
        if !middle.completed_entrusts.contains(&entrust) {
            middle.completed_entrusts.push(entrust);
        }
        middle.entrust_id = 0;
        middle.entrust_progress = 0;
        run.middle_state = serde_json::to_string(&middle)?;
    }
    Ok(())
}

/// Completing a leaf enters its safe zone even when it was a treasure/rest
/// event. The final safe zone is still followed by an explicitly chosen fight.
pub fn finish_node(run: &mut RougeRun) -> Result<(), AppError> {
    if run.terminal_state != 0 || run.interactive_index >= 0 || run.map_type != MAP_TYPE_NORMAL {
        return Ok(());
    }
    let graph: map::LayerMap = serde_json::from_str(&run.graph)?;
    if !graph
        .node(run.node_id)
        .is_some_and(|node| node.next.is_empty() && node_is_resolved(run, node))
    {
        return Ok(());
    }
    let board = middle_board(config::configs::get(), run.layer_id)?;
    run.map_type = MAP_TYPE_MIDDLE;
    run.middle_layer_id = board.id;
    run.position_index = 0;
    run.battle_interactive.clear();
    // Catalyst recovery is applied on entering, never on moving around a zone.
    // All current base catalysts state recovery of their initial Stimulus.
    sync_artifact_limit(run)?;
    let before_recovery = run.clone();
    run.power = run
        .power
        .saturating_add(details(run).safe_zone_recovery.unwrap_or(40))
        .min(run.power_limit);
    let heal = bonuses(run).safe_heal;
    if heal > 0 {
        heal_all(run, heal)?;
    }
    apply_map_effects(run, super::artifact_rules::MapTrigger::EnterSafeZone)?;
    run.middle_state = updated_entrust(
        &before_recovery,
        EntrustChange {
            gained_coin: (run.coin - before_recovery.coin).max(0),
            gained_power: (run.power - before_recovery.power).max(0),
            ..Default::default()
        },
    )?;
    let mut state = middle_state(run);
    if let Some(entrust) = config::configs::get().rouge_entrust.get(state.entrust_id) {
        if entrust_completed(entrust, state.entrust_progress) {
            let reward = entrust_effects::apply_reward(
                config::configs::get(),
                state.entrust_id,
                entrust_effects::Resources {
                    coin: run.coin,
                    power: run.power,
                    power_limit: run.power_limit,
                },
            );
            let supported =
                first_supported_interactive(&entrust.interactive, config::configs::get());
            if let Ok(reward) = reward
                && (entrust.interactive.is_empty() || supported.is_some())
            {
                if let Some(reward) = reward {
                    run.coin = run.coin.saturating_add(super::genius_bonuses::scale(
                        reward.coin_delta,
                        bonuses(run).coin_percent,
                    ));
                    run.power = reward.resources.power;
                    run.power_limit = reward.resources.power_limit;
                }
                if let Some(index) = supported {
                    run.battle_interactive = entrust.interactive.clone();
                    run.interactive_index = index;
                }
                if !state.completed_entrusts.contains(&state.entrust_id) {
                    state.completed_entrusts.push(state.entrust_id);
                }
                state.entrust_id = 0;
                state.entrust_progress = 0;
            }
        } else if entrust.incomplete_effect.is_empty() {
            let mut detail = details(run);
            super::special_events::cancel_entrust(&mut detail.special_events, state.entrust_id);
            run.details = serde_json::to_string(&detail)?;
            state.entrust_id = 0;
            state.entrust_progress = 0;
        }
    }
    run.middle_state = serde_json::to_string(&state)?;
    Ok(())
}

pub async fn resume(db: &SqlitePool, expected: RougeRun) -> Result<RougeRun, AppError> {
    let mut run = expected.clone();
    sync_artifact_limit(&mut run)?;
    super::artifact_lifecycle::prepare(&expected, &mut run)?;
    finish_node(&mut run)?;
    if run.map_type != expected.map_type
        || run.details != expected.details
        || run.power_limit != expected.power_limit
        || run.power != expected.power
    {
        save_action(db, &expected, &mut run).await?;
    }
    Ok(run)
}

pub fn sync_artifact_limit(run: &mut RougeRun) -> Result<(), AppError> {
    let Ok(bag) = serde_json::from_str::<bag::RunBag>(&run.bag) else {
        return Ok(());
    };
    let effects = super::artifact_rules::evaluate_battle_with_state(
        config::configs::get(),
        &bag,
        run.coin,
        &details(run).artifact_state,
    );
    let mut detail = details(run);
    let delta = effects
        .power_limit_bonus
        .saturating_sub(detail.artifact_power_limit_bonus);
    run.power_limit = run.power_limit.saturating_add(delta).max(0);
    run.power = run.power.clamp(0, run.power_limit);
    detail.artifact_power_limit_bonus = effects.power_limit_bonus;
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

/// Trigger, counter advance, consumption and resource changes are prepared on
/// one run snapshot. The caller commits them with its owning action.
pub fn apply_map_effects(
    run: &mut RougeRun,
    trigger: super::artifact_rules::MapTrigger,
) -> Result<(), AppError> {
    let Ok(bag) = serde_json::from_str::<bag::RunBag>(&run.bag) else {
        return Ok(());
    };
    apply_map_effects_from(run, trigger, &bag)
}

pub(super) fn apply_map_effects_from(
    run: &mut RougeRun,
    trigger: super::artifact_rules::MapTrigger,
    source: &bag::RunBag,
) -> Result<(), AppError> {
    sync_artifact_limit(run)?;
    let Ok(mut bag) = serde_json::from_str::<bag::RunBag>(&run.bag) else {
        return Ok(());
    };
    let mut detail = details(run);
    let change = super::artifact_rules::evaluate_map_with_context(
        config::configs::get(),
        source,
        trigger,
        &detail.artifact_state,
        &super::artifact_rules::MapContext {
            coin: run.coin,
            eligible_items: eligible_collections(run),
            seed: run.seed as u64
                ^ (run.layer_id as u64).rotate_left(32)
                ^ (run.node_id as u64).rotate_left(17)
                ^ (run.retry_num as u64).rotate_left(43),
        },
    );
    let coin = if change.coin > 0 {
        super::genius_bonuses::scale(change.coin, bonuses(run).coin_percent)
    } else {
        change.coin
    };
    run.coin = run.coin.saturating_add(coin).max(0);
    run.power = run
        .power
        .saturating_add(change.power)
        .clamp(0, run.power_limit);
    if change.heal_per_mille > 0 {
        heal_all(run, change.heal_per_mille)?;
    }
    if !change.consume_uids.is_empty() {
        bag.items
            .retain(|item| !change.consume_uids.contains(&item.uid));
    }
    for replacement in change.replace_items {
        super::artifact_lifecycle::replace(&mut bag, replacement.uid, replacement.item_id)?;
    }
    for id in change.grant_items {
        bag.add(id)?;
    }
    run.bag = serde_json::to_string(&bag)?;
    detail.artifact_state = change.state;
    run.details = serde_json::to_string(&detail)?;
    sync_artifact_limit(run)?;
    Ok(())
}

pub fn after_victory(run: &mut RougeRun, event_type: i32) -> Result<(), AppError> {
    let bonus = bonuses(run);
    let (heal, power) = match event_type {
        1 => (bonus.minor_heal, 0),
        2 => (bonus.danger_heal, bonus.danger_power),
        _ => (0, 0),
    };
    if heal > 0 {
        heal_all(run, heal)?;
    }
    run.power = run.power.saturating_add(power).min(run.power_limit);
    apply_map_effects(run, super::artifact_rules::MapTrigger::BattleWin)
}

pub fn shop_price(run: &RougeRun, original: i32) -> i32 {
    let reduction = serde_json::from_str::<bag::RunBag>(&run.bag)
        .ok()
        .map(|bag| {
            super::artifact_rules::evaluate_battle_with_state(
                config::configs::get(),
                &bag,
                run.coin,
                &details(run).artifact_state,
            )
        })
        .map(|effects| {
            effects.shop_price_reduction_per_mille
                + if effects.shop_price_reduction_per_mille > 0 {
                    effects.sale_discount_bonus_per_mille
                } else {
                    0
                }
        })
        .unwrap_or(0)
        .clamp(0, 1000);
    (original.max(0) as i64 * (1000 - reduction) as i64 / 1000).min(i32::MAX as i64) as i32
}

/// Only advertise menu effects whose corresponding server actions exist.
pub fn effect_info(run: &RougeRun) -> Vec<sonettobuf::RougeEffectInfo> {
    let tables = config::configs::get();
    let detail = details(run);
    let mut ids = detail.effects;
    for id in detail.outside_genius.as_deref().unwrap_or_default() {
        if let Some(row) = tables.rouge_genius_branch.get(*id) {
            ids.extend(
                row.effects
                    .split('|')
                    .filter_map(|id| id.parse::<i32>().ok()),
            );
        }
    }
    ids.sort_unstable();
    ids.dedup();
    let mut grouped: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for id in ids {
        if let Some(effect) = tables
            .rouge_effect
            .get(id)
            .filter(|effect| matches!(effect.r#type, 20 | 21 | 28))
        {
            grouped.entry(effect.r#type).or_default().push(id);
        }
    }
    grouped
        .into_iter()
        .map(|(kind, ids)| sonettobuf::RougeEffectInfo {
            r#type: Some(kind),
            effect_id: ids,
        })
        .collect()
}

pub fn queue_level_recruit(run: &mut RougeRun, old_exp: i32) {
    if team_progress(run.season, run.team_exp).0 > team_progress(run.season, old_exp).0 {
        let at = run.battle_interactive.split('|').count() as i32;
        if run.battle_interactive.is_empty() {
            run.battle_interactive = "5".to_owned();
            run.interactive_index = 0;
        } else {
            run.battle_interactive.push_str("|5");
            if run.interactive_index < 0 {
                run.interactive_index = at;
            }
        }
    }
}

pub(super) fn heal_all(run: &mut RougeRun, rate: i32) -> Result<(), AppError> {
    let mut team: RougeTeamInfo = serde_json::from_str(&run.team)?;
    for life in &mut team.hero_life_list {
        if life.life.unwrap_or(0) > 0 {
            life.life = Some((life.life.unwrap_or(0) + rate).min(1000));
        }
    }
    run.team = serde_json::to_string(&team)?;
    Ok(())
}

/// Resolve automatic grants in order, stopping at an actual player selection.
pub fn advance(run: &mut RougeRun, start: usize) -> Result<(), AppError> {
    let tables = config::configs::get();
    let segments: Vec<String> = run
        .battle_interactive
        .split('|')
        .map(str::to_owned)
        .collect();
    run.interactive_index = -1;
    for (index, raw) in segments.iter().enumerate().skip(start) {
        if raw == "3#301" {
            heal_all(run, 500)?;
            continue;
        }
        if raw == "1#10610206" {
            apply_automatic_drop(run, 10610206)?;
            continue;
        }
        if let Some((kind, id)) = supported_interactive(raw) {
            if kind == INTERACT_DROP {
                if let Some(drop) = drop_state(run, id, tables) {
                    if drop.offers.len() == drop.select_count as usize {
                        apply_automatic_drop(run, id)?;
                        continue;
                    }
                } else if tables.rouge_drop.get(id).is_some() {
                    apply_automatic_drop(run, id)?;
                    continue;
                }
            }
        }
        if interactive_requires_response(raw, tables) {
            run.interactive_index = index as i32;
            break;
        }
    }
    Ok(())
}

/// Captured 3.9 base rewards supersede the older exported experience values.
/// Other drop rows retain their configured values (no universal multiplier is
/// inferred from one party's bonuses).
pub fn reward_exp(drop_id: i32) -> i32 {
    match drop_id {
        1001 => 12,
        1002 => 15,
        1003 => 18,
        1006 => 16,
        1009 => 40,
        1010 => 48,
        1012 => 50,
        1013 => 80,
        _ => config::configs::get()
            .rouge_drop
            .get(drop_id)
            .map(|row| row.exp.max(0))
            .unwrap_or(0),
    }
}

impl RougeManager {
    pub(super) async fn resolve_choice(
        self,
        db: &SqlitePool,
        season: i32,
        layer: i32,
        node: i32,
        event: i32,
        choice_id: i32,
    ) -> Result<RougeMapInfo, AppError> {
        let expected = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run = expected.clone();
        let tables = config::configs::get();
        let graph: map::LayerMap = serde_json::from_str(&run.graph)?;
        if season != 1
            || run.map_type != MAP_TYPE_NORMAL
            || run.layer_id != layer
            || run.node_id != node
            || run.terminal_state != 0
            || pending_interactive(&run, tables).is_some()
            || !graph
                .node(node)
                .is_some_and(|node| node.event_id == event && node.node_type != 0)
        {
            return Err(AppError::InvalidRequest);
        }
        let choice = tables
            .rouge_choices_for_event(event)
            .into_iter()
            .find(|choice| choice.id == choice_id)
            .ok_or(AppError::InvalidRequest)?;
        if !choice.version.is_empty() || !choice_is_unlocked(&run, choice) {
            return Err(AppError::InvalidRequest);
        }
        if super::special_events::fountain_choice_ids(event).is_some() {
            return self
                .resolve_fountain_choice(db, expected, event, choice_id)
                .await;
        }
        let mut chosen: HashMap<String, i32> = serde_json::from_str(&run.choices)?;
        if chosen.insert(node.to_string(), choice_id).is_some() {
            return Err(AppError::InvalidRequest);
        }
        run.choices = serde_json::to_string(&chosen)?;
        if let Some(super::special_events::ChoiceAction::ChangeCurrentEvent { .. }) =
            super::special_events::choice_action(event, choice_id)
        {
            let mut graph: map::LayerMap = serde_json::from_str(&run.graph)?;
            super::special_events::transition_current(tables, &mut graph, node, choice_id)
                .map_err(|_| AppError::InvalidRequest)?
                .ok_or(AppError::InvalidRequest)?;
            chosen.remove(&node.to_string());
            run.choices = serde_json::to_string(&chosen)?;
            let mut finished: Vec<i32> = json_ids(&run.finished);
            finished.retain(|id| *id != node);
            run.finished = serde_json::to_string(&finished)?;
            run.graph = serde_json::to_string(&graph)?;
            run.battle_interactive.clear();
            run.interactive_index = -1;
            run.interactive_refresh = 0;
            run.retry_num = 0;
            save_action(db, &expected, &mut run).await?;
            return Ok(map_info(&run));
        }
        run.battle_interactive = choice.interactive.clone();
        run.interactive_refresh = 0;
        match super::special_events::choice_action(event, choice_id) {
            Some(super::special_events::ChoiceAction::Reward(reward)) => {
                apply_special_reward(&mut run, reward)?;
                run.battle_interactive.clear();
            }
            Some(super::special_events::ChoiceAction::Finish) => run.battle_interactive.clear(),
            _ => {}
        }
        // Observed 3.9 choice cost differs from the unlock threshold (100).
        if choice_id == 10611002 {
            if run.coin < 60 {
                return Err(AppError::InvalidRequest);
            }
            run.coin -= 60;
            run.spent_coin += 60;
        }
        advance(&mut run, 0)?;
        queue_level_recruit(&mut run, expected.team_exp);
        run.middle_state = updated_entrust(
            &expected,
            EntrustChange {
                gained_coin: (run.coin - expected.coin).max(0),
                spent_coin: (expected.coin - run.coin).max(0),
                gained_power: (run.power - expected.power).max(0),
                spent_power: (expected.power - run.power).max(0),
                collections: collection_counts(&serde_json::from_str(&run.bag)?)
                    .values()
                    .sum::<i32>()
                    - collection_counts(&serde_json::from_str(&expected.bag)?)
                        .values()
                        .sum::<i32>(),
                finished_event: (run.interactive_index < 0).then_some(event),
                ..Default::default()
            },
        )?;
        super::artifact_lifecycle::prepare(&expected, &mut run)?;
        finish_node(&mut run)?;
        save_action(db, &expected, &mut run).await?;
        Ok(map_info(&run))
    }
    async fn resolve_fountain_choice(
        self,
        db: &SqlitePool,
        expected: RougeRun,
        event: i32,
        choice: i32,
    ) -> Result<RougeMapInfo, AppError> {
        use super::special_events::{self, FountainResolution, FountainSource};
        let mut run = expected.clone();
        let mut detail = details(&run);
        let outcome = match special_events::resolve_fountain(
            &mut detail.special_events,
            FountainSource {
                run_seed: run.seed as u64,
                layer_id: run.layer_id,
                node_id: run.node_id,
            },
            event,
            choice,
            run.coin,
        )
        .map_err(|_| AppError::InvalidRequest)?
        {
            FountainResolution::Replayed(_) => return Ok(map_info(&run)),
            FountainResolution::Apply(outcome) => outcome,
        };
        // Only the explicit net outcome is charged; the displayed eligibility
        // threshold is not an additional stake. Positive grants follow the
        // same outside-growth scaling as other event coin grants.
        if outcome.coin_delta < 0 {
            let cost = outcome.coin_delta.saturating_neg();
            if run.coin < cost {
                return Err(AppError::InvalidRequest);
            }
            run.coin -= cost;
            run.spent_coin = run.spent_coin.saturating_add(cost);
        } else {
            run.coin = run.coin.saturating_add(super::genius_bonuses::scale(
                outcome.coin_delta,
                bonuses(&run).coin_percent,
            ));
        }
        if let Some(id) = outcome.badge_choice.filter(|_| run.coin > expected.coin) {
            super::badges::record_fountain_gain(&mut detail.achievements, outcome.source, id);
        }
        run.details = serde_json::to_string(&detail)?;
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph)?;
        let transition = special_events::fountain_transition_current(
            config::configs::get(),
            &mut graph,
            &outcome,
        )
        .map_err(|_| AppError::InvalidRequest)?;
        let mut choices: HashMap<String, i32> = serde_json::from_str(&run.choices)?;
        let mut finished: Vec<i32> = json_ids(&run.finished);
        if transition.is_some() {
            choices.remove(&run.node_id.to_string());
            finished.retain(|id| *id != run.node_id);
        } else if choices.insert(run.node_id.to_string(), choice).is_some() {
            return Err(AppError::InvalidRequest);
        }
        run.choices = serde_json::to_string(&choices)?;
        run.finished = serde_json::to_string(&finished)?;
        run.graph = serde_json::to_string(&graph)?;
        run.battle_interactive.clear();
        run.interactive_index = -1;
        run.interactive_refresh = 0;
        run.retry_num = 0;
        run.middle_state = updated_entrust(
            &expected,
            EntrustChange {
                gained_coin: (run.coin - expected.coin).max(0),
                spent_coin: (expected.coin - run.coin).max(0),
                finished_event: transition.is_none().then_some(event),
                ..Default::default()
            },
        )?;
        super::artifact_lifecycle::prepare(&expected, &mut run)?;
        if transition.is_none() {
            finish_node(&mut run)?;
        }
        save_action(db, &expected, &mut run).await?;
        Ok(map_info(&run))
    }

    pub async fn compose(
        self,
        db: &SqlitePool,
        season: i32,
        recipe_id: i32,
        ids: &[i64],
    ) -> Result<sonettobuf::RougeItem, AppError> {
        let expected = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run = expected.clone();
        let recipe = config::configs::get()
            .rouge_synthesis
            .get(recipe_id)
            .ok_or(AppError::InvalidRequest)?;
        if season != 1
            || run.terminal_state != 0
            || run.map_type != MAP_TYPE_MIDDLE
            || pending_interactive(&run, config::configs::get()).is_some()
            || !recipe.version.is_empty()
            || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err(AppError::InvalidRequest);
        }
        let state = middle_state(&run);
        if !state.pieces.iter().any(|piece| {
            piece.index == Some(run.position_index)
                && config::configs::get()
                    .rouge_piece
                    .get(piece.id.unwrap_or_default())
                    .is_some_and(|piece| piece.entrust_type == 1)
        }) {
            return Err(AppError::InvalidRequest);
        }
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
        let mut needed: Vec<i32> = recipe
            .synthetics
            .split('#')
            .filter_map(|id| id.parse().ok())
            .collect();
        let mut offered = Vec::new();
        for id in ids {
            let item = bag.item(*id).ok_or(AppError::InvalidRequest)?;
            if collection_shapes::get(item.item_id).is_some_and(|shape| shape.unremovable) {
                return Err(AppError::InvalidRequest);
            }
            offered.push(item.item_id);
        }
        needed.sort_unstable();
        offered.sort_unstable();
        if offered != needed || needed.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        // Add before deleting: the new item's UID must be above every consumed
        // UID. Inlaid disks are returned rather than silently destroyed.
        let product = bag.add(recipe.product)?;
        let holes: Vec<(i64, i32)> = bag
            .items
            .iter()
            .filter(|item| ids.contains(&item.uid))
            .flat_map(|item| {
                item.hole_ids
                    .iter()
                    .copied()
                    .zip(item.holes.iter().copied())
            })
            .filter(|(_, item)| *item > 0)
            .collect();
        bag.items.retain(|item| !ids.contains(&item.uid));
        for (uid, item_id) in holes {
            let mut returned = bag.add(item_id)?;
            returned.uid = uid;
            *bag.items.last_mut().unwrap() = returned;
        }
        run.bag = serde_json::to_string(&bag)?;
        let mut detail = details(&run);
        *detail.compositions.entry(recipe.product).or_default() += 1;
        run.details = serde_json::to_string(&detail)?;
        save_action(db, &expected, &mut run).await?;
        Ok(bag::RunBag::to_client_item(&product))
    }

    pub async fn active_talent(
        self,
        db: &SqlitePool,
        season: i32,
        index: i32,
    ) -> Result<sonettobuf::RougeTalentTree, AppError> {
        let expected = rouge::get_run(db, self.player_id, season)
            .await?
            .ok_or(AppError::InvalidRequest)?;
        let mut run = expected.clone();
        let start = load_original(db, self.player_id, season).await?;
        let mut detail = details(&run);
        let tree = talent_tree(&start, &run);
        let index = usize::try_from(index).map_err(|_| AppError::InvalidRequest)?;
        if run.terminal_state != 0
            || talent_points(&run) < 1
            || index >= tree.rouge_talent.len()
            || detail
                .active_talents
                .iter()
                .any(|active| active / 2 == index / 2)
            || index >= 2
                && !detail
                    .active_talents
                    .iter()
                    .any(|active| active / 2 + 1 == index / 2)
            || pending_interactive(&run, config::configs::get()).is_some()
        {
            return Err(AppError::InvalidRequest);
        }
        let talent = config::configs::get()
            .rouge_style_talent
            .get(tree.rouge_talent[index].id.unwrap_or_default())
            .ok_or(AppError::InvalidRequest)?;
        detail.active_talents.push(index);
        // The export contains some 3.9 talent references without drop rows.
        // Captured fallback: Pomegranate talent 113 grants a random Legendary
        // artefact. Other missing rows stay explicit, without invented rewards.
        let raw = &talent.interactive;
        if let Some(drop_id) = raw.strip_prefix("1#").and_then(|id| id.parse::<i32>().ok()) {
            if let Some(row) = config::configs::get().rouge_drop.get(drop_id) {
                apply_automatic_drop(&mut run, row.id)?;
            } else if talent.id == 113 {
                let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
                let item_id = last_reward_drop(
                    config::configs::get(),
                    5,
                    run.seed as u64,
                    index as u64,
                    eligible_collections(&run).as_ref(),
                )
                .ok_or(AppError::InvalidRequest)?;
                let item = bag.add(item_id)?;
                let _ = bag.place(item.uid, -1, -1, 0);
                run.bag = serde_json::to_string(&bag)?;
            }
        }
        run.details = serde_json::to_string(&detail)?;
        save_action(db, &expected, &mut run).await?;
        Ok(talent_tree(&start, &run))
    }
}

/// Fixed-size grants do not open a selection UI in the captured client.
pub fn apply_automatic_drop(run: &mut RougeRun, drop_id: i32) -> Result<(), AppError> {
    let tables = config::configs::get();
    // This respite reward is referenced by 3.9 but absent from the export.
    if drop_id == 10610206 {
        run.team_exp = run.team_exp.saturating_add(super::genius_bonuses::scale(
            20,
            bonuses(run).essence_percent,
        ));
        return Ok(());
    }
    let row = tables
        .rouge_drop
        .get(drop_id)
        .ok_or(AppError::InvalidRequest)?;
    if let Some(drop) = drop_state(run, drop_id, tables) {
        if drop.offers.len() != drop.select_count as usize {
            return Err(AppError::InvalidRequest);
        }
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
        for id in drop.offers {
            let item = bag.add(id)?;
            if row.enter_bag == 1 {
                let _ = bag.place(item.uid, -1, -1, 0);
            }
        }
        run.bag = serde_json::to_string(&bag)?;
    }
    run.coin = run.coin.saturating_add(super::genius_bonuses::scale(
        drop_coin(row),
        bonuses(run).coin_percent,
    ));
    run.power = run
        .power
        .saturating_add(row.power)
        .clamp(0, run.power_limit);
    run.team_exp = run.team_exp.saturating_add(super::genius_bonuses::scale(
        row.exp.max(0),
        bonuses(run).essence_percent,
    ));
    Ok(())
}

pub fn apply_effect(run: &mut RougeRun, effect_id: i32) -> Result<(), AppError> {
    let tables = config::configs::get();
    let effect = tables
        .rouge_effect
        .get(effect_id)
        .ok_or(AppError::InvalidRequest)?;
    if effect.r#type == 30 {
        let mut detail = details(run);
        super::special_events::queue_effect(
            tables,
            &mut detail.special_events,
            super::special_events::EffectSource {
                layer_id: run.layer_id,
                node_id: run.node_id,
                origin_id: effect_id,
            },
            effect_id,
        )
        .map_err(|_| AppError::InvalidRequest)?;
        run.details = serde_json::to_string(&detail)?;
        place_special_events(run)?;
        return Ok(());
    }
    let value = effect.type_param.parse::<i32>().ok();
    match (effect.r#type, value) {
        (13, Some(id)) if collection_shapes::get(id).is_some() => {
            let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
            bag.add(id)?;
            run.bag = serde_json::to_string(&bag)?;
        }
        (14, Some(amount)) => {
            let amount = if amount > 0 {
                super::genius_bonuses::scale(amount, bonuses(run).coin_percent)
            } else {
                amount
            };
            run.coin = run.coin.saturating_add(amount).max(0);
        }
        (15, Some(amount)) => {
            let amount = if amount > 0 {
                super::genius_bonuses::scale(amount, bonuses(run).essence_percent)
            } else {
                amount
            };
            run.team_exp = run.team_exp.saturating_add(amount).max(0);
        }
        (16, Some(amount)) => run.power_limit = run.power_limit.saturating_add(amount).max(0),
        (17, Some(amount)) => {
            run.power = run.power.saturating_add(amount).clamp(0, run.power_limit)
        }
        _ => {}
    }
    let mut detail = details(run);
    detail.effects.push(effect_id);
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rouge::original_tests::{init_config, insert_hero, test_db};

    async fn started() -> (SqlitePool, RougeManager) {
        init_config();
        let db = test_db().await;
        insert_hero(&db, 1, 1, 3003).await;
        insert_hero(&db, 1, 2, 3051).await;
        let manager = RougeManager::new(1);
        manager
            .select_difficulty(&db, 1, 1, &[], false)
            .await
            .unwrap();
        manager.select_style(&db, 1, 1).await.unwrap();
        manager.select_heroes(&db, 1, &[3003], 0).await.unwrap();
        (db, manager)
    }

    #[test]
    fn battle_life_uses_the_saved_ratio_and_rejects_unrecruited_or_dead_heroes() {
        let mut fight = sonettobuf::Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    model_id: Some(3003),
                    current_hp: Some(1250),
                    attr: Some(sonettobuf::HeroAttribute {
                        hp: Some(1250),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut roster = RougeTeamInfo {
            hero_life_list: vec![RougeHeroLife {
                hero_id: Some(3003),
                life: Some(400),
            }],
            ..Default::default()
        };
        apply_team_life(&mut fight, &roster).unwrap();
        assert_eq!(
            fight.attacker.as_ref().unwrap().entitys[0].current_hp,
            Some(500)
        );
        roster.hero_life_list[0].life = Some(0);
        assert!(apply_team_life(&mut fight, &roster).is_err());
        roster.hero_life_list.clear();
        assert!(apply_team_life(&mut fight, &roster).is_err());
    }

    #[tokio::test]
    async fn refresh_spends_50_then_75_and_insufficient_funds_preserve_stock() {
        let (db, manager) = started().await;
        let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut run = expected.clone();
        let mut graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.node_id == run.node_id)
            .unwrap();
        node.node_type = 7;
        node.event_id = 107010;
        run.graph = serde_json::to_string(&graph).unwrap();
        sqlx::query(
            "UPDATE user_rouge_run SET graph = ?, coin = 125 WHERE user_id = 1 AND season = 1",
        )
        .bind(&run.graph)
        .execute(&db)
        .await
        .unwrap();
        manager
            .shop_refresh(&db, 1, 111, run.node_id, 107010)
            .await
            .unwrap();
        let first = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(first.coin, 75);
        assert_eq!(
            serde_json::from_str::<shop::ShopState>(&first.shop)
                .unwrap()
                .stock
                .len(),
            8
        );
        manager
            .shop_refresh(&db, 1, 111, run.node_id, 107010)
            .await
            .unwrap();
        let second = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!((second.coin, second.spent_coin), (0, 125));
        assert!(
            manager
                .shop_refresh(&db, 1, 111, run.node_id, 107010)
                .await
                .is_err()
        );
        assert_eq!(
            rouge::get_run(&db, 1, 1).await.unwrap().unwrap().shop,
            second.shop
        );
    }

    #[tokio::test]
    async fn five_areas_allow_choice_exits_and_authorize_the_ending_piece() {
        let (db, manager) = started().await;
        for (layer, next) in [
            (111, Some(122)),
            (122, Some(132)),
            (132, Some(142)),
            (142, Some(151)),
            (151, None),
        ] {
            let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
            assert_eq!(run.layer_id, layer);
            let graph: map::LayerMap = serde_json::from_str(&run.graph).unwrap();
            assert_eq!(
                graph.stages,
                match layer {
                    111 => 5,
                    122 => 6,
                    132 => 7,
                    142 => 8,
                    _ => 6,
                }
            );
            // A sanitized one-node map isolates the exit transition. Captured
            // treasure/rest ids deliberately differ from their shared menu ids.
            let (event, choice, kind) = if layer == 151 {
                (108014, 10810105, 8)
            } else {
                (105030, 10510101, 5)
            };
            let graph = map::LayerMap {
                layer_id: layer,
                stages: 1,
                nodes: vec![map::MapNode {
                    stage: 0,
                    node_id: run.node_id,
                    last: vec![],
                    next: vec![],
                    event_id: event,
                    node_type: kind,
                }],
            };
            sqlx::query("UPDATE user_rouge_run SET graph = ? WHERE user_id = 1 AND season = 1")
                .bind(serde_json::to_string(&graph).unwrap())
                .execute(&db)
                .await
                .unwrap();
            let result = manager
                .choice_event(&db, 1, layer, run.node_id, event, choice)
                .await
                .unwrap();
            assert_eq!(result.map_type, Some(2));
            assert!(result.cur_interactive.is_none());
            assert!(manager.select_drop(&db, 1, &[0]).await.is_err());
            let middle = result.middle_layer_info.unwrap();
            let mid = middle.middle_layer_id.unwrap();
            manager.piece_move(&db, 1, layer, mid, 1).await.unwrap();
            // Opening and reopening the crafting UI is not a one-shot grant.
            manager
                .piece_talk_select(&db, 1, layer, mid, 1, 1001)
                .await
                .unwrap();
            manager
                .piece_talk_select(&db, 1, layer, mid, 1, 1001)
                .await
                .unwrap();
            if let Some(next) = next {
                assert!(
                    manager
                        .leave_middle_layer(&db, 1, layer, mid, next)
                        .await
                        .is_err()
                );
                if mid == 141 {
                    let priest = middle
                        .piece_info
                        .iter()
                        .find(|piece| piece.id == Some(1410201))
                        .unwrap();
                    manager
                        .piece_move(&db, 1, layer, mid, priest.index.unwrap())
                        .await
                        .unwrap();
                    manager
                        .piece_talk_select(&db, 1, layer, mid, priest.index.unwrap(), 1410201)
                        .await
                        .unwrap();
                }
                manager.piece_move(&db, 1, layer, mid, -1).await.unwrap();
                manager
                    .leave_middle_layer(&db, 1, layer, mid, next)
                    .await
                    .unwrap();
            } else {
                let boss = middle
                    .piece_info
                    .iter()
                    .find(|piece| piece.id == Some(1510501))
                    .unwrap();
                manager
                    .piece_move(&db, 1, layer, mid, boss.index.unwrap())
                    .await
                    .unwrap();
                let map = manager
                    .piece_talk_select(&db, 1, layer, mid, boss.index.unwrap(), 1510501)
                    .await
                    .unwrap();
                assert_eq!(map.map_type, Some(2));
                let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
                assert_eq!(details(&run).end_episode, 2902317);
                assert_eq!(details(&run).end_id, 101);
                assert_eq!(
                    run.terminal_state, 0,
                    "selecting an ending does not win the fight"
                );
            }
        }
    }

    #[tokio::test]
    async fn upgrade_recruits_on_an_ordinary_node_and_talents_have_sibling_gates() {
        let (db, manager) = started().await;
        sqlx::query("UPDATE user_rouge_run SET team_exp = 19, battle_interactive = '12#1001', interactive_index = 0 WHERE user_id = 1 AND season = 1")
            .execute(&db).await.unwrap();
        let (map, _, _, _) = manager.select_drop(&db, 1, &[0]).await.unwrap();
        assert_eq!(map.cur_interactive.as_deref(), Some("5"));
        let info = manager
            .original_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        assert_eq!(
            (
                info.team_level,
                info.team_exp,
                info.team_size,
                info.talent_point
            ),
            (Some(2), Some(11), Some(17), Some(1))
        );
        assert_eq!(info.talent_tree.unwrap().rouge_talent.len(), 18);
        let (_, map) = manager.recruit_heroes(&db, 1, &[3051]).await.unwrap();
        assert_eq!(map.map_type, Some(1));
        assert!(map.cur_interactive.is_none());
        assert!(manager.recruit_heroes(&db, 1, &[]).await.is_err());
        assert!(manager.active_talent(&db, 1, 2).await.is_err());
        let tree = manager.active_talent(&db, 1, 0).await.unwrap();
        assert_eq!(tree.rouge_talent[0].is_active, Some(1));
        assert!(manager.active_talent(&db, 1, 0).await.is_err());
        assert!(manager.active_talent(&db, 1, 1).await.is_err());
        assert_eq!(
            manager
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap()
                .talent_tree,
            Some(tree)
        );
    }

    #[tokio::test]
    async fn composition_checks_material_instances_and_preserves_recipe_history() {
        let (db, manager) = started().await;
        let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut run = expected.clone();
        run.map_type = 2;
        run.middle_layer_id = 111;
        run.position_index = 1;
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        let a = bag.add(8150014).unwrap();
        let b = bag.add(8150014).unwrap();
        run.bag = serde_json::to_string(&bag).unwrap();
        save(&db, &expected, &run).await.unwrap();
        assert!(manager.compose(&db, 1, 13, &[a.uid, a.uid]).await.is_err());
        assert!(manager.compose(&db, 1, 14, &[a.uid, b.uid]).await.is_err());
        assert_eq!(
            rouge::get_run(&db, 1, 1).await.unwrap().unwrap().bag,
            run.bag
        );
        let product = manager.compose(&db, 1, 13, &[a.uid, b.uid]).await.unwrap();
        assert_eq!(product.item_id, Some(8250015));
        assert!(product.id.unwrap() > b.uid);
        assert!(manager.compose(&db, 1, 13, &[a.uid, b.uid]).await.is_err());
        let run = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        assert_eq!(details(&run).compositions.get(&8250015), Some(&1));
        let bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        assert!(bag.item(a.uid).is_none() && bag.item(b.uid).is_none());
    }

    #[tokio::test]
    async fn captured_score_components_total_8990_and_rewards_are_credited_once() {
        let (db, manager) = started().await;
        let expected = rouge::get_run(&db, 1, 1).await.unwrap().unwrap();
        let mut run = expected.clone();
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        for _ in 0..45 {
            bag.add(8130804).unwrap();
        }
        run.bag = serde_json::to_string(&bag).unwrap();
        run.details = serde_json::to_string(&Details {
            end_id: 101,
            compositions: BTreeMap::from([(8250015, 1), (8250017, 1), (8310810, 1)]),
            consume_power: 48,
            ..Default::default()
        })
        .unwrap();
        run.middle_state = serde_json::to_string(&MiddleState {
            pieces: vec![],
            middle_layer_id: 151,
            entrust_id: 0,
            entrust_progress: 0,
            completed_entrusts: vec![1310233, 1110207],
        })
        .unwrap();
        run.spent_coin = 1160;
        run.terminal_state = 1;
        save(&db, &expected, &run).await.unwrap();
        sqlx::query("UPDATE user_rouge_run SET layer_index = 4, minor_count = 3, danger_count = 5, crisis_count = 2, step_count = 27 WHERE user_id = 1 AND season = 1").execute(&db).await.unwrap();
        sqlx::query(
            "UPDATE user_rouge_start SET genius_remainder = 410 WHERE user_id = 1 AND season = 1",
        )
        .execute(&db)
        .await
        .unwrap();
        let (info, result) = manager.end(&db, 1).await.unwrap();
        assert_eq!(info.state, Some(5));
        assert_eq!(result.final_score, Some(8990));
        assert_eq!(result.before_score, Some(8990));
        assert_eq!(result.collection2_score.as_deref(), Some("46#920"));
        assert_eq!(result.end2_score.as_deref(), Some("101#400"));
        assert_eq!(result.badge2_score.as_deref(), Some("2#150|3#150|5#150"));
        assert_eq!(result.add_point, Some(3));
        assert_eq!(result.remain_score2_point, Some(1490));
        assert_eq!(result.add_genius_point, Some(13));
        assert_eq!(result.remain_score2_genius_point, Some(300));
        assert_eq!(manager.end(&db, 1).await.unwrap(), (info, result));
        let outside = manager
            .original_outside_info(&db, 1)
            .await
            .unwrap()
            .rouge_info
            .unwrap();
        assert_eq!(outside.point, Some(11)); // 3 score rewards + 8 Craftmaster's Favors.
        assert_eq!(outside.genius_point, Some(13));
        assert_eq!(
            manager
                .original_info(&db, 1)
                .await
                .unwrap()
                .rouge_info
                .unwrap()
                .state,
            Some(2)
        );
    }
}
