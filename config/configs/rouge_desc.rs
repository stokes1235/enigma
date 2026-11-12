// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeDesc {
    pub desc: String,
    #[serde(rename = "descExtra")]
    pub desc_extra: String,
    #[serde(rename = "descExtraSimply")]
    pub desc_extra_simply: String,
    #[serde(rename = "descSimply")]
    pub desc_simply: String,
    #[serde(rename = "descType")]
    pub desc_type: i32,
    #[serde(rename = "effectId")]
    pub effect_id: i32,
    pub id: i32,
    pub name: String,
}
use std::collections::HashMap;

pub struct RougeDescTable {
    records: Vec<RougeDesc>,
    by_id: HashMap<i32, usize>,
}

impl RougeDescTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeDesc> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeDesc> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeDesc] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeDesc> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}