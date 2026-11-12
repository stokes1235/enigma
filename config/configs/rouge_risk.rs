// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeRisk {
    pub attr: String,
    #[serde(rename = "buffNum")]
    pub buff_num: i32,
    pub content: String,
    pub desc: String,
    pub id: i32,
    pub range: String,
    #[serde(rename = "scoreReward")]
    pub score_reward: i32,
    pub title: String,
    pub title_en: String,
    #[serde(rename = "viewDown")]
    pub view_down: i32,
}
use std::collections::HashMap;

pub struct RougeRiskTable {
    records: Vec<RougeRisk>,
    by_id: HashMap<i32, usize>,
}

impl RougeRiskTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeRisk> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeRisk> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeRisk] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeRisk> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}