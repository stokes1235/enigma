//! Resource rewards delivered when a completed entrust reaches a safe zone.
//!
//! Several base-game entrusts have an empty `interactive`, but their reward is
//! carried by a same-ID `rouge_effect`. Later-area variants reuse the same
//! reward description and the first area's effect. Keep unsupported effects
//! explicit so callers cannot silently consume an unimplemented reward.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resources {
    pub coin: i32,
    pub power: i32,
    pub power_limit: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedReward {
    pub effect_id: i32,
    pub resources: Resources,
    pub coin_delta: i32,
    pub power_delta: i32,
    pub power_limit_delta: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewardError {
    UnknownEntrust,
    InvalidResources,
    InvalidAmount { effect_id: i32 },
    UnsupportedEffect { effect_id: i32, effect_type: i32 },
    MissingCanonicalReward { entrust_id: i32 },
}

/// Computes only supported resource rewards; `None` means there is no resource
/// effect and the caller must still consider the entrust's `interactive`.
pub fn apply_reward(
    tables: &config::GameDB,
    entrust_id: i32,
    current: Resources,
) -> Result<Option<AppliedReward>, RewardError> {
    let entrust = tables
        .rouge_entrust
        .get(entrust_id)
        .ok_or(RewardError::UnknownEntrust)?;
    if current.coin < 0
        || current.power < 0
        || current.power_limit < 0
        || current.power > current.power_limit
    {
        return Err(RewardError::InvalidResources);
    }

    let effect = if let Some(effect) = tables.rouge_effect.get(entrust_id) {
        effect
    } else {
        // These are the eight base-game reward descriptions shared by the
        // first three areas. Require the table descriptions and objective type
        // to agree before using the first-area effect.
        let suffix = entrust_id % 10_000;
        if !matches!(entrust_id / 10_000, 121 | 131)
            || !matches!(suffix, 201 | 202 | 205 | 206 | 211 | 212 | 215 | 216)
        {
            return Ok(None);
        }
        let canonical_id = 1_110_000 + suffix;
        let descriptions_match = tables
            .rouge_piece_select
            .get(entrust_id)
            .zip(tables.rouge_piece_select.get(canonical_id))
            .is_some_and(|(variant, canonical)| {
                !variant.talk_desc.is_empty() && variant.talk_desc == canonical.talk_desc
            });
        let objective_matches = tables
            .rouge_entrust
            .get(canonical_id)
            .is_some_and(|canonical| canonical.r#type == entrust.r#type);
        if !descriptions_match || !objective_matches {
            return Err(RewardError::MissingCanonicalReward { entrust_id });
        }
        tables
            .rouge_effect
            .get(canonical_id)
            .ok_or(RewardError::MissingCanonicalReward { entrust_id })?
    };

    if !matches!(effect.r#type, 14 | 16 | 17) {
        return Err(RewardError::UnsupportedEffect {
            effect_id: effect.id,
            effect_type: effect.r#type,
        });
    }
    let amount = effect
        .type_param
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|amount| *amount >= 0)
        .ok_or(RewardError::InvalidAmount {
            effect_id: effect.id,
        })?;
    let mut resources = current;
    match effect.r#type {
        14 => resources.coin = resources.coin.saturating_add(amount),
        16 => resources.power_limit = resources.power_limit.saturating_add(amount),
        17 => {
            resources.power = resources
                .power
                .saturating_add(amount)
                .min(resources.power_limit)
        }
        _ => unreachable!(),
    }
    Ok(Some(AppliedReward {
        effect_id: effect.id,
        resources,
        coin_delta: resources.coin - current.coin,
        power_delta: resources.power - current.power,
        power_limit_delta: resources.power_limit - current.power_limit,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> &'static config::GameDB {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/excel2json");
        config::init(data.to_str().unwrap()).unwrap();
        config::configs::get()
    }

    fn initial() -> Resources {
        Resources {
            coin: 50,
            power: 25,
            power_limit: 40,
        }
    }

    #[test]
    fn empty_interactive_entrusts_still_grant_their_configured_resource_rewards() {
        let tables = tables();
        for (id, amount) in [
            (1110201, 100),
            (1110202, 150),
            (1110211, 200),
            (1110212, 300),
        ] {
            assert!(tables.rouge_entrust.get(id).unwrap().interactive.is_empty());
            let reward = apply_reward(tables, id, initial()).unwrap().unwrap();
            assert_eq!(reward.effect_id, id);
            assert_eq!(reward.resources.coin, 50 + amount);
            assert_eq!(reward.coin_delta, amount);
            assert_eq!(reward.power_delta, 0);
            assert_eq!(reward.power_limit_delta, 0);
        }
    }

    #[test]
    fn recovery_is_capped_but_maximum_increases_do_not_recover_power() {
        let tables = tables();
        for id in [1110205, 1110215] {
            let reward = apply_reward(tables, id, initial()).unwrap().unwrap();
            assert_eq!(reward.resources.power, 40);
            assert_eq!(reward.power_delta, 15);
            assert_eq!(reward.resources.power_limit, 40);
        }
        for (id, amount) in [(1110206, 25), (1110216, 50)] {
            let reward = apply_reward(tables, id, initial()).unwrap().unwrap();
            assert_eq!(reward.resources.power, 25);
            assert_eq!(reward.resources.power_limit, 40 + amount);
            assert_eq!(reward.power_delta, 0);
            assert_eq!(reward.power_limit_delta, amount);
        }
    }

    #[test]
    fn later_area_variants_require_the_same_configured_reward_description() {
        let tables = tables();
        for area in [121, 131] {
            for suffix in [201, 202, 205, 206, 211, 212, 215, 216] {
                let canonical_id = 1_110_000 + suffix;
                let canonical = apply_reward(tables, canonical_id, initial())
                    .unwrap()
                    .unwrap();
                let variant = apply_reward(tables, area * 10_000 + suffix, initial())
                    .unwrap()
                    .unwrap();
                assert_eq!(variant, canonical);
            }
        }
    }

    #[test]
    fn drop_and_unknown_effects_are_distinguished_from_resource_rewards() {
        let tables = tables();
        assert_eq!(apply_reward(tables, 1110207, initial()), Ok(None));
        assert_eq!(
            apply_reward(tables, 0, initial()),
            Err(RewardError::UnknownEntrust)
        );
        assert_eq!(
            apply_reward(tables, 1110231, initial()),
            Err(RewardError::UnsupportedEffect {
                effect_id: 1110231,
                effect_type: 30
            })
        );
    }

    #[test]
    fn resource_deltas_remain_valid_when_rewards_saturate() {
        let tables = tables();
        let reward = apply_reward(
            tables,
            1110201,
            Resources {
                coin: i32::MAX - 1,
                ..initial()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(reward.resources.coin, i32::MAX);
        assert_eq!(reward.coin_delta, 1);
        assert_eq!(
            apply_reward(
                tables,
                1110201,
                Resources {
                    power: 41,
                    ..initial()
                }
            ),
            Err(RewardError::InvalidResources)
        );
    }
}
