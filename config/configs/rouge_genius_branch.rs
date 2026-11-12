// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeGeniusBranch {
    pub attribute: String,
    pub before: String,
    pub cost: i32,
    pub desc: String,
    pub effects: String,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "initialCollection")]
    pub initial_collection: String,
    #[serde(rename = "isOrigin")]
    pub is_origin: i32,
    pub name: String,
    #[serde(rename = "openDesc")]
    pub open_desc: String,
    pub pos: String,
    pub season: i32,
    pub show: i32,
    #[serde(rename = "startView")]
    pub start_view: String,
    pub talent: i32,
}
use std::collections::HashMap;

pub struct RougeGeniusBranchTable {
    records: Vec<RougeGeniusBranch>,
    by_id: HashMap<i32, usize>,
}

impl RougeGeniusBranchTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeGeniusBranch> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeGeniusBranch> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeGeniusBranch] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeGeniusBranch> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}