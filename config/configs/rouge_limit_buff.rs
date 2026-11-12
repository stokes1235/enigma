// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLimitBuff {
    pub blank: i32,
    #[serde(rename = "buffType")]
    pub buff_type: i32,
    pub cd: i32,
    pub desc: String,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "initCollections")]
    pub init_collections: String,
    #[serde(rename = "needEmblem")]
    pub need_emblem: i32,
    #[serde(rename = "startView")]
    pub start_view: String,
    pub title: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeLimitBuffTable {
    records: Vec<RougeLimitBuff>,
    by_id: HashMap<i32, usize>,
}

impl RougeLimitBuffTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLimitBuff> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeLimitBuff> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeLimitBuff] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLimitBuff> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}