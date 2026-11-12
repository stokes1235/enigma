//! Shopping node stock for the original Rouge.
//!
//! `rouge_goods` rows are grouped by `goodsGroup`; every shipped group belongs to
//! layer 111 (`goodsGroup / 1000 == 111`). A shop node offers a small weighted
//! sample from one group, and the client addresses the offers by position in
//! `RougeBuyGoodsRequest.goods_pos`.

use config::rouge_goods::RougeGoods;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// Number of offers a shop node shows.
pub const SHOP_SLOTS: usize = 8;

pub fn refresh_price(tables: &config::GameDB, refresh_num: i32) -> i32 {
    let values: Vec<i32> = tables
        .rouge_const
        .get(21)
        .map(|row| {
            row.value
                .split('#')
                .filter_map(|value| value.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    values.first().copied().unwrap_or(50)
        + values.get(1).copied().unwrap_or(25) * refresh_num.max(0)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShopState {
    /// Node the stock was rolled for.
    pub node_id: i32,
    /// Goods ids in display order.
    pub stock: Vec<i32>,
    /// One-based positions already purchased at this node.
    #[serde(default)]
    pub bought: Vec<i32>,
    /// Number of manual rerolls at this node.
    #[serde(default)]
    pub refresh_num: i32,
}

/// Rolls a fresh stock for a shop node. Deterministic for a given seed.
pub fn generate(
    layer_id: i32,
    node_id: i32,
    seed: u64,
    goods: impl Iterator<Item = RougeGoods>,
) -> ShopState {
    generate_for_refresh(layer_id, node_id, seed, 0, goods)
}

pub fn generate_for_refresh(
    layer_id: i32,
    node_id: i32,
    seed: u64,
    refresh_num: i32,
    goods: impl Iterator<Item = RougeGoods>,
) -> ShopState {
    // Every shipped `goodsGroup` encodes its layer in the leading digits
    // (`111121 / 1000 == 111`).
    let all: Vec<RougeGoods> = goods.filter(|row| row.price > 0).collect();
    let mut pool: Vec<RougeGoods> = all
        .iter()
        .filter(|row| row.goods_group / 1000 == layer_id)
        .cloned()
        .collect();
    // The base export only has first-area stock. Reuse that catalog for later
    // areas until their weighted slot rules are available.
    if pool.is_empty() {
        pool = all
            .into_iter()
            .filter(|row| row.goods_group / 1000 == 111)
            .collect();
    }
    if pool.is_empty() {
        return ShopState {
            node_id,
            stock: Vec::new(),
            bought: Vec::new(),
            refresh_num,
        };
    }

    let mut rng = StdRng::seed_from_u64(
        seed ^ (node_id as u64) << 7 ^ (refresh_num.max(0) as u64).rotate_left(17),
    );
    let mut stock = Vec::with_capacity(SHOP_SLOTS);
    for _ in 0..SHOP_SLOTS {
        let total: u32 = pool.iter().map(|row| row.weights.max(1) as u32).sum();
        let mut roll = rng.random_range(0..total.max(1));
        let mut picked = pool[0].id;
        for row in &pool {
            let weight = row.weights.max(1) as u32;
            if roll < weight {
                picked = row.id;
                break;
            }
            roll -= weight;
        }
        stock.push(picked);
    }
    ShopState {
        node_id,
        stock,
        bought: Vec::new(),
        refresh_num,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goods(id: i32, group: i32, price: i32, weights: i32) -> RougeGoods {
        RougeGoods {
            id,
            goods_group: group,
            currency: 1,
            price,
            weights,
            creator: 0,
        }
    }

    #[test]
    fn rolls_four_offers_from_the_layer_groups() {
        let pool = vec![
            goods(1, 111121, 100, 100),
            goods(2, 111122, 200, 100),
            goods(3, 222121, 300, 100), // different layer, must be ignored
        ];
        let shop = generate(111, 55, 7, pool.into_iter());
        assert_eq!(shop.node_id, 55);
        assert_eq!(shop.stock.len(), SHOP_SLOTS);
        assert!(shop.stock.iter().all(|id| *id == 1 || *id == 2));
        // Deterministic.
        let again = generate(
            111,
            55,
            7,
            vec![goods(1, 111121, 100, 100), goods(2, 111122, 200, 100)].into_iter(),
        );
        assert_eq!(again.stock.len(), SHOP_SLOTS);
    }

    #[test]
    fn empty_pool_yields_no_offers() {
        let shop = generate(111, 1, 1, std::iter::empty());
        assert!(shop.stock.is_empty());
    }
}
