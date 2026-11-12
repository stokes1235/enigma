// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeMapSkill {
    #[serde(rename = "coinCost")]
    pub coin_cost: i32,
    pub desc: String,
    pub effects: String,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "middleLayerLimit")]
    pub middle_layer_limit: i32,
    #[serde(rename = "powerCost")]
    pub power_cost: i32,
    #[serde(rename = "stepCd")]
    pub step_cd: i32,
    #[serde(rename = "useLimit")]
    pub use_limit: i32,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeMapSkillTable {
    records: Vec<RougeMapSkill>,
    by_id: HashMap<i32, usize>,
}

impl RougeMapSkillTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeMapSkill> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeMapSkill> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeMapSkill] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeMapSkill> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}