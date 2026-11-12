use super::*;

fn source(seed: u64) -> FountainSource {
    FountainSource {
        run_seed: seed,
        layer_id: 111,
        node_id: 4,
    }
}

fn apply(result: Result<FountainResolution, FountainError>) -> FountainOutcome {
    match result.unwrap() {
        FountainResolution::Apply(value) => value,
        FountainResolution::Replayed(_) => panic!("expected a fresh action"),
    }
}

fn tables() -> &'static config::GameDB {
    super::super::original_tests::init_config();
    config::configs::get()
}

#[test]
fn phase_menus_are_explicit_and_nested_events_never_spawn_as_new_fountains() {
    assert_eq!(
        choice_ids(106113),
        Some([10611301, 10611302, 10611303].as_slice())
    );
    assert_eq!(
        choice_ids(1061131),
        Some([10611311, 10611312, 10611313].as_slice())
    );
    assert_eq!(
        choice_ids(1061132),
        Some([10611321, 10611322, 10611323].as_slice())
    );
    assert!(can_spawn_randomly(tables(), 106113, &[], &[]));
    assert!(!can_spawn_randomly(tables(), 1061131, &[], &[]));
    assert!(!can_spawn_randomly(tables(), 1061132, &[], &[]));
    assert!(
        choice_action(106113, 10611301).is_none(),
        "the generic deterministic +30 drop path does not own this random chain"
    );
}

#[test]
fn both_throw_and_observe_thresholds_match_their_own_client_configuration() {
    let tables = tables();
    let mut state = State::default();
    for (phase, event) in FOUNTAIN_EVENTS.iter().enumerate() {
        let ids = fountain_choice_ids(*event).unwrap();
        let threshold = [30, 80, 120][phase];
        for id in &ids[..2] {
            let row = tables.rouge_choice.get(*id).unwrap();
            assert_eq!(
                (row.unlock_type, row.unlock_param.parse::<i32>().unwrap()),
                (2, threshold)
            );
            assert_eq!(
                resolve_fountain(&mut state, source(7), *event, *id, threshold - 1),
                Err(FountainError::InsufficientCoin)
            );
        }
        apply(resolve_fountain(
            &mut state,
            source(7),
            *event,
            ids[0],
            threshold,
        ));
    }
}

#[test]
fn consecutive_wins_use_displayed_net_gains_without_an_additional_stake() {
    // Fixed action-local fixture, not a promise that every seed wins.
    let mut state = State::default();
    let mut coin = 1000;
    let mut badges = Vec::new();
    for (phase, event) in FOUNTAIN_EVENTS.iter().enumerate() {
        let id = fountain_choice_ids(*event).unwrap()[0];
        let outcome = apply(resolve_fountain(&mut state, source(7), *event, id, coin));
        assert_eq!(outcome.won, Some(true));
        assert_eq!(outcome.coin_delta, [30, 60, 120][phase]);
        assert_eq!(
            outcome.next_event_id,
            FOUNTAIN_EVENTS.get(phase + 1).copied()
        );
        coin += outcome.coin_delta;
        badges.push(outcome.badge_choice.unwrap());
    }
    assert_eq!(
        coin, 1210,
        "all three values are net changes; the 30/80/120 gate is not charged"
    );
    assert_eq!(badges, vec![10611301, 10611311, 10611321]);
    assert_eq!(
        resolve_fountain(&mut state, source(7), 1061132, 10611322, coin),
        Err(FountainError::PhaseMismatch)
    );
}

#[test]
fn losses_are_real_and_advance_by_the_explicit_local_phase_convention() {
    let mut state = State::default();
    let mut coin = 1000;
    for (phase, event) in FOUNTAIN_EVENTS.iter().enumerate() {
        let id = fountain_choice_ids(*event).unwrap()[0];
        let outcome = apply(resolve_fountain(&mut state, source(17), *event, id, coin));
        assert_eq!(outcome.won, Some(false));
        assert_eq!(outcome.coin_delta, -[20, 40, 100][phase]);
        assert_eq!(outcome.badge_choice, None);
        assert_eq!(
            outcome.next_event_id,
            FOUNTAIN_EVENTS.get(phase + 1).copied()
        );
        coin += outcome.coin_delta;
    }
    assert_eq!(coin, 840);
}

