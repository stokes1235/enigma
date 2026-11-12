// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeBadge {
    pub desc: String,
    pub icon: String,
    pub id: i32,
    pub name: String,
    pub score: i32,
    pub season: i32,
    pub trigger: i32,
    #[serde(rename = "triggerParam")]
    pub trigger_param: String,
}
use std::collections::HashMap;

pub struct RougeBadgeTable {
    records: Vec<RougeBadge>,
    by_id: HashMap<i32, usize>,
}

impl RougeBadgeTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeBadge> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeBadge> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeBadge] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeBadge> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}