// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeItemStaticDesc {
    pub id: i32,
    pub item1: i32,
    pub item1_attr: String,
    pub item2: i32,
    pub item2_attr: String,
    pub item3: i32,
    pub item3_attr: String,
}
use std::collections::HashMap;

pub struct RougeItemStaticDescTable {
    records: Vec<RougeItemStaticDesc>,
    by_id: HashMap<i32, usize>,
}

impl RougeItemStaticDescTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeItemStaticDesc> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeItemStaticDesc> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeItemStaticDesc] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeItemStaticDesc> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}