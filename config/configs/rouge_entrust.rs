// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeEntrust {
    pub id: i32,
    #[serde(rename = "incompleteEffect")]
    pub incomplete_effect: String,
    pub interactive: String,
    pub param: String,
    #[serde(rename = "type")]
    pub r#type: i32,
}
use std::collections::HashMap;

pub struct RougeEntrustTable {
    records: Vec<RougeEntrust>,
    by_id: HashMap<i32, usize>,
}

impl RougeEntrustTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeEntrust> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeEntrust> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeEntrust] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeEntrust> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}