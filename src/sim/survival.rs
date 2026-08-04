//! Survival system (DESIGN.md §4.3): hydration, nutrition, rest, and the
//! four period ailments — malaria, dysentery, sunstroke, trypanosomiasis.
//!
//! Everything is deterministic: ailment risk is an integer counter driven by
//! (biome, hour, month) plus item use — no randomness, so the determinism
//! contract (§9) holds. Ailments are telegraphed: stages 1–3 are named,
//! visible in the HUD, and each has a period-appropriate countermeasure.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The four ailments of the expedition (DESIGN.md §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Ailment {
    #[serde(rename = "malaria")]
    Malaria,
    #[serde(rename = "dysentery")]
    Dysentery,
    #[serde(rename = "sunstroke")]
    Sunstroke,
    #[serde(rename = "trypanosomiasis")]
    Trypanosomiasis,
}

impl Ailment {
    /// Display name, as the HUD shows it.
    pub fn label(self) -> &'static str {
        match self {
            Ailment::Malaria => "malaria",
            Ailment::Dysentery => "dysentery",
            Ailment::Sunstroke => "sunstroke",
            Ailment::Trypanosomiasis => "trypanosomiasis",
        }
    }
}

/// Risk counter and visible stage of one ailment.
///
/// Stage 0 = none, 1 = early symptoms (visible), 2 = severe (extra strain),
/// 3 = fatal. Risk rises on exposure and falls on countermeasures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AilmentState {
    pub stage: u8,
    risk: u8,
}

/// Exposure thresholds per ailment (risk -> stage).
const ONSET_RISK: u8 = 6;
const SEVERE_RISK: u8 = 10;
const DEATH_RISK: u8 = 14;

impl AilmentState {
    fn new() -> Self {
        AilmentState { stage: 0, risk: 0 }
    }

    fn expose(&mut self) {
        self.risk = self.risk.saturating_add(1);
        self.refresh();
    }

    fn relieve(&mut self, amount: u8) {
        self.risk = self.risk.saturating_sub(amount);
        self.refresh();
    }

    fn cure(&mut self) {
        self.risk = 0;
        self.stage = 0;
    }

    fn refresh(&mut self) {
        // Stage derives directly from risk: relieved risk lowers the stage.
        self.stage = if self.risk >= DEATH_RISK {
            3
        } else if self.risk >= SEVERE_RISK {
            2
        } else if self.risk >= ONSET_RISK {
            1
        } else {
            0
        };
        debug_assert!(self.stage <= 3);
    }
}

/// Survival stats, all in 0..=100. Reaching 0 in any stat is fatal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Survival {
    pub hydration: f32,
    pub nutrition: f32,
    pub rest: f32,
    ailments: BTreeMap<Ailment, AilmentState>,
}

/// Per-move consumption (1 move = 1 tick = 1 in-game hour).
const MOVE_HYDRATION: f32 = 0.25;
const MOVE_NUTRITION: f32 = 0.15;
const MOVE_REST: f32 = 0.1;

impl Survival {
    /// A fresh expedition: all stats full, no ailments.
    pub fn new() -> Self {
        let ailments = BTreeMap::from([
            (Ailment::Malaria, AilmentState::new()),
            (Ailment::Dysentery, AilmentState::new()),
            (Ailment::Sunstroke, AilmentState::new()),
            (Ailment::Trypanosomiasis, AilmentState::new()),
        ]);
        Survival {
            hydration: 100.0,
            nutrition: 100.0,
            rest: 100.0,
            ailments,
        }
    }

    pub fn stage(&self, ailment: Ailment) -> u8 {
        self.ailments.get(&ailment).map_or(0, |a| a.stage)
    }

    /// Ailments with stage >= 1, in fixed order (for the HUD).
    pub fn visible_ailments(&self) -> Vec<(Ailment, u8)> {
        self.ailments
            .iter()
            .filter(|(_, a)| a.stage >= 1)
            .map(|(k, a)| (*k, a.stage))
            .collect()
    }

