// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeCollectionTrammels {
    pub desc: String,
    pub id: i32,
    pub num: i32,
    #[serde(rename = "ruleList")]
    pub rule_list: String,
}
use std::collections::HashMap;

pub struct RougeCollectionTrammelsTable {
    records: Vec<RougeCollectionTrammels>,
    by_id: HashMap<i32, usize>,
}

impl RougeCollectionTrammelsTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeCollectionTrammels> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeCollectionTrammels> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeCollectionTrammels] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeCollectionTrammels> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}