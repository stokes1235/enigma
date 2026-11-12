// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeRepairShop {
    #[serde(rename = "collectionId")]
    pub collection_id: i32,
    pub consume: String,
    pub id: i32,
    pub season: i32,
    #[serde(rename = "upConsume")]
    pub up_consume: String,
}
use std::collections::HashMap;

pub struct RougeRepairShopTable {
    records: Vec<RougeRepairShop>,
    by_id: HashMap<i32, usize>,
}

impl RougeRepairShopTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeRepairShop> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeRepairShop> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeRepairShop] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeRepairShop> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}