// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RogueCollectionBackpack {
    pub id: i32,
    #[serde(rename = "initialCollection")]
    pub initial_collection: String,
    pub layout: String,
}
use std::collections::HashMap;

pub struct RogueCollectionBackpackTable {
    records: Vec<RogueCollectionBackpack>,
    by_id: HashMap<i32, usize>,
}

impl RogueCollectionBackpackTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RogueCollectionBackpack> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RogueCollectionBackpack> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RogueCollectionBackpack] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RogueCollectionBackpack> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}