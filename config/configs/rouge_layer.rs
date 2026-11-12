// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeLayer {
    pub bgm: i32,
    #[serde(rename = "crossDesc")]
    pub cross_desc: String,
    #[serde(rename = "dayOrNight")]
    pub day_or_night: i32,
    pub desc: String,
    #[serde(rename = "endStoryId")]
    pub end_story_id: String,
    #[serde(rename = "iconPos")]
    pub icon_pos: String,
    #[serde(rename = "iconRes")]
    pub icon_res: String,
    pub id: i32,
    #[serde(rename = "levelId")]
    pub level_id: i32,
    #[serde(rename = "mapRes")]
    pub map_res: String,
    #[serde(rename = "middleLayerId")]
    pub middle_layer_id: i32,
    pub name: String,
    #[serde(rename = "nameEn")]
    pub name_en: String,
    #[serde(rename = "pathPos")]
    pub path_pos: String,
    #[serde(rename = "pathRes")]
    pub path_res: String,
    #[serde(rename = "ruleIdInstead")]
    pub rule_id_instead: String,
    #[serde(rename = "ruleIdVersion")]
    pub rule_id_version: String,
    #[serde(rename = "sceneId")]
    pub scene_id: i32,
    #[serde(rename = "startStoryId")]
    pub start_story_id: i32,
    #[serde(rename = "unlockParam")]
    pub unlock_param: String,
    #[serde(rename = "unlockType")]
    pub unlock_type: i32,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeLayerTable {
    records: Vec<RougeLayer>,
    by_id: HashMap<i32, usize>,
}

impl RougeLayerTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeLayer> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeLayer> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeLayer] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeLayer> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}