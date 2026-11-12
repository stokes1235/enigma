// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeUnlockSkills {
    #[serde(rename = "skillId")]
    pub skill_id: i32,
    pub style: i32,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(rename = "unlockEmblem")]
    pub unlock_emblem: i32,
    pub version: String,
}
pub struct RougeUnlockSkillsTable {
    records: Vec<RougeUnlockSkills>,
}

impl RougeUnlockSkillsTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeUnlockSkills> = crate::load_rows(path)?;

        Ok(Self {
            records,
        })
    }

    #[inline]
    pub fn all(&self) -> &[RougeUnlockSkills] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeUnlockSkills> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}