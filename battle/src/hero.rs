pub use crate::engine::entity::stats::StatInputs;

use sonettobuf::{HeroAttribute, HeroExAttribute, HeroSpAttribute};

pub fn profile_attributes(input: &StatInputs) -> (HeroAttribute, HeroExAttribute, HeroSpAttribute) {
    let stats = crate::engine::entity::stats::Stats::build(input);
    (stats.base(), stats.ex(), stats.sp())
}

/// Base skill groups using the same insight/portrait replacements as a battle.
/// The server selects group one or two for a frozen Rouge support mapping.
pub fn active_skills(
    tables: &config::GameDB,
    hero_id: i32,
    rank: i32,
    ex_level: i32,
) -> (Vec<i32>, Vec<i32>, i32) {
    crate::engine::entity::skill::Skill::active_skills(tables, hero_id, rank, ex_level)
}
