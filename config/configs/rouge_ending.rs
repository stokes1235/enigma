// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeEnding {
    pub desc: String,
    #[serde(rename = "endingStoryId")]
    pub ending_story_id: i32,
    pub id: i32,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeEndingTable {
    records: Vec<RougeEnding>,
    by_id: HashMap<i32, usize>,
}

impl RougeEndingTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeEnding> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeEnding> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeEnding] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeEnding> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}