    /// One hour of travel (one successful move). `biome` is the tile entered;
    /// `hour` 0–23; `month` 1–12.
    pub fn on_travel(&mut self, biome: &str, hour: u64, month: u32) {
        self.drain(MOVE_HYDRATION, MOVE_NUTRITION, MOVE_REST);

        // Stage effects: an onset ailment taxes travel further.
        let extra = match self.worst_stage() {
            0 => 0.0,
            1 => 0.05,
            _ => 0.1,
        };
        self.hydration = (self.hydration - extra).max(0.0);

        let daylight = (6..18).contains(&hour);
        let flood_season = (6..=9).contains(&month);

        if daylight && (biome == "desert" || biome == "savanna") && self.hydration < 60.0 {
            self.expose(Ailment::Sunstroke); // shade/night travel avoids it
        }
        if flood_season && biome == "floodplain" {
            self.expose(Ailment::Malaria); // wet-season malaria; quinine cures
        }
        if daylight && biome == "savanna" {
            self.expose(Ailment::Trypanosomiasis); // tsetse country; avoidance only
        }
    }

    /// One hour of rest. Resting at night relieves sunstroke; a tent and a
    /// settlement both improve recovery.
    pub fn on_rest(&mut self, hour: u64, at_settlement: bool, has_tent: bool) {
        let mut gain = 12.0;
        if at_settlement {
            gain += 3.0;
        }
        if has_tent {
            gain += 3.0;
        }
        self.rest = (self.rest + gain).min(100.0);
        self.hydration = (self.hydration - 0.05).max(0.0);
        self.nutrition = (self.nutrition - 0.1).max(0.0);

        if (6..18).contains(&hour) {
            self.relieve(Ailment::Sunstroke, 1);
        } else {
            self.relieve(Ailment::Sunstroke, 2);
        }
    }

    /// Drink water. Unboiled water in the floodplain risks dysentery; boiled
    /// water never does (DESIGN.md §4.3).
    pub fn drink(&mut self, boiled: bool, in_floodplain: bool) {
        self.hydration = (self.hydration + 25.0).min(100.0);
        if !boiled && in_floodplain {
            self.expose(Ailment::Dysentery);
        }
        if boiled {
            self.relieve(Ailment::Dysentery, 2);
        }
    }

    pub fn eat(&mut self, amount: f32) {
        self.nutrition = (self.nutrition + amount).min(100.0);
    }

    /// Administer a cure (quinine for malaria).
    pub fn cure(&mut self, ailment: Ailment) {
        self.ailments
            .get_mut(&ailment)
            .expect("all ailments tracked")
            .cure();
    }

    /// The reason of death, if the expedition has died: a stat at zero or a
    /// stage-3 ailment. `None` while alive.
    pub fn cause_of_death(&self) -> Option<String> {
        if self.hydration <= 0.0 {
            return Some("died of thirst".to_string());
        }
        if self.nutrition <= 0.0 {
            return Some("died of hunger".to_string());
        }
        if self.rest <= 0.0 {
            return Some("died of exhaustion".to_string());
        }
        self.ailments
            .iter()
            .find(|(_, a)| a.stage >= 3)
            .map(|(k, _)| format!("died of {}", k.label()))
    }

    fn worst_stage(&self) -> u8 {
        self.ailments.values().map(|a| a.stage).max().unwrap_or(0)
    }

    fn expose(&mut self, ailment: Ailment) {
        self.ailments
            .get_mut(&ailment)
            .expect("all ailments tracked")
            .expose();
    }

    fn relieve(&mut self, ailment: Ailment, amount: u8) {
        self.ailments
            .get_mut(&ailment)
            .expect("all ailments tracked")
            .relieve(amount);
    }

    fn drain(&mut self, hydration: f32, nutrition: f32, rest: f32) {
        self.hydration = (self.hydration - hydration).max(0.0);
        self.nutrition = (self.nutrition - nutrition).max(0.0);
        self.rest = (self.rest - rest).max(0.0);
    }
}

