use crate::error::AppError;
use database::db::game::rouge;
use sonettobuf::{
    GetRouge2InfoReply, GetRouge2OutsideInfoReply, GetRougeInfoReply, GetRougeOutsideInfoReply,
    Rouge2AlchemyInfo, Rouge2AlchemyMaterialInfo, Rouge2AttrInfo, Rouge2BagInfo,
    Rouge2BossBattleInfo, Rouge2CareerLevelInfo, Rouge2GetUnlockCollectionsReply, Rouge2Info,
    Rouge2OutsideInfo, Rouge2RewardInfo, Rouge2TotalRecordInfo, RougeBag, RougeGameRecordInfo,
    RougeInfo, RougeOutsideBonusNo, RougeOutsideInfo, RougeTeamInfo, RougeWarehouse,
};
use sqlx::SqlitePool;

#[derive(Clone, Copy, Debug)]
pub struct RougeManager {
    player_id: i64,
}

impl RougeManager {
    pub fn new(player_id: i64) -> Self {
        Self { player_id }
    }

    /// Initial original-Rouge setup. Later exploration is deliberately not implemented yet.
    pub async fn original_info(
        self,
        db: &SqlitePool,
        season: i32,
    ) -> Result<GetRougeInfoReply, AppError> {
        Ok(GetRougeInfoReply {
            rouge_info: Some(load_original(db, self.player_id, season).await?),
            init_hero_ids: vec![],
        })
    }

    pub async fn select_difficulty(
        self,
        db: &SqlitePool,
        season: i32,
        difficulty: i32,
        version: &[i32],
        has_limiter: bool,
    ) -> Result<RougeInfo, AppError> {
        // Only the initially unlocked base-game difficulty is supported for now.
        if season != 1 || difficulty != 1 || !version.is_empty() || has_limiter {
            return Err(AppError::InvalidRequest);
        }
        load_original(db, self.player_id, season).await?;
        let result = sqlx::query("UPDATE user_rouge_start SET state = 1, difficulty = ? WHERE user_id = ? AND season = ? AND state = 0")
            .bind(difficulty).bind(self.player_id).bind(season).execute(db).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::InvalidRequest);
        }
        load_original(db, self.player_id, season).await
    }

    pub async fn abort_setup(self, db: &SqlitePool, season: i32) -> Result<RougeInfo, AppError> {
        if season != 1 {
            return Err(AppError::InvalidRequest);
        }
        let result = sqlx::query("UPDATE user_rouge_start SET state = 0, difficulty = 0, style = 0 WHERE user_id = ? AND season = ? AND state IN (1, 3)")
            .bind(self.player_id).bind(season).execute(db).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::InvalidRequest);
        }
        load_original(db, self.player_id, season).await
    }

    pub async fn select_style(
        self,
        db: &SqlitePool,
        season: i32,
        style: i32,
    ) -> Result<RougeInfo, AppError> {
        // These four styles are initially unlocked in the season-one base data.
        if season != 1 || !(1..=4).contains(&style) {
            return Err(AppError::InvalidRequest);
        }
        let result = sqlx::query("UPDATE user_rouge_start SET state = 3, style = ? WHERE user_id = ? AND season = ? AND state = 1")
            .bind(style).bind(self.player_id).bind(season).execute(db).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::InvalidRequest);
        }
        load_original(db, self.player_id, season).await
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

pub fn rouge_outside_info(season: i32) -> GetRougeOutsideInfoReply {
    GetRougeOutsideInfoReply {
        rouge_info: Some(RougeOutsideInfo {
            season: Some(season),
            genius_point: Some(0),
            genius_ids: Vec::new(),
            point: Some(0),
            have_get_point: Some(0),
            bonus: Some(RougeOutsideBonusNo::default()),
            review: Vec::new(),
            game_record_info: Some(RougeGameRecordInfo::default()),
            is_genius_new_stage: Some(false),
            limiter_info: None,
            cur_extra_point: Some(0),
        }),
    }
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

async fn load_original(db: &SqlitePool, user_id: i64, season: i32) -> Result<RougeInfo, AppError> {
    if season != 1 {
        return Err(AppError::InvalidRequest);
    }
    sqlx::query(
        "INSERT INTO user_rouge_start (user_id, season) VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(season)
    .execute(db)
    .await?;
    let (state, difficulty, style): (i32, i32, i32) = sqlx::query_as(
        "SELECT state, difficulty, style FROM user_rouge_start WHERE user_id = ? AND season = ?",
    )
    .bind(user_id)
    .bind(season)
    .fetch_one(db)
    .await?;
    // Values are from the base season's rouge_style table. Before selection they are zero.
    let (capacity, coin, power) = match style {
        1 => (15, 50, 40),
        2 => (20, 50, 50),
        3 | 4 => (15, 50, 50),
        _ => (0, 0, 0),
    };
    Ok(RougeInfo {
        season: Some(season),
        state: Some(state),
        difficulty: Some(difficulty),
        style: Some(style),
        team_size: Some(capacity),
        coin: Some(coin),
        power: Some(power),
        power_limit: Some(power),
        team_info: Some(RougeTeamInfo::default()),
        bag: Some(RougeBag::default()),
        warehouse: Some(RougeWarehouse::default()),
        ..Default::default()
    })
}

#[cfg(test)]
mod original_tests {
    use super::*;

    #[tokio::test]
    async fn setup_persists_and_rejects_out_of_order_or_unsupported_requests() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query("INSERT INTO users (id, username, created_at, updated_at) VALUES (1, 'rouge-test', 0, 0), (2, 'rouge-other', 0, 0)")
            .execute(&db).await.unwrap();
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
        assert!(player.select_style(&db, 1, 9).await.is_err());
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
}
