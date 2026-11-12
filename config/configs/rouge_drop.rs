// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeDrop {
    pub coin: String,
    pub drop: String,
    #[serde(rename = "dropCount")]
    pub drop_count: String,
    #[serde(rename = "enterBag")]
    pub enter_bag: i32,
    pub exp: i32,
    pub id: i32,
    #[serde(rename = "notOwned")]
    pub not_owned: i32,
    pub power: i32,
    #[serde(rename = "selectCount")]
    pub select_count: i32,
    pub talent: i32,
}
use std::collections::HashMap;

pub struct RougeDropTable {
    records: Vec<RougeDrop>,
    by_id: HashMap<i32, usize>,
}

impl RougeDropTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeDrop> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeDrop> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeDrop] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeDrop> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}