// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeStoryList {
    pub desc: String,
    pub id: i32,
    pub image: String,
    #[serde(rename = "levelIdDict")]
    pub level_id_dict: String,
    pub name: String,
    pub season: i32,
    #[serde(rename = "stageId")]
    pub stage_id: i32,
    #[serde(rename = "storyIdList")]
    pub story_id_list: String,
}
use std::collections::HashMap;

pub struct RougeStoryListTable {
    records: Vec<RougeStoryList>,
    by_id: HashMap<i32, usize>,
}

impl RougeStoryListTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeStoryList> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeStoryList> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeStoryList] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeStoryList> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}