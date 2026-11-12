// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RogueCollection {
    pub attr: String,
    pub desc: String,
    #[serde(rename = "dropGroupWeights")]
    pub drop_group_weights: String,
    #[serde(rename = "eventGroupWeights")]
    pub event_group_weights: String,
    #[serde(rename = "eventWeights")]
    pub event_weights: String,
    pub group: String,
    #[serde(rename = "heartbeatRange")]
    pub heartbeat_range: String,
    #[serde(rename = "holeNum")]
    pub hole_num: i32,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "inHandBook")]
    pub in_hand_book: i32,
    pub name: String,
    pub rare: i32,
    #[serde(rename = "shopWeights")]
    pub shop_weights: String,
    #[serde(rename = "showRare")]
    pub show_rare: i32,
    pub skills: String,
    pub spdesc: String,
    pub statetype: i32,
    #[serde(rename = "type")]
    pub r#type: i32,
    pub unique: i32,
    #[serde(rename = "unlockTask")]
    pub unlock_task: i32,
}
use std::collections::HashMap;

pub struct RogueCollectionTable {
    records: Vec<RogueCollection>,
    by_id: HashMap<i32, usize>,
}

impl RogueCollectionTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RogueCollection> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RogueCollection> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RogueCollection] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RogueCollection> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}