// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeDlcConst {
    pub id: i32,
    pub value: String,
    pub value2: String,
}
use std::collections::HashMap;

pub struct RougeDlcConstTable {
    records: Vec<RougeDlcConst>,
    by_id: HashMap<i32, usize>,
}

impl RougeDlcConstTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeDlcConst> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeDlcConst> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeDlcConst] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeDlcConst> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}