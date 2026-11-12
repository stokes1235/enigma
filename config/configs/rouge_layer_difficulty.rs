// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLayerDifficulty {
    pub difficulty: i32,
    pub layer: i32,
    #[serde(rename = "stepAttr")]
    pub step_attr: String,
}
pub struct RougeLayerDifficultyTable {
    records: Vec<RougeLayerDifficulty>,
}

impl RougeLayerDifficultyTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLayerDifficulty> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeLayerDifficulty] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLayerDifficulty> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}