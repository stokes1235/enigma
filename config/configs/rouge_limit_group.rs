// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLimitGroup {
    pub desc: String,
    pub icon: String,
    pub id: i32,
    pub title: String,
    #[serde(rename = "unlockCondition")]
    pub unlock_condition: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeLimitGroupTable {
    records: Vec<RougeLimitGroup>,
    by_id: HashMap<i32, usize>,
}

impl RougeLimitGroupTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLimitGroup> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeLimitGroup> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeLimitGroup] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLimitGroup> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}