impl Default for Survival {
    fn default() -> Self {
        Survival::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn travel_drains_stats() {
        let mut s = Survival::new();
        s.on_travel("desert", 12, 3);
        assert!(s.hydration < 100.0 && s.nutrition < 100.0 && s.rest < 100.0);
    }

    #[test]
    fn sunstroke_needs_daylight_desert_and_low_hydration() {
        let mut s = Survival::new();
        // Hydrated: no exposure.
        s.on_travel("desert", 12, 3);
        assert_eq!(s.stage(Ailment::Sunstroke), 0);

        // Thirsty in the desert at noon: exposure accumulates.
        let mut s = Survival::new();
        s.hydration = 40.0;
        for _ in 0..ONSET_RISK {
            s.on_travel("desert", 12, 3);
        }
        assert_eq!(s.stage(Ailment::Sunstroke), 1);

        // Night travel in the same desert: no exposure.
        let mut s = Survival::new();
        s.hydration = 40.0;
        for _ in 0..ONSET_RISK * 2 {
            s.on_travel("desert", 23, 3);
        }
        assert_eq!(s.stage(Ailment::Sunstroke), 0);
    }

    #[test]
    fn malaria_only_in_flood_season_floodplain() {
        let mut s = Survival::new();
        for _ in 0..ONSET_RISK * 2 {
            s.on_travel("floodplain", 12, 3); // dry season
        }
        assert_eq!(s.stage(Ailment::Malaria), 0);

        let mut s = Survival::new();
        for _ in 0..ONSET_RISK {
            s.on_travel("floodplain", 12, 8); // flood season
        }
        assert_eq!(s.stage(Ailment::Malaria), 1);
    }

    #[test]
    fn quinine_cures_malaria() {
        let mut s = Survival::new();
        for _ in 0..ONSET_RISK {
            s.on_travel("floodplain", 12, 8);
        }
        assert_eq!(s.stage(Ailment::Malaria), 1);
        s.cure(Ailment::Malaria);
        assert_eq!(s.stage(Ailment::Malaria), 0);
    }

    #[test]
    fn dysentery_from_unboiled_floodplain_water() {
        let mut s = Survival::new();
        for _ in 0..ONSET_RISK {
            s.drink(false, true);
        }
        assert_eq!(s.stage(Ailment::Dysentery), 1);
        // Boiled water relieves it.
        for _ in 0..ONSET_RISK / 2 {
            s.drink(true, true);
        }
        assert_eq!(s.stage(Ailment::Dysentery), 0);
    }

    #[test]
    fn trypanosomiasis_from_daylight_savanna() {
        let mut s = Survival::new();
        for _ in 0..ONSET_RISK {
            s.on_travel("savanna", 12, 3);
        }
        assert_eq!(s.stage(Ailment::Trypanosomiasis), 1);
    }

    #[test]
    fn rest_recovers_and_night_rest_relieves_sunstroke() {
        let mut s = Survival::new();
        s.hydration = 40.0;
        for _ in 0..ONSET_RISK {
            s.on_travel("desert", 12, 3);
        }
        assert_eq!(s.stage(Ailment::Sunstroke), 1);
        for _ in 0..ONSET_RISK / 2 {
            s.on_rest(23, false, false);
        }
        assert_eq!(s.stage(Ailment::Sunstroke), 0);
        assert!(s.rest > 0.0);
    }

    #[test]
    fn death_reasons() {
        let mut s = Survival::new();
        s.hydration = 0.0;
        assert_eq!(s.cause_of_death(), Some("died of thirst".to_string()));
        let mut s = Survival::new();
        s.nutrition = 0.0;
        assert_eq!(s.cause_of_death(), Some("died of hunger".to_string()));
        let mut s = Survival::new();
        for _ in 0..DEATH_RISK {
            s.drink(false, true);
        }
        assert_eq!(s.cause_of_death(), Some("died of dysentery".to_string()));
        let s = Survival::new();
        assert_eq!(s.cause_of_death(), None);
    }
}
