// Auto-generated from JSON data
// Do not edit manually

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RougeSurpriseAttack {
    #[serde(rename = "additionRule")]
    pub addition_rule: String,
    #[serde(rename = "hiddenRule")]
    pub hidden_rule: String,
    pub id: i32,
    #[serde(rename = "ruleDesc")]
    pub rule_desc: String,
    #[serde(rename = "tipsDesc")]
    pub tips_desc: String,
    pub title: String,
}
use std::collections::HashMap;

pub struct RougeSurpriseAttackTable {
    records: Vec<RougeSurpriseAttack>,
    by_id: HashMap<i32, usize>,
}

impl RougeSurpriseAttackTable {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let records: Vec<RougeSurpriseAttack> = crate::load_rows(path)?;

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
    pub fn get(&self, id: i32) -> Option<&RougeSurpriseAttack> {
        self.by_id.get(&id).map(|&i| &self.records[i])
    }

    #[inline]
    pub fn all(&self) -> &[RougeSurpriseAttack] {
        &self.records
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, RougeSurpriseAttack> {
        self.records.iter()
    }

    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
}