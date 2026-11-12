// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeDropGroup {
    #[serde(rename = "collectionId")]
    pub collection_id: i32,
    #[serde(rename = "groupId")]
    pub group_id: i32,
    pub id: i32,
    pub rate: i32,
}
use std::collections::HashMap;

pub struct RougeDropGroupTable {
    records: Vec<RougeDropGroup>,
    by_id: HashMap<i32, usize>,
    by_group: HashMap<i32, Vec<usize>>,
}

impl RougeDropGroupTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeDropGroup> = crate::load_rows(path)?;

        let mut by_id = HashMap::with_capacity(records.len());
        let mut by_group: HashMap<i32, Vec<usize>> = HashMap::new();

        for (idx, record) in records.iter().enumerate() {
            by_id.insert(record.id, idx);
            by_group.entry(record.group_id).or_default().push(idx);
        }

        Ok(Self {
            records,
            by_id,
            by_group,
        })
    }

    #[inline]
    pub fn get(&self, id: i32) -> Option<&RougeDropGroup> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    pub fn by_group(&self, group_id: i32) -> impl Iterator<Item = &'_ RougeDropGroup> + '_ {
        self.by_group
            .get(&group_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter())
            .map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeDropGroup] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeDropGroup> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}