//! Golden Alley permanent rewards. The client buys every unclaimed ancestor
//! up to the selected cell, and grants each stage's big reward for no points.
use super::*;
use crate::reward::{RewardManager, RewardSet, RewardedReply};
use common::time::ServerTime;
use sonettobuf::{RougeOutsideBonusStageNo, RougeReceivePointBonusReply};
use std::collections::BTreeSet;

pub struct Claim {
    pub outcome: RewardedReply<RougeReceivePointBonusReply>,
    pub point: i32,
}

fn open(stage: &config::rouge_reward_stage::RougeRewardStage, now: i64) -> bool {
    stage.stage <= 2
        || ServerTime::config_datetime_sec(&stage.open_time)
            .is_some_and(|time| i64::from(time) * 1000 <= now)
}

fn stage_ids(season: i32) -> Vec<i32> {
    let mut ids: Vec<_> = config::configs::get()
        .rouge_reward_stage
        .iter()
        .filter(|row| row.season == season)
        .map(|row| row.stage)
        .collect();
    ids.sort_unstable();
    ids
}

fn last_open_stage(season: i32, now: i64) -> i32 {
    config::configs::get()
        .rouge_reward_stage
        .iter()
        .filter(|row| row.season == season && open(row, now))
        .map(|row| row.stage)
        .max()
        .unwrap_or(0)
}

pub async fn bonus_info(
    db: &SqlitePool,
    user: i64,
    season: i32,
) -> Result<RougeOutsideBonusNo, AppError> {
    let claims: Vec<(i32, i32)> = sqlx::query_as(
        "SELECT stage, bonus_id FROM user_rouge_reward_claims
         WHERE user_id = ? AND season = ? ORDER BY stage, bonus_id",
    )
    .bind(user)
    .bind(season)
    .fetch_all(db)
    .await?;
    let seen: Option<i32> = sqlx::query_scalar(
        "SELECT seen_stage FROM user_rouge_reward_meta WHERE user_id = ? AND season = ?",
    )
    .bind(user)
    .bind(season)
    .fetch_optional(db)
    .await?;
    Ok(RougeOutsideBonusNo {
        bonus_stages: stage_ids(season)
            .into_iter()
            .map(|stage| RougeOutsideBonusStageNo {
                stage: Some(stage),
                bonus_ids: claims
                    .iter()
                    .filter(|(s, _)| *s == stage)
                    .map(|(_, id)| *id)
                    .collect(),
            })
            .collect(),
        is_new_stage: Some(last_open_stage(season, ServerTime::now_ms()) > seen.unwrap_or(0)),
    })
}

