// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeFightEvent {
    #[serde(rename = "advanceInteractive")]
    pub advance_interactive: String,
    #[serde(rename = "bossDesc")]
    pub boss_desc: String,
    #[serde(rename = "bossMask")]
    pub boss_mask: String,
    #[serde(rename = "episodeId")]
    pub episode_id: i32,
    #[serde(rename = "episodeIdInstead")]
    pub episode_id_instead: i32,
    pub id: i32,
    pub interactive: String,
    #[serde(rename = "isChangeScene")]
    pub is_change_scene: i32,
    #[serde(rename = "monsterMask")]
    pub monster_mask: String,
    pub title: String,
    #[serde(rename = "type")]
    pub r#type: i32,
    #[serde(rename = "versionEpisode")]
    pub version_episode: String,
    #[serde(rename = "versionEvent")]
    pub version_event: String,
    #[serde(rename = "versionInteractive")]
    pub version_interactive: String,
}
use std::collections::HashMap;

pub struct RougeFightEventTable {
    records: Vec<RougeFightEvent>,
    by_id: HashMap<i32, usize>,
}

impl RougeFightEventTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeFightEvent> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeFightEvent> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeFightEvent] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeFightEvent> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}