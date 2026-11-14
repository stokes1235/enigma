use super::*;

#[test]
fn independent_instances_use_consecutive_normal_uids() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        version: Some(7),
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let command = || {
        BuffCommand::GrantIndependent(BuffGrant {
            origin: CommandOrigin {
                domain: RuleDomain::BuffAct,
                key: DefinitionKey::new(884, "AddToBuffEntity3"),
            },
            source_uid: 10,
            target_uid: 10,
            buff_id: 31_080_145,
            amount: Some(1),
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    let first = manager
        .execute(&HpManager::default(), command())
        .unwrap()
        .change
        .added
        .unwrap()
        .buff
        .uid;
    let second = manager
        .execute(&HpManager::default(), command())
        .unwrap()
        .change
        .added
        .unwrap()
        .buff
        .uid;

    assert_eq!(first, Some(1002));
    assert_eq!(second, Some(1004));
}

#[test]
fn include_type_sixteen_refreshes_count_on_the_existing_uid() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        version: Some(7),
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let command = || {
        BuffCommand::Grant(BuffGrant {
            origin: CommandOrigin {
                domain: RuleDomain::BuffAct,
                key: DefinitionKey::new(928, "AddToTarget"),
            },
            source_uid: 10,
            target_uid: 10,
            buff_id: 31130123,
            amount: None,
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    let first = manager
        .execute(&HpManager::default(), command())
        .unwrap()
        .change
        .added
        .unwrap()
        .buff;
    let second = manager
        .execute(&HpManager::default(), command())
        .unwrap()
        .change
        .refreshed
        .pop()
        .unwrap()
        .after;

    assert_eq!(first.uid, Some(1002));
    assert_eq!(first.count, Some(1));
    assert_eq!(second.uid, first.uid);
    assert_eq!(second.count, Some(2));
}

#[test]
fn status_immunity_rejects_the_grant_and_consumes_its_charge() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                buffs: vec![BuffInfo {
                    buff_id: Some(31270408),
                    uid: Some(20),
                    count: Some(1),
                    duration: Some(1),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    let changes = manager
        .execute(
            &HpManager::default(),
            BuffCommand::Grant(BuffGrant {
                origin: CommandOrigin {
                    domain: RuleDomain::Behavior,
                    key: DefinitionKey::new(1, "AddBuff"),
                },
                source_uid: -2,
                target_uid: -1,
                buff_id: 4010,
                amount: None,
                occurrences: 1,
                child_uid_reservations: 0,
            }),
        )
        .unwrap();

    assert_eq!(
        changes
            .change
            .rejected
            .as_ref()
            .map(|result| result.blocker_buff_id),
        Some(31270408)
    );
    assert_eq!(changes.change.removed.len(), 1);
    assert!(!manager.has_buff_id(-1, 31270408));
    assert!(!manager.has_buff_id(-1, 4010));
}

#[test]
fn static_control_immunity_rejects_without_consuming_the_carrier() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                buffs: vec![BuffInfo {
                    buff_id: Some(5140006),
                    uid: Some(20),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    let changes = manager
        .execute(
            &HpManager::default(),
            BuffCommand::Grant(BuffGrant {
                origin: CommandOrigin {
                    domain: RuleDomain::Behavior,
                    key: DefinitionKey::new(1, "AddBuff"),
                },
                source_uid: -2,
                target_uid: -1,
                buff_id: 4010,
                amount: None,
                occurrences: 1,
                child_uid_reservations: 0,
            }),
        )
        .unwrap();

    assert_eq!(
        changes
            .change
            .rejected
            .as_ref()
            .map(|result| result.blocker_buff_id),
        Some(5140006)
    );
    assert!(changes.change.removed.is_empty());
    assert!(manager.has_buff_id(-1, 5140006));
    assert!(!manager.has_buff_id(-1, 4010));
    assert!(matches!(
        changes.events().as_slice(),
        [crate::engine::event::payload::BattleEvent::BuffRejected(rejected)]
            if rejected.source_uid == -2
                && rejected.target_uid == -1
                && rejected.buff_id == 4010
                && rejected.type_id > 0
                && rejected.blocker_buff_id == 5140006
    ));
}

#[test]
fn full_daze_resistance_resists_daze_like_immunity() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    let entity = |uid, model_id| FightEntityInfo {
        uid: Some(uid),
        model_id: Some(model_id),
        ..Default::default()
    };
    manager.seed(&Fight {
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 109380001), entity(-2, 0)],
            ..Default::default()
        }),
        ..Default::default()
    });
    let daze = |target_uid| {
        BuffCommand::Grant(BuffGrant {
            origin: CommandOrigin {
                domain: RuleDomain::Behavior,
                key: DefinitionKey::new(1, "AddBuff"),
            },
            source_uid: 10,
            target_uid,
            buff_id: 4011,
            amount: None,
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    let resisted = manager.execute(&HpManager::default(), daze(-1)).unwrap();
    let applied = manager.execute(&HpManager::default(), daze(-2)).unwrap();

    assert!(
        resisted
            .change
            .rejected
            .as_ref()
            .is_some_and(|rejected| rejected.resisted)
    );
    assert!(!manager.has_buff_id(-1, 4011));
    assert!(matches!(
        resisted.events().as_slice(),
        [crate::engine::event::payload::BattleEvent::BuffRejected(rejected)]
            if rejected.target_uid == -1 && rejected.buff_id == 4011
    ));
    assert_eq!(
        crate::engine::packet::effect::EffectPacket::buff_changes(&resisted.change)
            .first()
            .and_then(|effect| effect.effect_type),
        Some(sonettobuf::effect_type_enum::EffectType::Resistances as i32)
    );
    assert!(applied.change.rejected.is_none());
    assert!(manager.has_buff_id(-2, 4011));
}

#[test]
fn team_status_immunity_consumes_the_shared_carrier_budget() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        attacker: Some(FightTeam {
            entitys: vec![
                FightEntityInfo {
                    uid: Some(10),
                    team_type: Some(1),
                    buffs: vec![BuffInfo {
                        buff_id: Some(31430144),
                        uid: Some(20),
                        from_uid: Some(10),
                        act_info: vec![sonettobuf::BuffActInfo {
                            act_id: Some(1126),
                            param: vec![2],
                            str_param: Some(String::new()),
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FightEntityInfo {
                    uid: Some(11),
                    team_type: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    });
    let grant_control = || {
        BuffCommand::Grant(BuffGrant {
            origin: CommandOrigin {
                domain: RuleDomain::Behavior,
                key: DefinitionKey::new(1, "AddBuff"),
            },
            source_uid: -1,
            target_uid: 11,
            buff_id: 4010,
            amount: None,
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    for remaining in [1, 0] {
        let changes = manager
            .execute(&HpManager::default(), grant_control())
            .unwrap();
        assert_eq!(
            changes
                .change
                .rejected
                .as_ref()
                .map(|result| result.blocker_buff_id),
            Some(31430144)
        );
        assert!(!manager.has_buff_id(11, 4010));
        assert_eq!(
            manager.snapshot(10, 20).unwrap().act_info[0].param,
            [remaining]
        );
    }

    let changes = manager
        .execute(&HpManager::default(), grant_control())
        .unwrap();
    assert!(changes.change.rejected.is_none());
    assert!(manager.has_buff_id(10, 31430144));
    assert!(manager.has_buff_id(11, 4010));
}

#[test]
fn rejected_layer_buff_uses_its_configured_child_uid_lane() {
    crate::test_support::init_config();
    let mut manager = BuffManager::default();
    manager.seed(&Fight {
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                buffs: vec![BuffInfo {
                    buff_id: Some(530000417),
                    uid: Some(100055),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    let changes = manager
        .execute(
            &HpManager::default(),
            BuffCommand::Grant(BuffGrant {
                origin: CommandOrigin {
                    domain: RuleDomain::Behavior,
                    key: DefinitionKey::new(1, "AddBuff"),
                },
                source_uid: -1,
                target_uid: -1,
                buff_id: 530000111,
                amount: None,
                occurrences: 1,
                child_uid_reservations: 0,
            }),
        )
        .unwrap();

    assert_eq!(changes.change.rejected.unwrap().buff.uid, Some(100056));
}

#[test]
fn child_uid_grant_refreshes_an_existing_single_buff() {
    crate::test_support::init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                current_hp: Some(100),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut hp = HpManager::default();
    hp.seed(&fight);
    let mut manager = BuffManager::default();
    manager.seed(&fight);
    let command = || {
        BuffCommand::GrantUsingChildUid(BuffGrant {
            origin: CommandOrigin {
                domain: RuleDomain::Behavior,
                key: DefinitionKey::new(60142, "ConsumePowerAddBuff"),
            },
            source_uid: 10,
            target_uid: 10,
            buff_id: 31130123,
            amount: None,
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    let first = manager.execute(&hp, command()).unwrap();
    let first_uid = first.change.added.unwrap().buff.uid;
    let second = manager.execute(&hp, command()).unwrap();

    assert!(second.change.added.is_none());
    assert!(second.change.removed.is_empty());
    assert_eq!(second.change.refreshed.len(), 1);
    assert_eq!(second.change.refreshed[0].after.uid, first_uid);
}

#[test]
fn grant_snapshots_attribute_from_current_injury_bank() {
    crate::test_support::init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![
                FightEntityInfo {
                    uid: Some(10),
                    current_hp: Some(10_000),
                    buffs: vec![BuffInfo {
                        buff_id: Some(30800141),
                        uid: Some(1),
                        from_uid: Some(10),
                        act_common_params: Some("770#174#3489".to_owned()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                FightEntityInfo {
                    uid: Some(20),
                    current_hp: Some(10_000),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut hp = HpManager::default();
    hp.seed(&fight);
    let mut manager = BuffManager::default();
    manager.seed(&fight);

    let changes = manager
        .execute(
            &hp,
            BuffCommand::Grant(BuffGrant {
                origin: CommandOrigin {
                    domain: RuleDomain::Behavior,
                    key: DefinitionKey::new(60039, "RealDamageSelfAndAddBuffToTarget"),
                },
                source_uid: 10,
                target_uid: 20,
                buff_id: 30800111,
                amount: None,
                occurrences: 1,
                child_uid_reservations: 0,
            }),
        )
        .unwrap();

    assert_eq!(
        changes
            .change
            .added
            .unwrap()
            .buff
            .act_common_params
            .as_deref(),
        Some("767#317")
    );
}

#[test]
fn enhanced_passive_variant_retains_its_state_and_silently_consumes_the_attempt_uid() {
    crate::test_support::init_config();
    let fight = Fight {
        version: Some(7),
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(10),
                current_hp: Some(100),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut manager = BuffManager::default();
    let hp = HpManager::default();
    manager.seed(&fight);
    let origin = CommandOrigin {
        domain: RuleDomain::Behavior,
        key: DefinitionKey::new(1, "AddBuff"),
    };
    let grant = |buff_id| {
        BuffCommand::Grant(BuffGrant {
            origin,
            source_uid: 10,
            target_uid: 10,
            buff_id,
            amount: None,
            occurrences: 1,
            child_uid_reservations: 0,
        })
    };

    let enhanced = manager.execute(&hp, grant(30030208)).unwrap();
    let base_attempt = manager.execute(&hp, grant(30030207)).unwrap();
    let following = manager.execute(&hp, grant(370002300)).unwrap();

    assert_eq!(enhanced.change.added.unwrap().buff.uid, Some(1002));
    assert_eq!(base_attempt.change, BuffReplaceResult::default());
    assert!(base_attempt.events().is_empty());
    assert!(manager.has_buff_id(10, 30030208));
    assert!(!manager.has_buff_id(10, 30030207));
    assert_eq!(following.change.added.unwrap().buff.uid, Some(1006));
}

#[test]
fn general_immunity_blocks_three_bad_statuses_without_expanding_control_only_carriers() {
    crate::test_support::init_config();
    let seeded = |buff_id| {
        let mut manager = BuffManager::default();
        manager.seed(&Fight {
            defender: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(-1),
                    buffs: vec![BuffInfo {
                        buff_id: Some(buff_id),
                        uid: Some(20),
                        duration: Some(1),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        manager
    };
    let attempt = |manager: &mut BuffManager, buff_id| {
        manager
            .execute(
                &HpManager::default(),
                BuffCommand::Grant(BuffGrant {
                    origin: CommandOrigin {
                        domain: RuleDomain::Behavior,
                        key: DefinitionKey::new(1, "AddBuff"),
                    },
                    source_uid: -2,
                    target_uid: -1,
                    buff_id,
                    amount: None,
                    occurrences: 1,
                    child_uid_reservations: 0,
                }),
            )
            .unwrap()
            .change
            .rejected
            .is_some()
    };
    let mut general = seeded(500212);
    for bad in [4010, 90152, 7121] {
        assert!(
            attempt(&mut general, bad),
            "general immunity should block {bad}"
        );
    }
    assert!(
        !attempt(&mut general, 90001),
        "positive buffs must still apply"
    );
    assert!(general.has_buff_id(-1, 500212), "immunity is not consumed");
    let mut control_only = seeded(5091);
    assert!(attempt(&mut control_only, 4010));
    assert!(!attempt(&mut control_only, 90152));
    assert!(!attempt(&mut control_only, 7121));
}
