//! Trade & economy (DESIGN.md §4.4).
//!
//! Each settlement is a small independent market: buy price = base × season ×
//! local demand × reputation modifier; sell price = base × season × local
//! supply × resale penalty × reputation modifier. No global market sim.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::survival::Ailment;

/// Goods categories (DESIGN.md §4.4): supplies, gifts, expedition gear,
/// and trade goods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Supply,
    Gift,
    Gear,
    Trade,
}

/// One tradable good. Effects are optional: supplies restore stats, quinine
/// cures malaria, gifts carry reputation value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goods {
    pub id: String,
    pub name: String,
    pub category: Category,
    /// Base price in piastres.
    pub base_price: u32,
    /// Reputation gained when gifting one unit (0 for non-gifts).
    #[serde(default)]
    pub gift_value: i32,
    #[serde(default)]
    pub restores_hydration: f32,
    #[serde(default)]
    pub restores_nutrition: f32,
    #[serde(default)]
    pub cures: Option<Ailment>,
}

/// Supply/demand factors of one good at one settlement. Absent entries use 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarketEntry {
    pub demand: f32,
    pub supply: f32,
}

/// The market of one settlement (DESIGN.md §4.4: "a small independent state
/// struct updated on its own tick" — drift arrives in Phase 3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Market {
    pub settlement_id: String,
    pub items: BTreeMap<String, MarketEntry>,
}

/// Resale penalty applied to sell prices.
const SELL_PENALTY: f32 = 0.6;

impl Market {
    /// True when the market trades this good.
    pub fn trades(&self, goods_id: &str) -> bool {
        self.items.contains_key(goods_id)
    }

    /// Buy price in piastres: base × season × demand × reputation modifier.
    pub fn buy_price(&self, goods: &Goods, season: f32, reputation: i32) -> u32 {
        let rep_mod = 1.0 - (reputation.max(0) as f32) / 200.0;
        (goods.base_price as f32 * season * self.factor(&goods.id).demand * rep_mod).round() as u32
    }

    /// Sell price in piastres: base × season × supply × penalty × rep modifier.
    pub fn sell_price(&self, goods: &Goods, season: f32, reputation: i32) -> u32 {
        let rep_mod = 1.0 - (reputation.max(0) as f32) / 200.0;
        (goods.base_price as f32 * season * self.factor(&goods.id).supply * SELL_PENALTY * rep_mod)
            .round() as u32
    }

    fn factor(&self, goods_id: &str) -> MarketEntry {
        self.items.get(goods_id).copied().unwrap_or(MarketEntry {
            demand: 1.0,
            supply: 1.0,
        })
    }
}

/// Seasonal price factor by month, 1–12 (flood season makes goods dearer;
/// DESIGN.md §3.3 weather/seasons feed the market).
pub fn season_factor(month: u32) -> f32 {
    match month {
        1..=5 => 1.0,
        6..=9 => 1.2, // Nile flood season: supplies scarcer, prices up
        10..=12 => 1.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goods(id: &str, base: u32) -> Goods {
        Goods {
            id: id.to_string(),
            name: id.to_string(),
            category: Category::Trade,
            base_price: base,
            gift_value: 0,
            restores_hydration: 0.0,
            restores_nutrition: 0.0,
            cures: None,
        }
    }

    fn market() -> Market {
        Market {
            settlement_id: "cairo".into(),
            items: BTreeMap::from([(
                "dates".into(),
                MarketEntry {
                    demand: 0.6,
                    supply: 1.5,
                },
            )]),
        }
    }

    #[test]
    fn buy_price_scales_with_demand_and_season() {
        let m = market();
        let dates = goods("dates", 15);
        // base 15 × demand 0.6 = 9 (dry season), × 1.2 = 11 (flood season).
        assert_eq!(m.buy_price(&dates, 1.0, 0), 9);
        assert_eq!(m.buy_price(&dates, 1.2, 0), 11);
    }

    #[test]
    fn sell_price_has_resale_penalty() {
        let m = market();
        let dates = goods("dates", 15);
        // base 15 × supply 1.5 × 0.6 = 13.5 -> 14.
        assert_eq!(m.sell_price(&dates, 1.0, 0), 14);
    }

    #[test]
    fn reputation_modifier_discounts_prices() {
        let m = market();
        let dates = goods("dates", 15);
        assert!(m.buy_price(&dates, 1.0, 20) < m.buy_price(&dates, 1.0, 0));
    }

    #[test]
    fn absent_items_have_neutral_factors() {
        let m = Market::default();
        let salt = goods("salt", 20);
        assert_eq!(m.buy_price(&salt, 1.0, 0), 20);
        assert!(!m.trades("salt"));
    }

    #[test]
    fn season_factor_by_month() {
        assert_eq!(season_factor(3), 1.0);
        assert_eq!(season_factor(7), 1.2);
        assert_eq!(season_factor(11), 1.0);
        assert_eq!(season_factor(0), 1.0);
        assert_eq!(season_factor(13), 1.0);
    }
}
