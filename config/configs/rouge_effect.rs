// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeEffect {
    pub id: i32,
    pub tips: String,
    #[serde(rename = "type")]
    pub r#type: i32,
    #[serde(rename = "typeParam")]
    pub type_param: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeEffectTable {
    records: Vec<RougeEffect>,
    by_id: HashMap<i32, usize>,
}

impl RougeEffectTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeEffect> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeEffect> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeEffect] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeEffect> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}