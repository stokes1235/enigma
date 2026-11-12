// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLimitUnlock {
    pub id: i32,
    #[serde(rename = "skillId")]
    pub skill_id: i32,
    pub style: i32,
    #[serde(rename = "unlockCost")]
    pub unlock_cost: i32,
    pub version: i32,
}
use std::collections::HashMap;

pub struct RougeLimitUnlockTable {
    records: Vec<RougeLimitUnlock>,
    by_id: HashMap<i32, usize>,
}

impl RougeLimitUnlockTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLimitUnlock> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeLimitUnlock> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeLimitUnlock] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLimitUnlock> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}