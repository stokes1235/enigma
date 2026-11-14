use super::*;
use crate::engine::round::command::RoundCommand;
use crate::rouge::growth::settle_queue;

fn markers(ops: &[RuleOp]) -> Vec<(i32, i32, i32, i64)> {
    ops.iter()
        .filter_map(|op| match op {
            RuleOp::EffectMarker {
                target_uid,
                effect_type: effect_type @ (183 | 184),
                effect_num,
                config_effect,
                ..
            } => Some((*effect_type, *effect_num, *config_effect, *target_uid)),
            _ => None,
        })
        .collect()
}

fn queue(h: &mut Harness, cards: &[(i64, i32, i32)]) -> Vec<RuleOp> {
    h.managers.card = CardManager::new(
        cards
            .iter()
            .map(|&(uid, skill_id, card_type)| CardInfo {
                uid: Some(uid),
                skill_id: Some(skill_id),
                card_type: Some(card_type),
                temp_card: Some(false),
                ..Default::default()
            })
            .collect(),
    );
    for _ in cards {
        h.run(vec![RuleOp::Command(BattleCommand::Card(CardCommand::Play(
            CardPlay {
                origin: ORIGIN,
                hand_index: 0,
                target_uid: Some(-1),
                chosen_skill_id: None,
                choice: None,
                recorded_skill: None,
            },
        )))]);
    }
    settle_queue(&mut h.managers, &h.pool, &mut h.random)
}

#[test]
fn sampled_queue_counts_medicine_ultimate_and_device_tag_before_actions() {
    let mut frozen = context(&[8310405], 10);
    frozen.catalyst = Some(Catalyst {
        style_id: 5,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen));
    // Captured categories: special 5, buff 2, attack 1, device-tagged 1.
    // UID 0 medicines and the type-6 ultimate are both in the special group;
    // tag 14 is distinct for resonance even though it has no known level table.
    let ops = queue(
        &mut h,
        &[
            (0, 815001411, 0),
            (0, 815001411, 0),
            (0, 815000611, 0),
            (0, 815000611, 0),
            (10, 31160201, 6),
            (10, 30390121, 0),
            (10, 31160321, 0),
            (10, 31446012, 0),
            (10, 311602113, 0),
        ],
    );
    assert_eq!(markers(&ops), [(183, 4, 8, 0), (184, 2, 0, 0)]);
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 4);
    assert_eq!(h.managers.rouge.summary().unwrap().resonance_max_level, 2);
    assert_eq!(h.growth(100), 12, "both permanent thresholds are awarded once");
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 380);
    assert!(settle_queue(&mut h.managers, &h.pool, &mut h.random).is_empty());
    let reset = end_incantation_round(&mut h.managers);
    assert_eq!(markers(&reset), [(183, 0, 8, 0), (184, 0, 0, 0)]);
    assert_eq!(h.managers.attribute.get(10, AttrId::Attack), 0);
    assert_eq!(h.growth(100), 12);
}

#[test]
fn only_largest_categories_trigger_and_ties_and_caps_are_preserved() {
    let mut frozen = context(&[], 0);
    frozen.catalyst = Some(Catalyst {
        style_id: 5,
        ..Default::default()
    });
    let mut h = Harness::new(Some(frozen.clone()));
    let ops = queue(
        &mut h,
        &[
            (0, 815000611, 0),
            (0, 815000611, 0),
            (10, 31160201, 6),
            (10, 30030111, 0),
            (11, 30030111, 0),
            (10, 30030111, 0),
            (10, 30390121, 2), // support contributes its actual skill type
        ],
    );
    assert_eq!(markers(&ops), [(183, 2, 1, 0), (183, 2, 8, 0), (184, 1, 0, 0)]);

    let mut h = Harness::new(Some(frozen.clone()));
    let ops = queue(
        &mut h,
        &[
            (10, 31446012, 0),
            (10, 31446012, 0),
            (10, 31446012, 0),
            (10, 31446012, 0),
            (10, 30030111, 0),
            (11, 30030111, 0),
            (10, 30030111, 0),
        ],
    );
    assert!(markers(&ops).is_empty(), "unknown maximal type cannot promote a smaller group");

    let mut h = Harness::new(Some(frozen));
    let cards = vec![(0, 815000611, 0); 12];
    let ops = queue(&mut h, &cards);
    assert_eq!(markers(&ops), [(183, 5, 8, 0)]);
}

#[test]
fn actual_action_schedule_sends_level_and_applies_damage_before_the_first_cast() {
    let mut h = Harness::new(Some(context(&[], 0)));
    h.managers.card = CardManager::new(
        [10_i64, 11, 10]
            .into_iter()
            .map(|uid| CardInfo {
                uid: Some(uid),
                skill_id: Some(30030111),
                ..Default::default()
            })
            .collect(),
    );
    let commands = (0..3).map(|_| RoundCommand::PlayCard {
        card_index: 0,
        target_uid: Some(-1),
        chosen_skill_id: None,
        recorded_skill: None,
    });
    let result = schedule::run_player_commands(
        &mut h.managers,
        &h.pool,
        &h.catalog,
        &mut h.random,
        TargetContext { current_round: 1, ..Default::default() },
        commands,
        1,
        0,
    )
    .unwrap();
    let steps = crate::engine::packet::timeline::project_for_version(&result.frames, 7).unwrap();
    let level = steps.iter().position(|step| {
        step.act_effect.iter().any(|effect| {
            effect.effect_type == Some(183)
                && effect.effect_num == Some(1)
                && effect.config_effect == Some(1)
                && effect.target_id == Some(0)
        })
    }).expect("queue publishes its team level");
    let first = steps.iter().position(|step| step.act_id == Some(30030111))
        .expect("a real skill follows the level");
    assert!(level < first);
    assert_eq!(h.managers.hp.current(-1), 4685, "all three hits receive the 5% opening level");
    assert_eq!(h.managers.rouge.summary().unwrap().syntony_max_level, 1);
}

#[test]
fn ordinary_battles_never_settle_rouge_levels() {
    let mut h = Harness::new(None);
    let ops = queue(&mut h, &[(10, 30030111, 0); 3]);
    assert!(ops.is_empty());
    assert!(h.managers.rouge.summary().is_none());
    assert!(end_incantation_round(&mut h.managers).is_empty());
}