fn pending_chain(
    tables: &config::GameDB,
    season: i32,
    stage: i32,
    target: i32,
    claimed: &BTreeSet<i32>,
) -> Result<Vec<i32>, AppError> {
    let mut id = target;
    let mut visited = BTreeSet::new();
    let mut pending = Vec::new();
    while id != 0 && !claimed.contains(&id) {
        if !visited.insert(id) {
            return Err(AppError::InvalidRequest);
        }
        let row = tables
            .rouge_reward
            .get(id)
            .filter(|row| {
                row.season == season && row.stage == stage && matches!(row.r#type, 0 | 2 | 3)
            })
            .ok_or(AppError::InvalidRequest)?;
        pending.push(id);
        id = row.pre_id;
    }
    pending.reverse();
    Ok(pending)
}

impl RougeManager {
    pub async fn mark_bonus_stage(self, db: &SqlitePool, season: i32) -> Result<(), AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        sqlx::query(
            "INSERT INTO user_rouge_reward_meta (user_id, season, seen_stage) VALUES (?, ?, ?)
             ON CONFLICT (user_id, season) DO UPDATE SET seen_stage = MAX(seen_stage, excluded.seen_stage)",
        ).bind(self.player_id).bind(season)
            .bind(last_open_stage(season, ServerTime::now_ms())).execute(db).await?;
        Ok(())
    }

    pub async fn receive_point_bonus(
        self,
        db: &SqlitePool,
        season: i32,
        bonus_id: i32,
    ) -> Result<Claim, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let tables = config::configs::get();
        let target = tables
            .rouge_reward
            .get(bonus_id)
            .filter(|row| row.season == season)
            .ok_or(AppError::InvalidRequest)?;
        let stage = tables
            .rouge_reward_stage
            .iter()
            .find(|row| row.season == season && row.stage == target.stage)
            .filter(|row| open(row, ServerTime::now_ms()))
            .ok_or(AppError::InvalidRequest)?;
        rouge::get_or_create_outside(db, self.player_id, season).await?;
        let mut tx = db.begin().await?;
        // Acquire SQLite's writer lock before reading point/claims. Concurrent
        // requests must never validate against the same pre-payment snapshot.
        sqlx::query("UPDATE user_rouge_outside SET updated_at = updated_at WHERE user_id = ? AND season = ?")
            .bind(self.player_id).bind(season).execute(&mut *tx).await?;
        let point: i32 = sqlx::query_scalar(
            "SELECT point FROM user_rouge_outside WHERE user_id = ? AND season = ?",
        )
        .bind(self.player_id)
        .bind(season)
        .fetch_one(&mut *tx)
        .await?;
        let mut claimed: BTreeSet<i32> = sqlx::query_scalar::<_, i32>(
            "SELECT bonus_id FROM user_rouge_reward_claims WHERE user_id = ? AND season = ?",
        )
        .bind(self.player_id)
        .bind(season)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        if stage.pre_stage > 0
            && tables
                .rouge_reward
                .iter()
                .filter(|row| row.season == season && row.stage == stage.pre_stage)
                .any(|row| !claimed.contains(&row.id))
        {
            return Err(AppError::InvalidRequest);
        }
        let pending = if claimed.contains(&bonus_id) {
            Vec::new()
        } else if target.r#type == 1 {
            // Each row's terminal cell has type 2. All terminals must be claimed
            // before the free big reward; their predecessors were paid in order.
            if tables
                .rouge_reward
                .iter()
                .filter(|row| row.season == season && row.stage == stage.stage && row.r#type == 2)
                .any(|row| !claimed.contains(&row.id))
            {
                return Err(AppError::InvalidRequest);
            }
            vec![bonus_id]
        } else {
            pending_chain(tables, season, stage.stage, bonus_id, &claimed)?
        };
        let fields: Vec<i32> = tables
            .rouge_outside_const
            .iter()
            .find(|row| row.season == season && row.id == 2)
            .ok_or(AppError::InvalidRequest)?
            .value
            .split('#')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| AppError::InvalidRequest)?;
        if fields.len() != 3 || fields[0] != 2 || fields[1] != 1301 || fields[2] <= 0 {
            return Err(AppError::InvalidRequest);
        }
        let count = if target.r#type == 1 {
            0
        } else {
            pending.len() as i32
        };
        let cost = fields[2]
            .checked_mul(count)
            .ok_or(AppError::InvalidRequest)?;
        if point < cost {
            return Err(AppError::InsufficientCurrency);
        }
        let mut rewards = RewardSet::default();
        for id in pending {
            let row = tables
                .rouge_reward
                .get(id)
                .ok_or(AppError::InvalidRequest)?;
            rewards.extend(crate::reward::parse_strict(&row.value)?);
            sqlx::query("INSERT INTO user_rouge_reward_claims (user_id, season, stage, bonus_id, claimed_at) VALUES (?, ?, ?, ?, ?)")
                .bind(self.player_id).bind(season).bind(stage.stage).bind(id).bind(ServerTime::now_ms())
                .execute(&mut *tx).await?;
            claimed.insert(id);
        }
        sqlx::query("UPDATE user_rouge_outside SET point = point - ?, updated_at = ? WHERE user_id = ? AND season = ?")
            .bind(cost).bind(ServerTime::now_ms()).bind(self.player_id).bind(season).execute(&mut *tx).await?;
        let material_changes = rewards.material_changes();
        let rewards = RewardManager::new(self.player_id)
            .apply_in_transaction(&mut tx, db, rewards)
            .await?;
        tx.commit().await?;
        Ok(Claim {
            point: point - cost,
            outcome: RewardedReply {
                reply: RougeReceivePointBonusReply {
                    season: Some(season),
                    bonus_id: Some(bonus_id),
                    bonus_stage: Some(RougeOutsideBonusStageNo {
                        stage: Some(stage.stage),
                        bonus_ids: tables
                            .rouge_reward
                            .iter()
                            .filter(|row| {
                                row.season == season
                                    && row.stage == stage.stage
                                    && claimed.contains(&row.id)
                            })
                            .map(|row| row.id)
                            .collect(),
                    }),
                },
                rewards,
                material_changes,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rouge::original_tests::{init_config, test_db};

    async fn funded(point: i32) -> (SqlitePool, RougeManager) {
        init_config();
        let db = test_db().await;
        rouge::get_or_create_outside(&db, 1, 1).await.unwrap();
        sqlx::query(
            "UPDATE user_rouge_outside SET point = ?, have_get_point = ? WHERE user_id = 1",
        )
        .bind(point)
        .bind(point)
        .execute(&db)
        .await
        .unwrap();
        (db, RougeManager::new(1))
    }

    #[tokio::test]
    async fn selecting_a_later_cell_pays_only_missing_predecessors_and_replay_is_free() {
        let (db, player) = funded(10).await;
        let claim = player.receive_point_bonus(&db, 1, 113).await.unwrap();
        assert_eq!(claim.point, 7);
        assert_eq!(
            claim.outcome.reply.bonus_stage.unwrap().bonus_ids,
            vec![111, 112, 113]
        );
        let item: i32 =
            sqlx::query_scalar("SELECT quantity FROM items WHERE user_id = 1 AND item_id = 110103")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(item, 4);
        let again = player.receive_point_bonus(&db, 1, 113).await.unwrap();
        assert_eq!(again.point, 7);
        assert!(again.outcome.material_changes.is_empty());
        let last = player.receive_point_bonus(&db, 1, 114).await.unwrap();
        assert_eq!(last.point, 6);
        assert_eq!(
            rouge::get_or_create_outside(&db, 1, 1)
                .await
                .unwrap()
                .have_get_point,
            10
        );
        let info = bonus_info(&db, 1, 1).await.unwrap();
        assert_eq!(info.bonus_stages[0].bonus_ids, vec![111, 112, 113, 114]);
        assert!(info.bonus_stages[1].bonus_ids.is_empty());
        assert!(
            bonus_info(&db, 2, 1)
                .await
                .unwrap()
                .bonus_stages
                .iter()
                .all(|s| s.bonus_ids.is_empty())
        );
    }

    #[tokio::test]
    async fn insufficient_balance_and_locked_stage_do_not_grant_or_consume() {
        let (db, player) = funded(2).await;
        assert!(player.receive_point_bonus(&db, 1, 114).await.is_err());
        assert!(player.receive_point_bonus(&db, 1, 211).await.is_err());
        assert!(player.receive_point_bonus(&db, 2, 111).await.is_err());
        assert!(player.receive_point_bonus(&db, 1, -1).await.is_err());
        assert!(player.receive_point_bonus(&db, 1, 100).await.is_err());
        assert_eq!(
            rouge::get_or_create_outside(&db, 1, 1).await.unwrap().point,
            2
        );
        assert!(
            bonus_info(&db, 1, 1)
                .await
                .unwrap()
                .bonus_stages
                .iter()
                .all(|s| s.bonus_ids.is_empty())
        );
    }

    #[tokio::test]
    async fn big_reward_is_free_and_required_before_the_next_stage() {
        let (db, player) = funded(20).await;
        for id in [114, 124, 134, 144] {
            player.receive_point_bonus(&db, 1, id).await.unwrap();
        }
        assert!(player.receive_point_bonus(&db, 1, 211).await.is_err());
        let big = player.receive_point_bonus(&db, 1, 100).await.unwrap();
        assert_eq!(big.point, 4);
        assert_eq!(big.outcome.rewards.hero_ids, vec![3091]);
        let again = player.receive_point_bonus(&db, 1, 100).await.unwrap();
        assert!(again.outcome.rewards.hero_ids.is_empty());
        let next = player.receive_point_bonus(&db, 1, 211).await.unwrap();
        assert_eq!(next.point, 3);
    }

    #[tokio::test]
    async fn an_inventory_failure_rolls_back_the_entire_chain_and_payment() {
        let (db, player) = funded(10).await;
        sqlx::raw_sql("CREATE TRIGGER reject_test_reward BEFORE INSERT ON items WHEN NEW.item_id = 110104 BEGIN SELECT RAISE(ABORT, 'test grant failure'); END;")
            .execute(&db).await.unwrap();
        assert!(player.receive_point_bonus(&db, 1, 113).await.is_err());
        assert_eq!(
            rouge::get_or_create_outside(&db, 1, 1).await.unwrap().point,
            10
        );
        let claims: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_rouge_reward_claims")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(claims, 0);
        let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE user_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(items, 0);
        let currencies: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM currencies WHERE user_id = 1")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(currencies, 0);
    }

    #[tokio::test]
    async fn stage_notice_acknowledgement_survives_reloading() {
        let (db, player) = funded(1).await;
        assert_eq!(
            bonus_info(&db, 1, 1).await.unwrap().is_new_stage,
            Some(true)
        );
        player.mark_bonus_stage(&db, 1).await.unwrap();
        player.mark_bonus_stage(&db, 1).await.unwrap();
        assert_eq!(
            bonus_info(&db, 1, 1).await.unwrap().is_new_stage,
            Some(false)
        );
        assert_eq!(
            bonus_info(&db, 2, 1).await.unwrap().is_new_stage,
            Some(true)
        );
        assert!(player.mark_bonus_stage(&db, 2).await.is_err());
    }

    #[tokio::test]
    async fn all_configured_stages_can_be_claimed_for_exactly_the_normal_cell_cost() {
        let (db, player) = funded(224).await;
        for stage in stage_ids(1) {
            let table = config::configs::get();
            for row in table
                .rouge_reward
                .iter()
                .filter(|row| row.stage == stage && row.r#type == 2)
            {
                player.receive_point_bonus(&db, 1, row.id).await.unwrap();
            }
            let big = table
                .rouge_reward
                .iter()
                .find(|row| row.stage == stage && row.r#type == 1)
                .unwrap();
            player.receive_point_bonus(&db, 1, big.id).await.unwrap();
        }
        let outside = rouge::get_or_create_outside(&db, 1, 1).await.unwrap();
        assert_eq!(outside.point, 0);
        assert_eq!(outside.have_get_point, 224);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_rouge_reward_claims WHERE user_id = 1")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(count as usize, config::configs::get().rouge_reward.len());
    }

    #[tokio::test]
    async fn concurrent_duplicate_claims_never_charge_or_grant_twice() {
        let (db, player) = funded(10).await;
        let (a, b) = tokio::join!(
            player.receive_point_bonus(&db, 1, 113),
            player.receive_point_bonus(&db, 1, 113),
        );
        // A contending SQLite connection may ask the caller to retry; the
        // committed chain remains all-or-nothing and that retry is idempotent.
        assert!(a.is_ok() || b.is_ok());
        let retry = player.receive_point_bonus(&db, 1, 113).await.unwrap();
        assert_eq!(retry.point, 7);
        let quantity: i32 =
            sqlx::query_scalar("SELECT quantity FROM items WHERE user_id = 1 AND item_id = 110103")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(quantity, 4);
    }

    #[test]
    fn reward_stage_dates_follow_the_existing_server_clock() {
        init_config();
        let stage = config::configs::get()
            .rouge_reward_stage
            .iter()
            .find(|s| s.stage == 3)
            .unwrap();
        let at = i64::from(ServerTime::config_datetime_sec(&stage.open_time).unwrap()) * 1000;
        assert!(!open(stage, at - 1));
        assert!(open(stage, at));
    }
}
