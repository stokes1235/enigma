// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeMapChoice {
    #[serde(rename = "dropId")]
    pub drop_id: i32,
    #[serde(rename = "groupId")]
    pub group_id: i32,
    #[serde(rename = "layerId")]
    pub layer_id: i32,
}
use std::collections::HashMap;

pub struct RougeMapChoiceTable {
    records: Vec<RougeMapChoice>,
    by_group: HashMap<i32, Vec<usize>>,
}

impl RougeMapChoiceTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeMapChoice> = crate::load_rows(path)?;

        let mut by_group: HashMap<i32, Vec<usize>> = HashMap::new();

        for (idx, record) in records.iter().enumerate() {
            by_group.entry(record.group_id).or_default().push(idx);
        }

        Ok(Self {
            records,
            by_group,
        })
    }

    pub fn by_group(&self, group_id: i32) -> impl Iterator<Item = &'_ RougeMapChoice> + '_ {
        self.by_group
            .get(&group_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter())
            .map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeMapChoice] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeMapChoice> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}