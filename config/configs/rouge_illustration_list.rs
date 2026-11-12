// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeIllustrationList {
    pub desc: String,
    pub dlc: i32,
    #[serde(rename = "eventId")]
    pub event_id: String,
    #[serde(rename = "fullImage")]
    pub full_image: String,
    pub id: i32,
    pub image: String,
    pub name: String,
    #[serde(rename = "nameEn")]
    pub name_en: String,
    pub order: i32,
    pub season: i32,
}
use std::collections::HashMap;

pub struct RougeIllustrationListTable {
    records: Vec<RougeIllustrationList>,
    by_id: HashMap<i32, usize>,
}

impl RougeIllustrationListTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeIllustrationList> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeIllustrationList> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeIllustrationList] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeIllustrationList> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}