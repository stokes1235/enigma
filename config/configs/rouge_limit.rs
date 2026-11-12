// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLimit {
    pub desc: String,
    pub group: i32,
    pub id: i32,
    #[serde(rename = "initCollections")]
    pub init_collections: String,
    pub level: i32,
    #[serde(rename = "riskValue")]
    pub risk_value: i32,
    #[serde(rename = "startView")]
    pub start_view: String,
    pub title: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeLimitTable {
    records: Vec<RougeLimit>,
    by_id: HashMap<i32, usize>,
}

impl RougeLimitTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLimit> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeLimit> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeLimit] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLimit> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}