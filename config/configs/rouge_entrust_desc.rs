// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeEntrustDesc {
    pub desc: String,
    #[serde(rename = "finishDesc")]
    pub finish_desc: String,
    pub id: i32,
}
use std::collections::HashMap;

pub struct RougeEntrustDescTable {
    records: Vec<RougeEntrustDesc>,
    by_id: HashMap<i32, usize>,
}

impl RougeEntrustDescTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeEntrustDesc> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeEntrustDesc> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeEntrustDesc] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeEntrustDesc> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}