//! Content loaders: world.toml and the biome map (DESIGN.md §7, §8.2).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::world::biome::{BiomeDef, BiomeGrid};
use crate::world::{River, Settlement, World};

/// Name of the world data file, at the content directory root.
pub const WORLD_FILE: &str = "world.toml";

/// Current world data format version; `world.toml` must declare it.
pub const WORLD_VERSION: u32 = 1;

#[derive(serde::Deserialize)]
struct WorldToml {
    version: u32,
    map: MapToml,
    biome: BTreeMap<String, BiomeToml>,
    region: RegionToml,
    #[serde(default)]
    settlements: Vec<SettlementToml>,
    #[serde(default)]
    rivers: Vec<RiverToml>,
}

#[derive(serde::Deserialize)]
struct MapToml {
    /// Path to the biome PNG, relative to the content directory.
    png: String,
    /// Normalized hex color (`"3a6ea5"`) -> biome ID.
    legend: BTreeMap<String, String>,
}

#[derive(serde::Deserialize)]
struct BiomeToml {
    passable: bool,
    movement_cost: f32,
}

#[derive(serde::Deserialize)]
struct RegionToml {
    /// Human-readable metadata for the region; parsed, not used in Phase 1.
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    start_settlement: String,
}

#[derive(serde::Deserialize)]
struct SettlementToml {
    id: String,
    name: String,
    polity: String,
    x: u32,
    y: u32,
}

#[derive(serde::Deserialize)]
struct RiverToml {
    id: String,
    name: String,
    points: Vec<(u32, u32)>,
}

/// Load and parse the full world (world.toml + biome PNG) under
/// `content_dir`. Schema errors and unresolvable references fail loudly —
/// content lint is `cargo test` failing on a typo (§9).
pub fn load_world(content_dir: &Path) -> Result<World> {
    let toml_text = std::fs::read_to_string(content_dir.join(WORLD_FILE))
        .with_context(|| format!("reading {}", content_dir.join(WORLD_FILE).display()))?;
    let raw: WorldToml =
        toml::from_str(&toml_text).with_context(|| format!("parsing {}", WORLD_FILE))?;
    if raw.version != WORLD_VERSION {
        anyhow::bail!(
            "{} declares version {}, expected {}",
            WORLD_FILE,
            raw.version,
            WORLD_VERSION
        );
    }

    let biome_defs: BTreeMap<String, BiomeDef> = raw
        .biome
        .iter()
        .map(|(id, b)| {
            let def = BiomeDef {
                id: id.clone(),
                passable: b.passable,
                movement_cost: b.movement_cost,
            };
            validate_biome(&def)?;
            Ok((id.clone(), def))
        })
        .collect::<Result<_>>()?;

    let png_path = content_dir.join(&raw.map.png);
    let img = image::open(&png_path)
        .with_context(|| format!("opening biome map {}", png_path.display()))?
        .to_rgba8();
    let grid = BiomeGrid::from_pixels(
        img.as_raw(),
        img.width(),
        img.height(),
        &raw.map.legend,
        &biome_defs,
    )?;

    let settlements: Vec<Settlement> = raw
        .settlements
        .iter()
        .map(|s| Settlement {
            id: s.id.clone(),
            name: s.name.clone(),
            polity: s.polity.clone(),
            x: s.x,
            y: s.y,
        })
        .collect();

    let rivers: Vec<River> = raw
        .rivers
        .iter()
        .map(|r| River {
            id: r.id.clone(),
            name: r.name.clone(),
            points: r.points.clone(),
        })
        .collect();

    Ok(World {
        width: img.width(),
        height: img.height(),
        grid,
        settlements,
        rivers,
        start_settlement: raw.region.start_settlement,
    })
}

/// Biome definitions must satisfy: passable biomes cost >= 1.0, impassable
/// biomes cost 0.0. Violations are content-lint errors.
fn validate_biome(def: &BiomeDef) -> Result<()> {
    if def.passable && def.movement_cost < 1.0 {
        anyhow::bail!(
            "biome {:?}: passable biomes must cost at least 1.0, got {}",
            def.id,
            def.movement_cost
        );
    }
    if !def.passable && def.movement_cost != 0.0 {
        anyhow::bail!(
            "biome {:?}: impassable biomes must cost 0.0, got {}",
            def.id,
            def.movement_cost
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_costs_are_enforced() {
        let ok = BiomeDef {
            id: "desert".into(),
            passable: true,
            movement_cost: 1.0,
        };
        assert!(validate_biome(&ok).is_ok());

        let bad = BiomeDef {
            id: "water".into(),
            passable: false,
            movement_cost: 5.0,
        };
        assert!(validate_biome(&bad).is_err());

        let bad = BiomeDef {
            id: "desert".into(),
            passable: true,
            movement_cost: 0.5,
        };
        assert!(validate_biome(&bad).is_err());
    }
}
