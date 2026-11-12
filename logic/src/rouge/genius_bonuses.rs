//! Numeric permanent-growth bonuses, read from the original Rouge tables.
//! Scripted effects are intentionally not inferred from their numeric ids.
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bonuses {
    pub attributes: BTreeMap<i32, i32>,
    pub initial_coin: i32,
    pub initial_capacity: i32,
    pub credit_percent: i32,
    pub essence_percent: i32,
    pub coin_percent: i32,
    pub battle_coin_percent: i32,
    pub safe_heal: i32,
    pub minor_heal: i32,
    pub danger_heal: i32,
    pub danger_power: i32,
    pub minor_drops: i32,
    pub danger_drops: i32,
    pub crisis_drops: i32,
    pub crisis_select: i32,
}

pub fn from_ids(tables: &config::GameDB, ids: &[i32]) -> Bonuses {
    let mut result = Bonuses::default();
    let mut seen = HashSet::new();
    for id in ids.iter().filter(|id| seen.insert(**id)) {
        let Some(row) = tables
            .rouge_genius_branch
            .get(*id)
            .filter(|row| row.season == 1)
        else {
            continue;
        };
        for raw in row.attribute.split('|') {
            if let Some((id, value)) = raw.split_once('#')
                && let (Ok(id), Ok(value)) = (id.parse::<i32>(), value.parse::<i32>())
            {
                *result.attributes.entry(id).or_default() += value;
            }
        }
        for raw in row.start_view.split('|') {
            if let Some((name, value)) = raw.split_once('#')
                && let Ok(value) = value.parse::<i32>()
            {
                match name {
                    "coin" => result.initial_coin += value,
                    "capacity" => result.initial_capacity += value,
                    "point" => result.credit_percent += value,
                    _ => {}
                }
            }
        }
        // These effects have no numeric parameter in the export. Their exact
        // amounts are stated by rouge_genius_branch.desc / language_en and the
        // uploaded overview; keep this finite mapping separate from parsing.
        match id {
            202 | 308 | 608 => result.essence_percent += 10,
            208 | 802 => result.coin_percent += 10,
            305 => result.battle_coin_percent += 10,
            204 => result.safe_heal += 200,
            304 => result.minor_heal += 50,
            307 => result.danger_heal += 100,
            310 => result.danger_power += 10,
            607 => result.minor_drops += 1,
            610 => result.danger_drops += 1,
            613 => {
                result.crisis_drops += 1;
                result.crisis_select += 1;
            }
            _ => {}
        }
    }
    result
}

pub fn scale(amount: i32, percent: i32) -> i32 {
    (amount.max(0) as i64 * (100_i64 + percent as i64).max(0) / 100).min(i32::MAX as i64) as i32
}

impl Bonuses {
    pub fn extra_drops(&self, drop_id: i32) -> (i32, i32) {
        match drop_id {
            1001..=1005 => (self.minor_drops, 0),
            1006..=1010 => (self.danger_drops, 0),
            1011..=1015 => (self.crisis_drops, self.crisis_select),
            _ => (0, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> &'static config::GameDB {
        super::super::original_tests::init_config();
        config::configs::get()
    }
    #[test]
    fn numeric_growth_matches_the_overview_totals_without_double_counting() {
        let tables = tables();
        let ids: Vec<i32> = tables
            .rouge_genius_branch
            .iter()
            .filter(|row| row.season == 1)
            .map(|row| row.id)
            .collect();
        let all = from_ids(tables, &ids);
        assert_eq!(
            (
                all.initial_coin,
                all.initial_capacity,
                all.credit_percent,
                all.essence_percent
            ),
            (100, 8, 25, 30)
        );
        assert_eq!(all.attributes.get(&101), Some(&700));
        assert_eq!(all.attributes.get(&102), Some(&250));
        assert_eq!(from_ids(tables, &[102, 102]).initial_coin, 20);
        assert_eq!(all.extra_drops(1001), (1, 0));
        assert_eq!(all.extra_drops(1013), (1, 1));
        assert_eq!(all.extra_drops(10610204), (0, 0));
    }
    #[test]
    fn bonuses_stack_before_integer_rounding_and_saturate() {
        assert_eq!(scale(15, 30), 19);
        assert_eq!(scale(i32::MAX, 100), i32::MAX);
        assert_eq!(scale(-5, 30), 0);
    }
}
