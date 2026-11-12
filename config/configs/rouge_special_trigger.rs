// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeSpecialTrigger {
    #[serde(rename = "dropGroupCorrectWeight")]
    pub drop_group_correct_weight: String,
    #[serde(rename = "eventCorrectWeight")]
    pub event_correct_weight: String,
    #[serde(rename = "eventGroupCorrectWeight")]
    pub event_group_correct_weight: String,
    pub id: i32,
    #[serde(rename = "inPictorial")]
    pub in_pictorial: i32,
    #[serde(rename = "isShow")]
    pub is_show: i32,
    pub name: String,
    #[serde(rename = "reasonRange")]
    pub reason_range: String,
    #[serde(rename = "shopGroupCorrectWeight")]
    pub shop_group_correct_weight: String,
    #[serde(rename = "unlockTask")]
    pub unlock_task: i32,
}
use std::collections::HashMap;

pub struct RougeSpecialTriggerTable {
    records: Vec<RougeSpecialTrigger>,
    by_id: HashMap<i32, usize>,
}

impl RougeSpecialTriggerTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeSpecialTrigger> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeSpecialTrigger> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeSpecialTrigger] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeSpecialTrigger> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}