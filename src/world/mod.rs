//! World data: biome grid, pathfinding (DESIGN.md §8.2, §8.3).
//!
//! Invariant: this module must never import `macroquad` or `egui`.

pub mod biome;
pub mod pathfind;

use serde::{Deserialize, Serialize};

/// A named settlement: a tile position plus its 1890s polity (DESIGN.md §3.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settlement {
    pub id: String,
    pub name: String,
    pub polity: String,
    pub x: u32,
    pub y: u32,
}

/// A named river: a polyline of tile coordinates.
///
/// Rivers are metadata for travel/economy in later phases; the navigable
/// water itself is the biome grid. Content lint requires every river point
/// to sit on a water tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct River {
    pub id: String,
    pub name: String,
    pub points: Vec<(u32, u32)>,
}

/// The loaded macro-geography of one region (DESIGN.md §3.1): the biome grid
/// plus named settlements and rivers.
#[derive(Debug, Clone)]
pub struct World {
    pub width: u32,
    pub height: u32,
    pub grid: biome::BiomeGrid,
    pub settlements: Vec<Settlement>,
    pub rivers: Vec<River>,
    /// ID of the settlement where the expedition begins.
    pub start_settlement: String,
}

impl World {
    /// The start settlement, if it resolves.
    pub fn start(&self) -> Option<&Settlement> {
        self.settlements
            .iter()
            .find(|s| s.id == self.start_settlement)
    }

    /// The settlement with the given ID, if it exists.
    pub fn settlement(&self, id: &str) -> Option<&Settlement> {
        self.settlements.iter().find(|s| s.id == id)
    }
}
