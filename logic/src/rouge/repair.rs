//! Safe-zone Mystic Barter and Voiceless Scales. Price magnitudes are from
//! rouge_repair_shop_price. Stock and penalty selection use a stable local
//! seed policy because the exported table does not expose the server sampler.
use super::*;
use rand::{Rng, SeedableRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Trigger {
    pub repair_shop_collections: BTreeMap<i32, i32>,
    pub cur_bought_repair_shop_collections: Vec<i32>,
    pub repair_random_num: i32,
    pub displace_num: i32,
    pub displace_max_num: i32,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

fn genius(run: &RougeRun, id: i32) -> bool {
    flow::details(run)
        .outside_genius
        .as_deref()
        .unwrap_or_default()
        .contains(&id)
}

fn stock(run: &RougeRun, index: i32, refresh: i32) -> BTreeMap<i32, i32> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(
        run.seed as u64
            ^ (run.middle_layer_id as u64).rotate_left(11)
            ^ (index as u64).rotate_left(25)
            ^ (refresh as u64).wrapping_mul(7919),
    );
    let tables = config::configs::get();
    let mut rows: Vec<_> = tables
        .rouge_repair_shop
        .iter()
        .filter(|row| {
            row.season == 1
                && row.consume == "1#200|2#200"
                && artifact_rules::base_item_ids().contains(&row.collection_id)
        })
        .collect();
    rows.shuffle(&mut rng);
    let family = if genius(run, 207) { 2100 } else { 2000 };
    let mut result = BTreeMap::new();
    for row in rows {
        // Only the two penalty families explicitly present in this export
        // are sampled. Other price IDs are accepted only if actually offered.
        result
            .entry(row.collection_id)
            .or_insert_with(|| family + rng.random_range(1..=2));
        if result.len() == 3 {
            break;
        }
    }
    result
}

pub(super) fn open(
    run: &RougeRun,
    piece: &mut sonettobuf::RougePieceInfo,
    selection: i32,
) -> Result<(), AppError> {
    if !genius(run, 201) {
        return Err(AppError::InvalidRequest);
    }
    let mut trigger: Trigger = piece
        .trigger_str
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    if selection == 1002 && trigger.repair_shop_collections.is_empty() {
        trigger.repair_shop_collections = stock(
            run,
            piece.index.unwrap_or_default(),
            trigger.repair_random_num,
        );
        if trigger.repair_shop_collections.len() != 3 {
            return Err(AppError::InvalidRequest);
        }
    }
    trigger.displace_max_num =
        const_i32(config::configs::get(), 19, 1) + i32::from(genius(run, 210));
    piece.select_id = Some(selection);
    piece.trigger_str = Some(serde_json::to_string(&trigger)?);
    Ok(())
}

async fn active(
    db: &SqlitePool,
    user: i64,
    season: i32,
    selection: i32,
) -> Result<(RougeRun, MiddleState, usize, Trigger), AppError> {
    let run = rouge::get_run(db, user, season)
        .await?
        .ok_or(AppError::InvalidRequest)?;
    if season != 1
        || run.map_type != MAP_TYPE_MIDDLE
        || run.terminal_state != 0
        || !genius(&run, 201)
        || pending_interactive(&run, config::configs::get()).is_some()
    {
        return Err(AppError::InvalidRequest);
    }
    let state = middle_state(&run);
    let index = state
        .pieces
        .iter()
        .position(|piece| {
            piece.index == Some(run.position_index)
                && piece.select_id == Some(selection)
                && piece.finish != Some(true)
        })
        .ok_or(AppError::InvalidRequest)?;
    let trigger = state.pieces[index]
        .trigger_str
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .ok_or(AppError::InvalidRequest)?;
    Ok((run, state, index, trigger))
}

fn penalty(run: &mut RougeRun, price: i32) -> Result<(), AppError> {
    match price {
        2001 | 2101 | 3001 | 3101 => {
            let amount = match price {
                2001 => 10,
                2101 => 8,
                3001 => 20,
                _ => 16,
            };
            run.power_limit = run.power_limit.saturating_sub(amount).max(0);
            run.power = run.power.min(run.power_limit);
        }
        2002 | 2102 | 3002 | 3102 => {
            let percent = match price {
                2002 => 40,
                2102 => 30,
                3002 => 80,
                _ => 60,
            };
            let mut team: RougeTeamInfo = serde_json::from_str(&run.team)?;
            // The exported text says lose X% HP without specifying the basis.
            // Use current HP, rounded down, preserving at least one life unit.
            for life in &mut team.hero_life_list {
                let old = life.life.unwrap_or_default();
                if old > 0 {
                    life.life = Some((old * (100 - percent) / 100).max(1));
                }
            }
            run.team = serde_json::to_string(&team)?;
        }
        2003 | 2103 | 3003 | 3103 => {
            let amount = match price {
                2003 => 20,
                2103 => 16,
                3003 => 40,
                _ => 32,
            };
            if run.power < amount {
                return Err(AppError::InvalidRequest);
            }
            run.power -= amount;
        }
        _ => return Err(AppError::InvalidRequest),
    }
    Ok(())
}

async fn commit(
    db: &SqlitePool,
    before: &RougeRun,
    mut after: RougeRun,
    mut state: MiddleState,
    index: usize,
    trigger: Trigger,
) -> Result<sonettobuf::RougePieceInfo, AppError> {
    state.pieces[index].trigger_str = Some(serde_json::to_string(&trigger)?);
    after.middle_state = serde_json::to_string(&state)?;
    let resources = after.clone();
    after.middle_state = updated_entrust(
        &resources,
        EntrustChange {
            spent_power: (before.power - after.power).max(0),
            collections: {
                let old = artifact_lifecycle::owned(&serde_json::from_str(&before.bag)?);
                artifact_lifecycle::owned(&serde_json::from_str(&after.bag)?)
                    .keys()
                    .filter(|id| !old.contains_key(id))
                    .count() as i32
            },
            ..Default::default()
        },
    )?;
    let mut detail = flow::details(&after);
    detail.consume_power = detail
        .consume_power
        .saturating_add((before.power - after.power).max(0));
    after.details = serde_json::to_string(&detail)?;
    flow::save_action(db, before, &mut after).await?;
    middle_state(&after)
        .pieces
        .get(index)
        .cloned()
        .ok_or(AppError::InvalidRequest)
}

impl RougeManager {
    pub async fn repair_buy(
        self,
        db: &SqlitePool,
        season: i32,
        collection: i32,
    ) -> Result<sonettobuf::RougePieceInfo, AppError> {
        let (run, state, index, mut trigger) = active(db, self.player_id, season, 1002).await?;
        if trigger
            .cur_bought_repair_shop_collections
            .contains(&collection)
        {
            return Err(AppError::InvalidRequest);
        }
        let price = *trigger
            .repair_shop_collections
            .get(&collection)
            .ok_or(AppError::InvalidRequest)?;
        let mut after = run.clone();
        penalty(&mut after, price)?;
        let mut bag: bag::RunBag = serde_json::from_str(&after.bag)?;
        bag.add(collection)?;
        after.bag = serde_json::to_string(&bag)?;
        trigger.cur_bought_repair_shop_collections.push(collection);
        let mut detail = flow::details(&after);
        detail.repair_shop_num = detail.repair_shop_num.saturating_add(1);
        after.details = serde_json::to_string(&detail)?;
        commit(db, &run, after, state, index, trigger).await
    }

    pub async fn repair_random(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<sonettobuf::RougePieceInfo, AppError> {
        let (run, state, index, mut trigger) = active(db, self.player_id, season, 1002).await?;
        if !genius(&run, 213)
            || trigger.repair_random_num >= const_i32(config::configs::get(), 13, 1)
        {
            return Err(AppError::InvalidRequest);
        }
        trigger.repair_random_num += 1;
        trigger.repair_shop_collections =
            stock(&run, run.position_index, trigger.repair_random_num);
        trigger.cur_bought_repair_shop_collections.clear();
        commit(db, &run, run.clone(), state, index, trigger).await
    }

    pub async fn displace(
        self,
        db: &SqlitePool,
        season: i32,
        uid: i64,
    ) -> Result<(sonettobuf::RougePieceInfo, i64), AppError> {
        let (run, state, index, mut trigger) = active(db, self.player_id, season, 1003).await?;
        if trigger.displace_num >= trigger.displace_max_num {
            return Err(AppError::InvalidRequest);
        }
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag)?;
        let item = bag.item(uid).cloned().ok_or(AppError::InvalidRequest)?;
        if collection_shapes::is_unique(item.item_id) {
            return Err(AppError::InvalidRequest);
        }
        // Exchange keeps all inserted Disks rather than silently deleting them.
        for (&id, &disk_uid) in item.holes.iter().zip(&item.hole_ids) {
            if id > 0 && disk_uid > 0 {
                bag.items.push(bag::BagItem {
                    uid: disk_uid,
                    item_id: id,
                    row: -1,
                    col: -1,
                    rotation: 0,
                    holes: vec![],
                    hole_ids: vec![],
                });
            }
        }
        bag.items.retain(|old| old.uid != uid);
        // No authoritative exchange pool is exported. The local policy draws
        // uniformly from other non-unique base items of the same rarity.
        let rare =
            artifact_rules::collection_rarity(item.item_id).ok_or(AppError::InvalidRequest)?;
        let pool: Vec<_> = artifact_rules::base_item_ids()
            .iter()
            .copied()
            .filter(|id| {
                *id != item.item_id
                    && !collection_shapes::is_unique(*id)
                    && artifact_rules::collection_rarity(*id) == Some(rare)
            })
            .collect();
        if pool.is_empty() {
            return Err(AppError::InvalidRequest);
        }
        let mut rng = rand::rngs::StdRng::seed_from_u64(
            run.seed as u64 ^ (uid as u64).rotate_left(13) ^ trigger.displace_num as u64,
        );
        let replacement = bag.add(pool[rng.random_range(0..pool.len())])?;
        let mut after = run.clone();
        after.bag = serde_json::to_string(&bag)?;
        trigger.displace_num += 1;
        let mut detail = flow::details(&after);
        detail.displace_num = detail.displace_num.saturating_add(1);
        after.details = serde_json::to_string(&detail)?;
        Ok((
            commit(db, &run, after, state, index, trigger).await?,
            replacement.uid,
        ))
    }
}
