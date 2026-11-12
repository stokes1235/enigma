//! Exercise the actual server adapter, card execution and settlement transaction.
//! Enemy HP/attack are controlled fixtures; artefact growth is never fabricated.

use super::*;
use crate::logic::rouge::{bag::RunBag, flow, map::MapNode};
use battle::dungeon::{BuiltFight, FightOptions};
use database::{
    db::game::{battle as battle_db, rouge},
    models::game::{heros::UserHeroModel, rouge::RougeRun},
};
use sonettobuf::{
    BeginRoundOper, BeginRoundRequest, BuffInfo, FightGroup, FightTeam, HeroAttribute,
};
use sqlx::sqlite::SqlitePoolOptions;

const USER: i64 = 188;
const HERO: i32 = 3003;
const EVENT: i32 = 101101;
const NODE: i32 = 10;
const SCARAB: i32 = 8320310;

struct Fixture {
    db: SqlitePool,
    group: FightGroup,
    actor: i64,
    item: i64,
    episode: i32,
    battle: i32,
}

impl Fixture {
    async fn new() -> Self {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (?, 'rouge-real-growth', 0, 0)",
        )
        .bind(USER)
        .execute(&db)
        .await
        .unwrap();
        let actor = UserHeroModel::new(USER, db.clone())
            .create_hero(HERO)
            .await
            .unwrap();
        rouge::get_or_create_start(&db, USER, 1).await.unwrap();
        sqlx::query(
            "UPDATE user_rouge_start SET state = 4, difficulty = 1, style = 1,
             hero_ids = '[3003]' WHERE user_id = ? AND season = 1",
        )
        .bind(USER)
        .execute(&db)
        .await
        .unwrap();
        let graph = LayerMap {
            layer_id: 111,
            stages: 3,
            nodes: vec![
                MapNode {
                    stage: 0,
                    node_id: 1,
                    last: vec![],
                    next: vec![NODE],
                    event_id: 0,
                    node_type: 0,
                },
                MapNode {
                    stage: 1,
                    node_id: NODE,
                    last: vec![1],
                    next: vec![20],
                    event_id: EVENT,
                    node_type: 2,
                },
                MapNode {
                    stage: 2,
                    node_id: 20,
                    last: vec![NODE],
                    next: vec![],
                    event_id: 108101,
                    node_type: 8,
                },
            ],
        };
        let team = RougeTeamInfo {
            battle_hero_list: vec![RougeBattleHero {
                index: Some(1),
                hero_id: Some(HERO),
                ..Default::default()
            }],
            hero_life_list: vec![RougeHeroLife {
                hero_id: Some(HERO),
                life: Some(1000),
            }],
            hero_info_list: vec![RougeHeroInfo {
                hero_id: Some(HERO),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut bag = RunBag {
            rows: 8,
            cols: 8,
            items: vec![],
            uid_counter: 0,
        };
        let item = bag.add(SCARAB).unwrap().uid;
        bag.place(item, -1, -1, 0).unwrap();
        sqlx::query(
            "INSERT INTO user_rouge_run
             (user_id, season, layer_index, layer_id, stage, node_id, seed, graph,
              coin, power, power_limit, team, bag, middle_state, details, updated_at)
             VALUES (?, 1, 0, 111, 1, ?, 41, ?, 0, 40, 40, ?, ?, '{}', ?, 0)",
        )
        .bind(USER)
        .bind(NODE)
        .bind(serde_json::to_string(&graph).unwrap())
        .bind(serde_json::to_string(&team).unwrap())
        .bind(serde_json::to_string(&bag).unwrap())
        .bind(serde_json::to_string(&flow::Details::default()).unwrap())
        .execute(&db)
        .await
        .unwrap();
        let episode = config::configs::get().rouge_fight_episode(EVENT).unwrap();
        let battle = config::configs::get()
            .episode
            .get(episode)
            .unwrap()
            .battle_id;
        Self {
            db,
            group: FightGroup {
                hero_list: vec![actor],
                ..Default::default()
            },
            actor,
            item,
            episode,
            battle,
        }
    }

    async fn run(&self) -> RougeRun {
        rouge::get_run(&self.db, USER, 1).await.unwrap().unwrap()
    }

    async fn build(&self) -> BuiltFight {
        crate::logic::battle_setup::dungeon::build_fight(
            &self.db,
            USER,
            self.episode,
            self.battle,
            &self.group,
            FightOptions::default(),
            None,
        )
        .await
        .unwrap()
    }

    async fn store_bag(&self, bag: &RunBag) {
        sqlx::query("UPDATE user_rouge_run SET bag = ? WHERE user_id = ? AND season = 1")
            .bind(serde_json::to_string(bag).unwrap())
            .bind(USER)
            .execute(&self.db)
            .await
            .unwrap();
    }

    async fn fight_id(&self) -> i64 {
        battle_db::create_fight_instance(
            &self.db,
            battle_db::NewFightInstance {
                user_id: USER,
                episode_id: self.episode,
                battle_id: self.battle,
                multiplication: 1,
                entry_cost: "{}",
                checkpoint: "{}",
                created_at: 0,
            },
        )
        .await
        .unwrap()
    }
}

fn incantation_rate(built: &BuiltFight, actor: i64) -> i32 {
    built
        .sp_attributes
        .iter()
        .find(|(uid, _)| *uid == actor)
        .and_then(|(_, attr)| attr.normal_skill_rate)
        .unwrap_or_default()
}

fn start_controlled(mut built: BuiltFight, actor: i64, enemy_hp: i32) -> battle::Battle {
    // Preserve the production adapter's ex/sp attributes and frozen context.
    // Remove unrelated waves/AI. Real rank-one draws are moved and composed
    // into a rank-two ordinary card, never replaced by a fabricated counter.
    built.fight.battle_id = Some(0);
    let attacker = built.fight.attacker.as_mut().unwrap();
    assert_eq!(attacker.entitys.len(), 1);
    let hero = &mut attacker.entitys[0];
    assert_eq!(hero.uid, Some(actor));
    hero.attr.as_mut().unwrap().attack = Some(10_000);
    hero.passive_skill.clear();
    // Verified plain 709#1 carrier supplies one extra action point, making
    // the single hero's move+play sequence legal without free-card semantics.
    hero.buffs = vec![BuffInfo {
        uid: Some(900),
        buff_id: Some(2_137_001),
        from_uid: Some(actor),
        layer: Some(1),
        ..Default::default()
    }];
    built.fight.defender = Some(FightTeam {
        entitys: vec![FightEntityInfo {
            uid: Some(-1),
            team_type: Some(2),
            level: Some(1),
            current_hp: Some(enemy_hp),
            attr: Some(HeroAttribute {
                hp: Some(enemy_hp),
                attack: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    });
    battle::Battle::start(
        battle::catalog::BattleCatalog::new(config::configs::get()),
        built,
        42,
    )
    .unwrap()
}

fn cast_rank_two(battle: &mut battle::Battle) -> i32 {
    assert!(battle.card_info_push().act_point.unwrap_or_default() >= 2);
    let hand = battle.card_info_push().card_group;
    let (left, right) = hand
        .iter()
        .enumerate()
        .find_map(|(left, card)| {
            hand.iter()
                .enumerate()
                .skip(left + 2)
                .find_map(|(right, other)| {
                    (card.uid == other.uid
                        && card.skill_id == other.skill_id
                        && !card.temp_card.unwrap_or_default()
                        && !other.temp_card.unwrap_or_default()
                        && matches!(card.card_type, None | Some(0)))
                    .then_some((left, right))
                })
        })
        .expect("same-seed real draws must contain a movable pair of ordinary rank-one cards");
    assert_eq!(
        left, 0,
        "keep the controlled pair's post-composition position explicit"
    );
    let actor = battle.entity_info(hand[left].uid.unwrap()).unwrap();
    let rank_one = hand[left].skill_id.unwrap();
    let skill_id = [&actor.skill_group1, &actor.skill_group2]
        .into_iter()
        .find_map(|group| {
            group
                .iter()
                .position(|id| *id == rank_one)
                .and_then(|index| group.get(index + 1))
                .copied()
        })
        .expect("the character's real basic skill group supplies its next rank");
    assert_eq!(
        config::configs::get()
            .skill
            .get(skill_id)
            .unwrap()
            .skill_rank,
        2
    );
    let reply = battle::dungeon::begin_round(
        battle,
        BeginRoundRequest {
            opers: vec![
                BeginRoundOper {
                    oper_type: Some(1),
                    param1: Some(right as i32 + 1),
                    param2: Some(left as i32 + 2),
                    ..Default::default()
                },
                BeginRoundOper {
                    oper_type: Some(2),
                    param1: Some(left as i32 + 1),
                    to_id: Some(-1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    )
    .unwrap();
    fn contains_skill(steps: &[sonettobuf::FightStep], skill_id: i32) -> bool {
        steps.iter().any(|step| {
            step.act_id == Some(skill_id)
                || step.act_effect.iter().any(|effect| {
                    effect
                        .fight_step
                        .as_ref()
                        .is_some_and(|step| contains_skill(std::slice::from_ref(step), skill_id))
                })
        })
    }
    assert!(contains_skill(&reply.round.unwrap().fight_step, skill_id));
    let summary = battle.rouge_summary().unwrap();
    assert_eq!((summary.casts, summary.rank2_casts), (1, 1));
    skill_id
}

#[tokio::test]
async fn entry_resources_are_frozen_before_battle_and_commit_atomically() {
    let f = Fixture::new().await;
    let mut bag: RunBag = serde_json::from_str(&f.run().await.bag).unwrap();
    let disk = bag.add(8140101).unwrap().uid;
    bag.place(disk, -1, -1, 0).unwrap();
    f.store_bag(&bag).await;
    sqlx::query("UPDATE user_rouge_run SET power = 20 WHERE user_id = ? AND season = 1")
        .bind(USER)
        .execute(&f.db)
        .await
        .unwrap();
    let request = sonettobuf::StartDungeonRequest {
        chapter_id: Some(ROUGE_CHAPTER_ID),
        episode_id: Some(f.episode),
        fight_group: Some(f.group.clone()),
        ..Default::default()
    };
    let mut active = ActiveBattle::prepare(&f.db, USER, f.episode, f.battle, request)
        .await
        .unwrap();
    assert_eq!(
        f.run().await.power,
        20,
        "preparation must not write entry effects"
    );
    assert_eq!(active.runtime.rouge_resources().unwrap().power, 22);
    let attacker = active.runtime.reconnect_state().0.attacker.unwrap();
    assert_eq!(
        attacker.power,
        Some(15),
        "ordinary battle energy is separate"
    );
    assert!(
        attacker
            .ex_team_str
            .as_deref()
            .unwrap()
            .starts_with("0#22#40#")
    );
    assert_eq!(
        attacker
            .skill_infos
            .iter()
            .map(|skill| (skill.skill_id, skill.need_power, skill.r#type))
            .collect::<Vec<_>>(),
        vec![
            (Some(800000002), Some(5), Some(1)),
            (Some(800000001), Some(3), Some(1))
        ],
    );

    // A competing map change invalidates the complete prepared snapshot; it
    // must neither grant the entry resource nor create an active fight.
    sqlx::query("UPDATE user_rouge_run SET coin = 1 WHERE user_id = ? AND season = 1")
        .bind(USER)
        .execute(&f.db)
        .await
        .unwrap();
    assert!(
        active
            .activate(&f.db, USER, &Default::default())
            .await
            .is_err()
    );
    assert_eq!((f.run().await.power, f.run().await.coin), (20, 1));
    assert!(
        battle_db::load_active_fight(&f.db, USER)
            .await
            .unwrap()
            .is_none()
    );

    sqlx::query("UPDATE user_rouge_run SET coin = 0 WHERE user_id = ? AND season = 1")
        .bind(USER)
        .execute(&f.db)
        .await
        .unwrap();
    active
        .activate(&f.db, USER, &Default::default())
        .await
        .unwrap();
    assert_eq!(f.run().await.power, 22);
    active
        .use_cloth_skill(sonettobuf::UseClothSkillRequest {
            skill_id: Some(800000002),
            r#type: Some(2),
            from_id: Some(0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(active.runtime.rouge_resources().unwrap().power, 17);
    assert_eq!(
        f.run().await.power,
        22,
        "combat spending is saved only at settlement"
    );
    battle_db::update_fight_checkpoint(
        &f.db,
        USER,
        active.fight_id.unwrap(),
        &active.checkpoint_json().unwrap(),
    )
    .await
    .unwrap();
    let record = battle_db::load_active_fight(&f.db, USER)
        .await
        .unwrap()
        .unwrap();
    let restored = ActiveBattle::restore(&f.db, USER, record).await.unwrap();
    assert_eq!(restored.runtime.rouge_resources().unwrap().power, 17);
    assert_eq!(
        f.run().await.power,
        22,
        "reconnect must not grant entry twice"
    );
}

#[tokio::test]
async fn actual_catalyst_spend_is_saved_once_with_victory() {
    let f = Fixture::new().await;
    let built = f.build().await;
    let mut active = ActiveBattle {
        fight_id: Some(f.fight_id().await),
        chapter_id: ROUGE_CHAPTER_ID,
        episode_id: f.episode,
        battle_id: f.battle,
        runtime: start_controlled(built, f.actor, 1),
        fight_group: Some(f.group.clone()),
        ..Default::default()
    };
    active
        .use_cloth_skill(sonettobuf::UseClothSkillRequest {
            skill_id: Some(800000002),
            r#type: Some(2),
            from_id: Some(0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(active.runtime.rouge_resources().unwrap().spent_power, 5);
    assert_eq!(f.run().await.power, 40);
    cast_rank_two(&mut active.runtime);
    assert_eq!(active.runtime.outcome(), battle::BattleOutcome::Victory);
    settle_active(&f.db, USER, &active).await.unwrap();
    assert_eq!(f.run().await.power, 35);
    assert_eq!(flow::details(&f.run().await).consume_power, 5);
    assert!(settle_active(&f.db, USER, &active).await.is_err());
    assert_eq!(f.run().await.power, 35);
    assert_eq!(flow::details(&f.run().await).consume_power, 5);
}

#[tokio::test]
async fn catalyst_spending_and_recovery_update_the_right_entrust_counter() {
    let f = Fixture::new().await;
    let mut base = f.run().await;
    base.coin = 50;
    base.spent_coin = 4;
    let mut details = flow::details(&base);
    details.consume_power = 2;
    base.details = serde_json::to_string(&details).unwrap();
    let initial = battle::rouge::Catalyst {
        coin: 50,
        power: 40,
        power_limit: 40,
        ..Default::default()
    };
    let result = battle::rouge::Resources {
        coin: 47,
        power: 38,
        power_limit: 40,
        spent_coin: 10,
        spent_power: 5,
    };
    for (kind, amount) in [(1, 7), (2, 10), (5, 3), (6, 5)] {
        let entrust = config::configs::get()
            .rouge_entrust
            .iter()
            .find(|entry| entry.r#type == kind)
            .expect("test requires each resource entrust type");
        let mut run = base.clone();
        run.middle_state = serde_json::json!({
            "pieces": [], "entrust_id": entrust.id, "entrust_progress": 0,
            "completed_entrusts": [], "middle_layer_id": run.middle_layer_id
        })
        .to_string();
        crate::logic::rouge::apply_battle_resources(&mut run, &initial, &result).unwrap();
        let middle: serde_json::Value = serde_json::from_str(&run.middle_state).unwrap();
        assert_eq!(middle["entrust_progress"], amount);
        assert_eq!((run.coin, run.power, run.spent_coin), (47, 38, 14));
        assert_eq!(flow::details(&run).consume_power, 7);
    }
    let mut stale = base.clone();
    stale.power -= 1;
    assert!(crate::logic::rouge::apply_battle_resources(&mut stale, &initial, &result).is_err());
    assert_eq!((stale.power, stale.spent_coin), (39, 4));
    let mut invalid = result.clone();
    invalid.spent_power = 1; // a net loss of two cannot arise from spending one
    assert!(crate::logic::rouge::apply_battle_resources(&mut base, &initial, &invalid).is_err());
    assert_eq!((base.power, base.spent_coin), (40, 4));
}

#[tokio::test]
async fn placed_turn_artefacts_trigger_from_a_real_cast_and_round_transition() {
    let f = Fixture::new().await;
    let mut bag: RunBag = serde_json::from_str(&f.run().await.bag).unwrap();
    for item_id in [8110213, 8130803, 8230027, 8210314] {
        let uid = bag.add(item_id).unwrap().uid;
        bag.place(uid, -1, -1, 0).unwrap();
    }
    f.store_bag(&bag).await;
    let built = f.build().await;
    let mut fight = start_controlled(built, f.actor, 1_000_000);
    let amount = |fight: &battle::Battle, buff_id: i32| {
        fight
            .entity_info(f.actor)
            .unwrap()
            .buffs
            .iter()
            .filter(|buff| buff.buff_id == Some(buff_id))
            .map(|buff| buff.layer.unwrap_or_default())
            .sum::<i32>()
    };
    assert_eq!(amount(&fight, 90071), 1, "clockwork triggers at opening");
    cast_rank_two(&mut fight);
    assert_eq!(amount(&fight, 90091), 5, "the cane follows the real cast");
    assert!(
        amount(&fight, 90011) >= 2,
        "the slingshot grants Lucky for the selected cast"
    );
    assert_eq!(amount(&fight, 90001), 1, "the glove triggers at round end");
    assert_eq!(
        amount(&fight, 90071),
        2,
        "clockwork triggers again in round two"
    );
}

#[tokio::test]
async fn pendulum_exposes_free_moves_and_does_not_grant_moxie_for_moving() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE user_rouge_start SET style = 4 WHERE user_id = ? AND season = 1")
        .bind(USER)
        .execute(&f.db)
        .await
        .unwrap();
    let mut fight = start_controlled(f.build().await, f.actor, 1_000_000);
    let mut control_built = f.build().await;
    let control_context = control_built.rouge.as_mut().unwrap();
    control_context.catalyst.as_mut().unwrap().style_id = 1;
    // Match Pendulum's +2 hand size without changing the move rule. This
    // keeps the real seeded hand and every composition identical.
    control_context.artefacts.push(battle::rouge::Artefact {
        uid: 9999, item_id: 8330306, ..Default::default()
    });
    let mut control = start_controlled(control_built, f.actor, 1_000_000);
    assert_eq!(fight.card_info_push().extra_move_act, Some(-1));
    assert_eq!(control.card_info_push().extra_move_act, Some(0));
    let hand = fight.card_info_push().card_group;
    assert_eq!(hand, control.card_info_push().card_group);
    let (left, right) = hand
        .iter()
        .enumerate()
        .find_map(|(left, card)| {
            hand.iter()
                .enumerate()
                .skip(left + 2)
                .find_map(|(right, other)| {
                    (card.uid == other.uid
                        && card.skill_id == other.skill_id
                        && !card.temp_card.unwrap_or_default()
                        && !other.temp_card.unwrap_or_default())
                    .then_some((left, right))
                })
        })
        .expect("seeded ordinary hand supplies a non-adjacent pair");
    let request = BeginRoundRequest {
        opers: vec![BeginRoundOper {
            oper_type: Some(1), param1: Some(right as i32 + 1),
            param2: Some(left as i32 + 2), ..Default::default()
        }], ..Default::default()
    };
    battle::dungeon::begin_round(&mut fight, request.clone()).unwrap();
    battle::dungeon::begin_round(&mut control, request).unwrap();
    assert_eq!(
        fight.entity_info(f.actor).unwrap().ex_point.unwrap_or_default() + 1,
        control.entity_info(f.actor).unwrap().ex_point.unwrap_or_default(),
        "identical compositions still grant Moxie; only the normal move adds one more"
    );
}

#[tokio::test]
async fn actual_scarab_cast_settles_once_from_frozen_source_and_next_fight_applies_growth_once() {
    let f = Fixture::new().await;
    assert!(can_start(&f.db, USER, f.episode).await.unwrap());
    let first = f.build().await;
    let base_rate = incantation_rate(&first, f.actor);
    assert_eq!(
        base_rate, 0,
        "this fixture must isolate the Scarab growth lane"
    );
    let frozen = first.rouge.as_ref().unwrap();
    assert!(
        frozen
            .growth_sources
            .iter()
            .any(|source| source.uid == f.item && source.actor_uids == vec![f.actor])
    );
    assert_eq!(frozen.permanent_growth[&f.item]["growth_pct"], 0);

    // Moving the item after fight preparation must not revoke this fight's
    // source. It does prevent the following fight from inheriting its bonus.
    let mut bag: RunBag = serde_json::from_str(&f.run().await.bag).unwrap();
    bag.remove(f.item).unwrap();
    f.store_bag(&bag).await;
    let fight_id = f.fight_id().await;
    let mut active = ActiveBattle {
        fight_id: Some(fight_id),
        chapter_id: ROUGE_CHAPTER_ID,
        episode_id: f.episode,
        battle_id: f.battle,
        runtime: start_controlled(first, f.actor, 1),
        fight_group: Some(f.group.clone()),
        ..Default::default()
    };
    let cast_skill = cast_rank_two(&mut active.runtime);
    assert_eq!(active.runtime.outcome(), battle::BattleOutcome::Victory);
    assert_eq!(active.runtime.rouge_counters()[&f.item]["growth_pct"], 2);
    let settlement = settle_active(&f.db, USER, &active).await.unwrap();
    assert_eq!(settlement.result.is_win, Some(true));
    let settled = f.run().await;
    let state = flow::details(&settled).artifact_state;
    assert_eq!(state.permanent_rates[&f.item][&214], 20);
    assert_eq!(state.battle_growth[&fight_id][&f.item], 2);
    let stored_bag: RunBag = serde_json::from_str(&settled.bag).unwrap();
    assert!(!RunBag::is_placed(stored_bag.item(f.item).unwrap()));
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT active FROM fight_instances WHERE id = ?")
            .bind(fight_id)
            .fetch_one(&f.db)
            .await
            .unwrap(),
        0
    );

    // settle_active must roll back the entire duplicate, including automatic
    // coin/experience and badge/growth ledgers, when its instance is finished.
    assert!(settle_active(&f.db, USER, &active).await.is_err());
    let duplicate = f.run().await;
    assert_eq!(duplicate.details, settled.details);
    assert_eq!(duplicate.bag, settled.bag);
    assert_eq!(
        (duplicate.coin, duplicate.team_exp),
        (settled.coin, settled.team_exp)
    );

    let warehouse = f.build().await;
    assert_eq!(incantation_rate(&warehouse, f.actor), base_rate);
    assert!(warehouse.rouge.as_ref().unwrap().growth_sources.is_empty());
    assert!(
        warehouse
            .rouge
            .as_ref()
            .unwrap()
            .permanent_growth
            .is_empty()
    );

    bag.place(f.item, -1, -1, 0).unwrap();
    f.store_bag(&bag).await;
    let next = f.build().await;
    assert_eq!(incantation_rate(&next, f.actor), base_rate + 20);
    assert_eq!(
        next.rouge.as_ref().unwrap().permanent_growth[&f.item]["growth_pct"],
        2
    );

    // The two production-built fights differ only in placement/growth. Their
    // same-seed first real cast must gain two percent, not four percent from
    // adding the frozen baseline again inside the battle engine.
    let mut without = start_controlled(warehouse, f.actor, 1_000_000);
    let mut with = start_controlled(next, f.actor, 1_000_000);
    assert_eq!(cast_rank_two(&mut without), cast_skill);
    assert_eq!(cast_rank_two(&mut with), cast_skill);
    let damage_without = without.rouge_summary().unwrap().damage_dealt;
    let damage_with = with.rouge_summary().unwrap().damage_dealt;
    assert!(
        damage_without > 10_000,
        "avoid rounding hiding a double application"
    );
    assert!(
        (damage_with * 100 - damage_without * 102).abs() <= 200,
        "next-fight growth must be applied exactly once: {damage_without} -> {damage_with}"
    );
    assert!(without.rouge_counters().is_empty());
    assert_eq!(
        with.rouge_counters()[&f.item]["growth_pct"],
        2,
        "new fight reports only new growth, not its old two-point baseline"
    );
}
