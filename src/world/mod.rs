//! World data: biome grid, pathfinding, geometry (DESIGN.md §3, §8.2, §8.3).
//!
//! Invariant: this module must never import `macroquad` or `egui`.

pub mod biome;
pub mod coords;
pub mod geo;
pub mod pathfind;

pub use coords::KM_PER_CELL;

use std::collections::BTreeMap;

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

/// A named river: a polyline of tile coordinates plus committed km geometry.
///
/// Rivers are metadata for travel/economy in later phases; the navigable
/// water itself is the biome grid. Content lint requires every river point
/// to sit on a water tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct River {
    pub id: String,
    pub name: String,
    pub points: Vec<(u32, u32)>,
    /// Committed polyline length in kilometres (precomputed at import time).
    pub length_km: f64,
    /// Width class authored in Tiled (`narrow`, `normal`, `wide`).
    pub width_class: String,
    /// Whether the river is navigable by vessel.
    pub navigable: bool,
    /// Seasonal rules: `None` = always; `Some("dry")` = impassable in dry season.
    pub seasonal: Option<String>,
}

/// A caravan route between two settlements (DESIGN.md §4.2): fast travel
/// once both endpoints are scouted. Risky routes roll a telegraphed
/// encounter; safe scouted routes are genuinely safe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub from: String,
    pub to: String,
    pub risky: bool,
    /// Committed polyline length in kilometres (precomputed at import time).
    pub path_km: f64,
    /// Dense sim-tile path along the authored polyline (for hour-by-hour
    /// sim: visited marking, weather, encounter position).
    pub tiles: Vec<(u32, u32)>,
    /// Travel modes permitted on this route (e.g. `caravan`, `foot`).
    pub modes: Vec<String>,
    /// Whether the route is seasonal (impassable in some seasons).
    pub seasonal: bool,
    /// Whether the route is blocked (e.g. collapsed bridge) — impassable.
    pub blocked: bool,
}

/// A named region polygon (floodplain, danger zone, region boundary).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub polygon: geo::KmPolygon,
}

/// A named landmark point (used by clue SpatialHints in WS2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landmark {
    pub id: String,
    pub kind: String,
    pub point: geo::KmPoint,
}

/// Regional vocabulary and social knowledge (EXPANSION_PLAN §WS2): the
/// direction and distance terms a region's people use, preferred and
/// disliked gifts, terrain lore, landmark terms, and etiquette facts.
/// Loaded from `content/regions/*.toml` — one file per region.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegionKnowledge {
    pub direction_terms: Vec<String>,
    pub distance_terms: Vec<String>,
    pub preferred_gifts: Vec<String>,
    pub disliked_gifts: Vec<String>,
    pub terrain_lore: Vec<String>,
    pub landmark_terms: Vec<String>,
    pub etiquette: Vec<String>,
}

/// The loaded macro-geography of one region (DESIGN.md §3.1): the biome grid
/// plus named settlements, rivers, routes, regions, and landmarks.
#[derive(Debug, Clone)]
pub struct World {
    pub width: u32,
    pub height: u32,
    pub grid: biome::BiomeGrid,
    pub settlements: Vec<Settlement>,
    pub rivers: Vec<River>,
    pub routes: Vec<Route>,
    pub regions: Vec<Region>,
    pub landmarks: Vec<Landmark>,
    /// Regional vocabulary and social knowledge, keyed by region id.
    pub knowledges: BTreeMap<String, RegionKnowledge>,
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
