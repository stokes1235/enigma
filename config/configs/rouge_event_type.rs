// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeEventType {
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: i32,
    pub version: String,
}
pub struct RougeEventTypeTable {
    records: Vec<RougeEventType>,
}

impl RougeEventTypeTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeEventType> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeEventType] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeEventType> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}