#[test]
fn observations_have_both_outcomes_but_are_not_the_badges_configured_throw_ids() {
    let mut wins = 0;
    let mut losses = 0;
    for seed in 0..128 {
        let result = apply(resolve_fountain(
            &mut State::default(),
            source(seed),
            106113,
            10611302,
            30,
        ));
        match result.won {
            Some(true) => {
                wins += 1;
                assert_eq!(result.coin_delta, 30);
            }
            Some(false) => {
                losses += 1;
                assert_eq!(result.coin_delta, -20);
            }
            None => panic!("Observe must use the same displayed 50/50 rule"),
        }
        assert!(result.badge_choice.is_none());
        assert_eq!(result.next_event_id, Some(1061131));
    }
    assert!(
        wins > 0 && losses > 0,
        "a local random resolver must not always grant the exported positive drop"
    );
}

#[test]
fn serialization_preserves_the_roll_and_replayed_requests_never_need_another_payment() {
    let mut state = State::default();
    let first = apply(resolve_fountain(
        &mut state,
        source(7),
        106113,
        10611301,
        30,
    ));
    state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(
        resolve_fountain(&mut state, source(7), 106113, 10611301, 0),
        Ok(FountainResolution::Replayed(first))
    );
    assert_eq!(state.fountains.values().next().unwrap().outcomes.len(), 1);
    assert_eq!(
        resolve_fountain(&mut state, source(7), 106113, 10611302, 1000),
        Err(FountainError::PhaseMismatch)
    );
}

#[test]
fn cannot_skip_phases_and_leaving_is_free_and_terminal() {
    let mut state = State::default();
    assert_eq!(
        resolve_fountain(&mut state, source(1), 1061131, 10611311, 1000),
        Err(FountainError::PhaseMismatch)
    );
    assert_eq!(
        resolve_fountain(&mut state, source(1), 106113, 10611311, 1000),
        Err(FountainError::InvalidChoice)
    );
    assert!(state.fountains.is_empty());
    let leave = apply(resolve_fountain(&mut state, source(1), 106113, 10611303, 0));
    assert_eq!(
        (
            leave.coin_delta,
            leave.won,
            leave.next_event_id,
            leave.badge_choice
        ),
        (0, None, None, None)
    );
    assert_eq!(
        resolve_fountain(&mut state, source(1), 106113, 10611301, 1000),
        Err(FountainError::PhaseMismatch)
    );
    assert_eq!(
        resolve_fountain(&mut state, source(1), 106113, 10611303, 1000),
        Ok(FountainResolution::Replayed(leave))
    );
    let another = FountainSource {
        node_id: 5,
        ..source(1)
    };
    assert!(matches!(
        resolve_fountain(&mut state, another, 106113, 10611303, 0),
        Ok(FountainResolution::Apply(_))
    ));
}

#[test]
fn phase_transition_preserves_paths_and_terminal_stage_leaves_graph_for_normal_finish() {
    let tables = tables();
    let mut graph = LayerMap {
        layer_id: 111,
        stages: 3,
        nodes: vec![MapNode {
            stage: 1,
            node_id: 4,
            last: vec![0],
            next: vec![5],
            event_id: 106113,
            node_type: 6,
        }],
    };
    let mut state = State::default();
    for (phase, event) in FOUNTAIN_EVENTS.iter().enumerate() {
        let outcome = apply(resolve_fountain(
            &mut state,
            source(7),
            *event,
            fountain_choice_ids(*event).unwrap()[0],
            1000,
        ));
        let transition = fountain_transition_current(tables, &mut graph, &outcome).unwrap();
        assert_eq!(
            (
                graph.nodes[0].stage,
                graph.nodes[0].last.clone(),
                graph.nodes[0].next.clone()
            ),
            (1, vec![0], vec![5])
        );
        if phase < 2 {
            assert_eq!(transition.unwrap().event_id, FOUNTAIN_EVENTS[phase + 1]);
            assert_eq!(graph.nodes[0].event_id, FOUNTAIN_EVENTS[phase + 1]);
        } else {
            assert!(transition.is_none());
            assert_eq!(graph.nodes[0].event_id, 1061132);
        }
    }
}
