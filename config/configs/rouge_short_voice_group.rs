// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeShortVoiceGroup {
    pub id: i32,
    pub rate: i32,
    #[serde(rename = "triggerParam")]
    pub trigger_param: i32,
    #[serde(rename = "triggerType")]
    pub trigger_type: i32,
}
use std::collections::HashMap;

pub struct RougeShortVoiceGroupTable {
    records: Vec<RougeShortVoiceGroup>,
    by_id: HashMap<i32, usize>,
}

impl RougeShortVoiceGroupTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeShortVoiceGroup> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeShortVoiceGroup> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeShortVoiceGroup] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeShortVoiceGroup> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}