//! Original-Rouge achievement facts. Collection scoring still uses the ending
//! bag; "obtained" achievements instead retain real per-run instance history.
//! Missing legacy telemetry stays unknown rather than awarding a badge.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub collection_uids: BTreeSet<i64>,
    pub collection_ids: BTreeSet<i32>,
    pub disk_uids: BTreeSet<i64>,
    pub recruited_heroes: BTreeSet<i32>,
    pub fight_ids: BTreeSet<i64>,
    pub defeated_events: BTreeSet<i32>,
    pub successful_fountain_choices: BTreeSet<i32>,
    pub fountain_gains: std::collections::BTreeMap<String, BTreeSet<i32>>,
    pub map_revives: i32,
    pub battle_revives: i32,
    pub max_action_damage: i64,
    /// Actual deployed combatants plus aiders in a won ending fight. None is
    /// not interchangeable with an empty party or the entire recruited roster.
    pub final_party: Option<Vec<i32>>,
}

/// One Fountain must yield all three real gains; successes at different map
/// nodes must not be combined into a fictitious lucky three-stage encounter.
pub fn record_fountain_gain(
    progress: &mut Progress,
    source: super::special_events::FountainSource,
    choice: i32,
) {
    const REQUIRED: [i32; 3] = [10611301, 10611311, 10611321];
    if !REQUIRED.contains(&choice) {
        return;
    }
    let key = format!("{}:{}:{}", source.run_seed, source.layer_id, source.node_id);
    let gains = progress.fountain_gains.entry(key).or_default();
    gains.insert(choice);
    if REQUIRED.iter().all(|id| gains.contains(id)) {
        progress.successful_fountain_choices.extend(REQUIRED);
    }
}

