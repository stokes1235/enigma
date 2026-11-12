// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougePiece {
    #[serde(rename = "bossEffect")]
    pub boss_effect: i32,
    pub desc: String,
    #[serde(rename = "entrustType")]
    pub entrust_type: i32,
    pub id: i32,
    #[serde(rename = "pieceRes")]
    pub piece_res: String,
    #[serde(rename = "talkId")]
    pub talk_id: i32,
    pub title: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougePieceTable {
    records: Vec<RougePiece>,
    by_id: HashMap<i32, usize>,
}

impl RougePieceTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougePiece> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougePiece> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougePiece] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougePiece> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}