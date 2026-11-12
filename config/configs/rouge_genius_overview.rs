// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeGeniusOverview {
    pub icon: String,
    pub id: i32,
    pub ismul: i32,
    pub name: String,
    pub value: i32,
}
use std::collections::HashMap;

pub struct RougeGeniusOverviewTable {
    records: Vec<RougeGeniusOverview>,
    by_id: HashMap<i32, usize>,
}

impl RougeGeniusOverviewTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeGeniusOverview> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeGeniusOverview> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeGeniusOverview] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeGeniusOverview> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}