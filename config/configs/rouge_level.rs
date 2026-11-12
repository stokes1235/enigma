// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLevel {
    pub exp: i32,
    pub level: i32,
    pub season: i32,
}
pub struct RougeLevelTable {
    records: Vec<RougeLevel>,
}

impl RougeLevelTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLevel> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeLevel] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLevel> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}