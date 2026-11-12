// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeShortVoice {
    #[serde(rename = "audioId")]
    pub audio_id: i32,
    pub desc: String,
    #[serde(rename = "groupId")]
    pub group_id: i32,
    pub id: i32,
    pub weight: i32,
}
use std::collections::HashMap;

pub struct RougeShortVoiceTable {
    records: Vec<RougeShortVoice>,
    by_id: HashMap<i32, usize>,
    by_group: HashMap<i32, Vec<usize>>,
}

impl RougeShortVoiceTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeShortVoice> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeShortVoice> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    pub fn by_group(&self, group_id: i32) -> impl Iterator<Item = &'_ RougeShortVoice> + '_ {
        self.by_group
            .get(&group_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter())
            .map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeShortVoice] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeShortVoice> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}