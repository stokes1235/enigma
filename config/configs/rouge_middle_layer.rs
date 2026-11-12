// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeMiddleLayer {
    #[serde(rename = "dayOrNight")]
    pub day_or_night: i32,
    pub empty: i32,
    pub id: i32,
    #[serde(rename = "leavePos")]
    pub leave_pos: String,
    #[serde(rename = "leavePosUnlockParam")]
    pub leave_pos_unlock_param: String,
    #[serde(rename = "leavePosUnlockType")]
    pub leave_pos_unlock_type: i32,
    #[serde(rename = "mapRes")]
    pub map_res: String,
    pub name: String,
    #[serde(rename = "nextLayer")]
    pub next_layer: String,
    pub path: String,
    #[serde(rename = "pathPointPos")]
    pub path_point_pos: String,
    #[serde(rename = "pathSelect")]
    pub path_select: String,
    #[serde(rename = "pointPos")]
    pub point_pos: String,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeMiddleLayerTable {
    records: Vec<RougeMiddleLayer>,
    by_id: HashMap<i32, usize>,
}

impl RougeMiddleLayerTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeMiddleLayer> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeMiddleLayer> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeMiddleLayer] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeMiddleLayer> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}