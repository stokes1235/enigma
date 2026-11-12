// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeStyle {
    #[serde(rename = "activeSkills")]
    pub active_skills: String,
    pub capacity: i32,
    pub coin: i32,
    pub desc: String,
    #[serde(rename = "halfCost")]
    pub half_cost: i32,
    pub icon: String,
    pub id: i32,
    #[serde(rename = "layoutId")]
    pub layout_id: i32,
    #[serde(rename = "mapSkills")]
    pub map_skills: String,
    pub name: String,
    #[serde(rename = "passiveSkillDescs")]
    pub passive_skill_descs: String,
    #[serde(rename = "passiveSkillDescs2")]
    pub passive_skill_descs2: String,
    pub power: i32,
    #[serde(rename = "powerLimit")]
    pub power_limit: i32,
    pub season: i32,
    #[serde(rename = "talentPointGroup")]
    pub talent_point_group: String,
    #[serde(rename = "talentSkill")]
    pub talent_skill: String,
    #[serde(rename = "unlockParam")]
    pub unlock_param: String,
    #[serde(rename = "unlockType")]
    pub unlock_type: i32,
    pub version: String,
}
use std::collections::HashMap;

pub struct RougeStyleTable {
    records: Vec<RougeStyle>,
    by_id: HashMap<i32, usize>,
}

impl RougeStyleTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeStyle> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeStyle> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeStyle] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeStyle> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}