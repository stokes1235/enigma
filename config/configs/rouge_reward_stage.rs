// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeRewardStage {
    #[serde(rename = "bigRewardId")]
    pub big_reward_id: i32,
    pub icon: String,
    pub jump: String,
    #[serde(rename = "lockName")]
    pub lock_name: String,
    pub name: String,
    #[serde(rename = "openTime")]
    pub open_time: String,
    #[serde(rename = "pointLimit")]
    pub point_limit: i32,
    #[serde(rename = "preStage")]
    pub pre_stage: i32,
    pub season: i32,
    pub stage: i32,
}
pub struct RougeRewardStageTable {
    records: Vec<RougeRewardStage>,
}

impl RougeRewardStageTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeRewardStage> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeRewardStage] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeRewardStage> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}