pub fn observe_bag(run: &mut RougeRun, bag: &bag::RunBag) -> Result<(), AppError> {
    let mut detail = flow::details(run);
    for (uid, id) in artifact_lifecycle::owned(bag) {
        if uid <= 0 || !artifact_rules::base_item_ids().contains(&id) {
            continue;
        }
        detail.achievements.collection_uids.insert(uid);
        detail.achievements.collection_ids.insert(id);
        if collection_shapes::is_enchant_item(id) {
            detail.achievements.disk_uids.insert(uid);
        }
    }
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

pub fn observe_roster(run: &mut RougeRun) -> Result<(), AppError> {
    let mut detail = flow::details(run);
    let team: RougeTeamInfo = serde_json::from_str(&run.team)?;
    detail.achievements.recruited_heroes.extend(
        team.hero_life_list
            .iter()
            .filter_map(|life| life.hero_id)
            .filter(|id| *id > 0),
    );
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

pub fn record_battle(
    run: &mut RougeRun,
    fight_id: i64,
    event_id: Option<i32>,
    won: bool,
    ending_fight: bool,
    frozen_party: &[i32],
    summary: Option<&battle::rouge::BattleSummary>,
) -> Result<(), AppError> {
    // Real settlement IDs provide the transaction's idempotency key. Legacy
    // synthetic/no-ID battles cannot manufacture damage or participation.
    if fight_id <= 0 {
        return Ok(());
    }
    let mut detail = flow::details(run);
    if !detail.achievements.fight_ids.insert(fight_id) {
        return Ok(());
    }
    if let Some(summary) = summary {
        detail.achievements.max_action_damage = detail
            .achievements
            .max_action_damage
            .max(summary.max_action_damage.max(0));
        detail.achievements.battle_revives = detail
            .achievements
            .battle_revives
            .saturating_add(summary.ally_revives.max(0));
    }
    if won {
        if let Some(event) = event_id.filter(|id| *id > 0) {
            detail.achievements.defeated_events.insert(event);
        }
        if ending_fight && !frozen_party.is_empty() && frozen_party.iter().all(|id| *id > 0) {
            detail.achievements.final_party = Some(frozen_party.to_vec());
        }
    }
    run.details = serde_json::to_string(&detail)?;
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub battles: i32,
    pub collections: i32,
    pub compositions: i32,
    pub spent_power: i32,
    pub spent_coin: i32,
    pub max_action_damage: i64,
    pub revives: i32,
    pub crisis_battles: i32,
    pub entrusts: i32,
    pub characters: i32,
    pub disks: i32,
    pub defeated_events: BTreeSet<i32>,
    pub collection_ids: BTreeSet<i32>,
    pub successful_fountain_choices: BTreeSet<i32>,
    pub final_party: Option<Vec<i32>>,
    pub cleared: bool,
}

pub fn facts(run: &RougeRun, completed_battles: i32, crisis_battles: i32, entrusts: i32) -> Facts {
    let detail = flow::details(run);
    let mut p = detail.achievements;
    // Legacy saves can contribute their actual current bag and roster. Do not
    // reconstruct consumed items, vanished allies, or previous failed battles.
    if let Ok(bag) = serde_json::from_str::<bag::RunBag>(&run.bag) {
        for (uid, id) in artifact_lifecycle::owned(&bag) {
            if uid > 0 && artifact_rules::base_item_ids().contains(&id) {
                p.collection_uids.insert(uid);
                p.collection_ids.insert(id);
                if collection_shapes::is_enchant_item(id) {
                    p.disk_uids.insert(uid);
                }
            }
        }
    }
    if let Ok(team) = serde_json::from_str::<RougeTeamInfo>(&run.team) {
        p.recruited_heroes.extend(
            team.hero_life_list
                .iter()
                .filter_map(|life| life.hero_id)
                .filter(|id| *id > 0),
        );
    }
    Facts {
        battles: completed_battles.max(p.fight_ids.len() as i32),
        collections: p.collection_uids.len() as i32,
        compositions: detail.compositions.values().copied().sum(),
        spent_power: detail.consume_power,
        spent_coin: run.spent_coin,
        max_action_damage: p.max_action_damage,
        revives: p.map_revives.saturating_add(p.battle_revives),
        crisis_battles,
        entrusts,
        characters: p.recruited_heroes.len() as i32,
        disks: p.disk_uids.len() as i32,
        defeated_events: p.defeated_events,
        collection_ids: p.collection_ids,
        successful_fountain_choices: p.successful_fountain_choices,
        final_party: p.final_party,
        cleared: run.terminal_state == 1,
    }
}

pub fn evaluate<'a>(
    tables: &'a config::GameDB,
    facts: &Facts,
) -> Vec<&'a config::rouge_badge::RougeBadge> {
    tables
        .rouge_badge
        .iter()
        .filter(|badge| {
            if badge.season != 1 {
                return false;
            }
            let Ok(need) = badge.trigger_param.parse::<i32>() else {
                return badge.trigger == 15
                    && badge
                        .trigger_param
                        .split('#')
                        .map(str::parse::<i32>)
                        .collect::<Result<Vec<_>, _>>()
                        .is_ok_and(|ids| {
                            !ids.is_empty()
                                && ids
                                    .iter()
                                    .all(|id| facts.successful_fountain_choices.contains(id))
                        });
            };
            match badge.trigger {
                // Both saved Chinese Wiki and English table explicitly say >12.
                1 => facts.battles > need,
                2 => facts.collections >= need,
                // 1001 "Artefact Exchange" is composition, not the separate 1003
                // Voiceless Scales (displace) service.
                3 => facts.compositions >= need,
                4 => facts.spent_power >= need,
                5 => facts.spent_coin >= need,
                6 => facts.max_action_damage >= i64::from(need),
                7 => facts.revives >= need,
                8 => facts.crisis_battles >= need,
                9 => facts.entrusts >= need,
                10 => facts.characters >= need,
                11 => {
                    facts.cleared
                        && facts
                            .final_party
                            .as_ref()
                            .is_some_and(|party| !party.is_empty() && (party.len() as i32) < need)
                }
                12 => facts.disks >= need,
                13 => facts.defeated_events.contains(&need),
                14 => facts.collection_ids.contains(&need),
                15 => facts.successful_fountain_choices.contains(&need),
                16 => {
                    facts.cleared
                        && facts.final_party.as_ref().is_some_and(|party| {
                            !party.is_empty()
                                && party.iter().all(|id| {
                                    tables
                                        .rouge_character(*id)
                                        .is_some_and(|hero| hero.rare < need)
                                })
                        })
                }
                _ => false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> &'static config::GameDB {
        original_tests::init_config();
        config::configs::get()
    }
    fn ids(facts: &Facts) -> Vec<i32> {
        evaluate(tables(), facts)
            .into_iter()
            .map(|badge| badge.id)
            .collect()
    }

    #[test]
    fn all_configured_badges_have_evidence_and_boundary_conditions() {
        let facts = Facts {
            battles: 13,
            collections: 40,
            compositions: 3,
            spent_power: 120,
            spent_coin: 1000,
            max_action_damage: 150000,
            revives: 1,
            crisis_battles: 3,
            entrusts: 3,
            characters: 20,
            disks: 15,
            defeated_events: BTreeSet::from([103206, 101917, 103409]),
            collection_ids: BTreeSet::from([8330111, 8330311, 8330112, 8320025]),
            successful_fountain_choices: BTreeSet::from([10611301, 10611311, 10611321]),
            final_party: Some(vec![3005]),
            cleared: true,
        };
        assert!(tables().rouge_character(3005).unwrap().rare < 5);
        assert_eq!(ids(&facts), (1..=21).collect::<Vec<_>>());
        let mut boundary = facts.clone();
        boundary.battles = 12;
        boundary.max_action_damage = 149999;
        boundary.final_party = None;
        for missing in [1, 6, 11, 21] {
            assert!(!ids(&boundary).contains(&missing));
        }
        boundary = facts;
        boundary.successful_fountain_choices.remove(&10611311);
        assert!(!ids(&boundary).contains(&19));
        boundary.cleared = false;
        assert!(!ids(&boundary).contains(&11) && !ids(&boundary).contains(&21));
    }

    #[test]
    fn six_star_aider_counts_even_when_the_combatant_is_low_rarity() {
        let mut facts = Facts {
            cleared: true,
            final_party: Some(vec![3005, 3003]),
            ..Default::default()
        };
        assert!(tables().rouge_character(3003).unwrap().rare >= 5);
        assert!(ids(&facts).contains(&11));
        assert!(!ids(&facts).contains(&21));
        facts.final_party = Some(vec![3005, 99999999]);
        assert!(!ids(&facts).contains(&21));
    }

    #[test]
    fn only_real_fountain_gains_and_recorded_victories_unlock_their_badges() {
        let mut facts = Facts::default();
        facts.defeated_events.insert(103206);
        assert_eq!(ids(&facts), vec![13]);
        facts
            .successful_fountain_choices
            .extend([10611301, 10611311]);
        assert!(!ids(&facts).contains(&19));
    }

    #[test]
    fn fountain_successes_from_different_nodes_do_not_combine() {
        let mut progress = Progress::default();
        let source = super::super::special_events::FountainSource {
            run_seed: 7,
            layer_id: 111,
            node_id: 10,
        };
        record_fountain_gain(&mut progress, source, 10611301);
        record_fountain_gain(&mut progress, source, 10611311);
        record_fountain_gain(
            &mut progress,
            super::super::special_events::FountainSource {
                node_id: 11,
                ..source
            },
            10611321,
        );
        assert!(progress.successful_fountain_choices.is_empty());
        record_fountain_gain(&mut progress, source, 10611321);
        assert_eq!(
            progress.successful_fountain_choices,
            BTreeSet::from([10611301, 10611311, 10611321])
        );
    }
}
