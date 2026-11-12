// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougePieceSelect {
    #[serde(rename = "activeParam")]
    pub active_param: String,
    #[serde(rename = "activeType")]
    pub active_type: i32,
    pub content: String,
    pub display: i32,
    pub id: i32,
    #[serde(rename = "talkDesc")]
    pub talk_desc: String,
    pub title: String,
    #[serde(rename = "triggerParam")]
    pub trigger_param: String,
    #[serde(rename = "triggerType")]
    pub trigger_type: i32,
    #[serde(rename = "unlockParam")]
    pub unlock_param: String,
    #[serde(rename = "unlockType")]
    pub unlock_type: i32,
}
use std::collections::HashMap;

pub struct RougePieceSelectTable {
    records: Vec<RougePieceSelect>,
    by_id: HashMap<i32, usize>,
}

impl RougePieceSelectTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougePieceSelect> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougePieceSelect> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougePieceSelect] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougePieceSelect> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}