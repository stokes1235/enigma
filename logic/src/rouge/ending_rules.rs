//! Client-confirmed run-local requirements for original Rouge choices.
//!
//! `activeType` controls visibility and unlock animation; `unlockType` controls
//! availability. Callers validate both. Ending history comes from the run's snapshot;
//! other historical systems are resolved by the caller.
use database::models::game::rouge::RougeRun;

use super::{artifact_rules, bag, collection_shapes, flow};

fn number(param: &str) -> Option<i32> {
    param.trim().parse::<i32>().ok().filter(|value| *value >= 0)
}

fn pair(param: &str) -> Option<(i32, i32)> {
    let (first, second) = param.split_once('#')?;
    Some((number(first)?, number(second)?))
}

/// Own inlaid disks as well as the placed and warehouse items. The client
/// retains inlaid disks in RougeCollectionModel:getAllCollections().
fn owned_ids(run: &RougeRun) -> Option<Vec<i32>> {
    let bag = serde_json::from_str::<bag::RunBag>(&run.bag).ok()?;
    Some(
        bag.items
            .iter()
            .flat_map(|item| std::iter::once(item.item_id).chain(item.holes.iter().copied()))
            .filter(|id| *id > 0)
            .collect(),
    )
}

/// Implements RougeMapUnlockHelper requirements backed by run state or the
/// persisted ending-history snapshot. Other history and DLC gates fail closed.
pub fn is_unlocked(run: &RougeRun, kind: i32, param: &str) -> bool {
    match kind {
        0 => true,
        1 => {
            number(param).is_some_and(|id| owned_ids(run).is_some_and(|items| items.contains(&id)))
        }
        2 => number(param).is_some_and(|need| run.coin >= need),
        3 => number(param).is_some_and(|need| {
            super::persisted_team(run)
                .is_some_and(|team| team.hero_life_list.len() >= need as usize)
        }),
        4 => number(param).is_some_and(|need| run.power >= need),
        5 => number(param)
            .is_some_and(|need| owned_ids(run).is_some_and(|items| items.len() >= need as usize)),
        6 => pair(param).is_some_and(|(rarity, need)| {
            owned_ids(run).is_some_and(|items| {
                items
                    .into_iter()
                    .filter(|id| artifact_rules::collection_rarity(*id) == Some(rarity))
                    .count()
                    >= need as usize
            })
        }),
        // PassLayer compares the configured layer ID, not the display number.
        7 => number(param).is_some_and(|layer| run.layer_id >= layer),
        8 => pair(param).is_some_and(|(tag, need)| {
            owned_ids(run).is_some_and(|items| {
                items
                    .into_iter()
                    // The shipped helper breaks after the first editor tag.
                    // Do not substitute effective tags added by an inlay.
                    .filter(|id| artifact_rules::collection_tags(*id).first() == Some(&tag))
                    .count()
                    >= need as usize
            })
        }),
        9 => number(param).is_some_and(|id| {
            flow::details(run)
                .outside_genius
                .as_deref()
                .is_some_and(|nodes| nodes.contains(&id))
        }),
        10 if run.map_type == super::MAP_TYPE_MIDDLE => pair(param).is_some_and(|(id, choice)| {
            super::middle_state(run)
                .pieces
                .iter()
                .any(|piece| piece.id == Some(id) && piece.select_id == Some(choice))
        }),
        12 => flow::details(run)
            .passed_endings
            .as_deref()
            .is_some_and(|endings| !endings.is_empty()),
        13 => number(param).is_some_and(|id| {
            flow::details(run)
                .passed_endings
                .as_deref()
                .is_some_and(|endings| endings.contains(&id))
        }),
        15 if run.map_type == super::MAP_TYPE_MIDDLE => {
            let ids = param.split('#').map(number).collect::<Option<Vec<_>>>();
            ids.is_some_and(|ids| {
                super::middle_state(run).pieces.iter().any(|piece| {
                    piece.finish == Some(true) && piece.id.is_some_and(|id| ids.contains(&id))
                })
            })
        }
        17 => number(param).is_some_and(|need| {
            super::persisted_team(run).is_some_and(|team| {
                team.hero_life_list
                    .iter()
                    .filter(|hero| hero.life.unwrap_or_default() <= 0)
                    .count()
                    >= need as usize
            })
        }),
        18 => number(param).is_some_and(|need| {
            owned_ids(run).is_some_and(|items| {
                items
                    .into_iter()
                    .filter(|id| !collection_shapes::is_unique(*id))
                    .count()
                    >= need as usize
            })
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> RougeRun {
        RougeRun {
            user_id: 1,
            season: 1,
            layer_index: 3,
            layer_id: 141,
            stage: 0,
            node_id: 0,
            seed: 1,
            graph: "{}".to_owned(),
            coin: 300,
            power: 40,
            updated_at: 0,
            map_type: super::super::MAP_TYPE_MIDDLE,
            middle_layer_id: 141,
            position_index: 1,
            choices: "{}".to_owned(),
            finished: "[]".to_owned(),
            bag: serde_json::to_string(&bag::RunBag {
                rows: 8,
                cols: 8,
                items: Vec::new(),
                uid_counter: 0,
            })
            .unwrap(),
            shop: "{}".to_owned(),
            spent_coin: 0,
            retry_num: 0,
            terminal_state: 0,
            minor_count: 0,
            danger_count: 0,
            crisis_count: 0,
            step_count: 0,
            team: serde_json::to_string(&sonettobuf::RougeTeamInfo::default()).unwrap(),
            power_limit: 40,
            interactive_index: 0,
            interactive_refresh: 0,
            battle_interactive: String::new(),
            team_exp: 0,
            middle_state: String::new(),
            details: "{}".to_owned(),
        }
    }

    fn own(run: &mut RougeRun, id: i32) {
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        bag.add(id).unwrap();
        run.bag = serde_json::to_string(&bag).unwrap();
    }

    #[test]
    fn chapel_endings_require_their_real_key_items() {
        super::super::original_tests::init_config();
        let tables = config::configs::get();
        for (choice, item) in [(1410202, 8320025), (1410301, 8320907), (1410302, 8180501)] {
            let mut run = run();
            let row = tables.rouge_piece_select.get(choice).unwrap();
            assert_eq!(row.active_type, 12); // Visible after a previous ending.
            assert!(!is_unlocked(&run, row.unlock_type, &row.unlock_param));
            own(&mut run, item);
            assert!(is_unlocked(&run, row.unlock_type, &row.unlock_param));
            assert!(!is_unlocked(&run, row.active_type, &row.active_param));
            let mut detail = flow::details(&run);
            detail.passed_endings = Some(vec![101]);
            run.details = serde_json::to_string(&detail).unwrap();
            assert!(is_unlocked(&run, row.active_type, &row.active_param));
        }
    }

    #[test]
    fn each_fifth_area_requires_its_matching_chapel_choice() {
        super::super::original_tests::init_config();
        let tables = config::configs::get();
        for (piece, choice, area) in [
            (1410201, 1410201, 151),
            (1410201, 1410202, 152),
            (1410301, 1410301, 153),
            (1410301, 1410302, 154),
        ] {
            let mut run = run();
            run.middle_state = serde_json::json!({
                "middle_layer_id": 141,
                "entrust_id": 0,
                "pieces": [{"id": piece, "selectId": choice, "finish": true}]
            })
            .to_string();
            let board = tables.rouge_middle_layer.get(141).unwrap();
            assert!(is_unlocked(
                &run,
                board.leave_pos_unlock_type,
                &board.leave_pos_unlock_param
            ));
            for next in tables
                .rouge_layer
                .iter()
                .filter(|row| (151..=154).contains(&row.id))
            {
                assert_eq!(
                    is_unlocked(&run, next.unlock_type, &next.unlock_param),
                    next.id == area,
                    "choice {choice} authorizes only area {area}"
                );
            }
            run.map_type = super::super::MAP_TYPE_NORMAL;
            assert!(!is_unlocked(&run, 10, &format!("{piece}#{choice}")));
            assert!(!is_unlocked(&run, 15, "1410201#1410301"));
        }
    }

    #[test]
    fn base_safe_zone_features_require_building_charter() {
        super::super::original_tests::init_config();
        let mut run = run();
        for choice in [1002, 1003] {
            let row = config::configs::get()
                .rouge_piece_select
                .get(choice)
                .unwrap();
            assert!(!is_unlocked(&run, row.unlock_type, &row.unlock_param));
        }
        let mut details = flow::details(&run);
        details.outside_genius = Some(vec![201]);
        run.details = serde_json::to_string(&details).unwrap();
        for choice in [1002, 1003] {
            let row = config::configs::get()
                .rouge_piece_select
                .get(choice)
                .unwrap();
            assert!(is_unlocked(&run, row.unlock_type, &row.unlock_param));
        }
    }

    #[test]
    fn inventory_requirements_include_inlays_and_keep_editor_rarity() {
        let mut run = run();
        own(&mut run, 8310405);
        own(&mut run, 8320025);
        let mut bag: bag::RunBag = serde_json::from_str(&run.bag).unwrap();
        bag.items[0].holes = vec![8140101];
        bag.items[0].hole_ids = vec![3];
        bag.uid_counter = 3;
        run.bag = serde_json::to_string(&bag).unwrap();
        assert!(is_unlocked(&run, 1, "8140101"));
        assert!(is_unlocked(&run, 5, "3"));
        assert!(!is_unlocked(&run, 5, "4"));
        assert!(is_unlocked(&run, 6, "4#1"));
        assert!(is_unlocked(&run, 6, "3#1"));
        assert!(!is_unlocked(&run, 6, "3#2"));
        assert!(is_unlocked(&run, 18, "2"));
        assert!(!is_unlocked(&run, 18, "3"));
        for (kind, param) in [(1, "bad"), (2, "-1"), (6, "3#1#2"), (15, ""), (20, "1")] {
            assert!(!is_unlocked(&run, kind, param));
        }
    }

    #[test]
    fn ending_requirements_use_the_persisted_history_snapshot() {
        let mut run = run();
        assert!(!is_unlocked(&run, 12, ""));
        assert!(!is_unlocked(&run, 13, "101"));
        let mut details = flow::details(&run);
        details.passed_endings = Some(vec![101]);
        run.details = serde_json::to_string(&details).unwrap();
        assert!(is_unlocked(&run, 12, ""));
        assert!(is_unlocked(&run, 13, "101"));
        assert!(!is_unlocked(&run, 13, "102"));
        // The current run can fail without forgetting a previous success.
        run.terminal_state = 2;
        assert!(is_unlocked(&run, 13, "101"));
    }
}
