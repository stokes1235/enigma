// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeReward {
    pub icon: String,
    pub id: i32,
    pub offset: String,
    pub pos: String,
    #[serde(rename = "preId")]
    pub pre_id: i32,
    #[serde(rename = "rewardName")]
    pub reward_name: String,
    #[serde(rename = "rewardType")]
    pub reward_type: i32,
    pub season: i32,
    pub stage: i32,
    #[serde(rename = "type")]
    pub r#type: i32,
    pub value: String,
}
use std::collections::HashMap;

pub struct RougeRewardTable {
    records: Vec<RougeReward>,
    by_id: HashMap<i32, usize>,
}

impl RougeRewardTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeReward> = crate::load_rows(path)?;

        let mut by_id = HashMap::with_capacity(records.len());

        for (idx, record) in records.iter().enumerate() {
            by_id.insert(record.id, idx);
        }

        Ok(Self {
            records,
            by_id,
        })
    }

    #[inline]
    pub fn get(&self, id: i32) -> Option<&RougeReward> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeReward] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeReward> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}