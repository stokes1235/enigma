// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeCollectionUnlock {
    pub id: i32,
    #[serde(rename = "rareSort")]
    pub rare_sort: i32,
    #[serde(rename = "sortId")]
    pub sort_id: i32,
    #[serde(rename = "typeSort")]
    pub type_sort: i32,
    #[serde(rename = "unlockParam")]
    pub unlock_param: String,
    #[serde(rename = "unlockType")]
    pub unlock_type: i32,
}
use std::collections::HashMap;

pub struct RougeCollectionUnlockTable {
    records: Vec<RougeCollectionUnlock>,
    by_id: HashMap<i32, usize>,
}

impl RougeCollectionUnlockTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeCollectionUnlock> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeCollectionUnlock> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeCollectionUnlock] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeCollectionUnlock> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}