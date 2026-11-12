// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLayerRule {
    pub version: String,
}
pub struct RougeLayerRuleTable {
    records: Vec<RougeLayerRule>,
}

impl RougeLayerRuleTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLayerRule> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeLayerRule] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLayerRule> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}