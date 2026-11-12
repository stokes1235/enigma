// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougePiecePosition {
    pub desc: String,
    #[serde(rename = "entrustType")]
    pub entrust_type: i32,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "layerId")]
    pub layer_id: String,
    #[serde(rename = "pieceRes")]
    pub piece_res: String,
    #[serde(rename = "talkId")]
    pub talk_id: i32,
    #[serde(rename = "talkView")]
    pub talk_view: String,
    pub title: String,
    #[serde(rename = "unlockParam")]
    pub unlock_param: String,
    #[serde(rename = "unlockType")]
    pub unlock_type: i32,
    pub version: i32,
}
use std::collections::HashMap;

pub struct RougePiecePositionTable {
    records: Vec<RougePiecePosition>,
    by_id: HashMap<i32, usize>,
}

impl RougePiecePositionTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougePiecePosition> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougePiecePosition> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougePiecePosition] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougePiecePosition> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}