//! Biome grid from an indexed-color PNG + TOML color legend (DESIGN.md §3.1).
//!
//! One palette color = one biome with a movement cost and passability. Any
//! pixel color not present in the legend is a load error, so a typo or a
//! stray color fails content lint — the intended guardrail (§9).

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};

/// A biome definition: passability and per-tile movement cost.
///
/// Invariant: `movement_cost` is `0.0` exactly when `passable` is `false`
/// (water). Passable biomes always cost at least `1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeDef {
    pub id: String,
    pub passable: bool,
    /// Cost of entering one tile of this biome; 1.0 = desert baseline.
    pub movement_cost: f32,
}

/// The tile grid of a region: per-tile indices into a small biome table.
#[derive(Debug, Clone)]
pub struct BiomeGrid {
    width: u32,
    height: u32,
    tiles: Vec<u16>,
    biomes: Vec<BiomeDef>,
}

impl BiomeGrid {
    /// Build a grid from RGBA8 pixels (row-major, `width * height * 4` bytes).
    ///
    /// `legend` maps normalized hex colors (`"3a6ea5"`) to biome IDs; every
    /// pixel color must be in the legend, and every legend entry must name a
    /// defined biome. Both are content-lint errors when violated.
    pub fn from_pixels(
        rgba: &[u8],
        width: u32,
        height: u32,
        legend: &BTreeMap<String, String>,
        biome_defs: &BTreeMap<String, BiomeDef>,
    ) -> Result<Self> {
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            bail!(
                "pixel buffer length {expected} expected, got {}",
                rgba.len()
            );
        }

        let mut index_of: BTreeMap<String, u16> = BTreeMap::new();
        let mut biomes: Vec<BiomeDef> = Vec::new();
        for (id, def) in biome_defs {
            if index_of.contains_key(id) {
                continue;
            }
            index_of.insert(id.clone(), biomes.len() as u16);
            biomes.push(def.clone());
        }

        let mut color_to_biome: BTreeMap<(u8, u8, u8), u16> = BTreeMap::new();
        for (hex, id) in legend {
            let rgb = parse_hex_color(hex).with_context(|| format!("bad legend color {hex:?}"))?;
            let idx = *index_of
                .get(id)
                .ok_or_else(|| anyhow!("legend names unknown biome {id:?}"))?;
            color_to_biome.insert(rgb, idx);
        }

        let mut tiles = Vec::with_capacity(width as usize * height as usize);
        for (i, px) in rgba.chunks_exact(4).enumerate() {
            let rgb = (px[0], px[1], px[2]);
            let idx = *color_to_biome.get(&rgb).ok_or_else(|| {
                let x = (i as u32) % width;
                let y = (i as u32) / width;
                anyhow!(
                    "pixel ({x},{y}) has color #{:02x}{:02x}{:02x} which is not in the biome legend",
                    px[0], px[1], px[2]
                )
            })?;
            tiles.push(idx);
        }

        Ok(Self {
            width,
            height,
            tiles,
            biomes,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn in_bounds(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// The biome definition at a tile, if in bounds.
    pub fn biome_at(&self, x: u32, y: u32) -> Option<&BiomeDef> {
        if !self.in_bounds(x, y) {
            return None;
        }
        let idx = self.tiles[(y * self.width + x) as usize] as usize;
        Some(&self.biomes[idx])
    }

    /// Biome ID at a tile ("" when out of bounds).
    pub fn biome_id_at(&self, x: u32, y: u32) -> &str {
        self.biome_at(x, y).map(|b| b.id.as_str()).unwrap_or("")
    }

    /// True when a tile is in bounds and its biome is passable.
    pub fn passable_at(&self, x: u32, y: u32) -> bool {
        self.biome_at(x, y).is_some_and(|b| b.passable)
    }

    /// Movement cost of a tile; 0.0 out of bounds or impassable.
    pub fn movement_cost_at(&self, x: u32, y: u32) -> f32 {
        match self.biome_at(x, y) {
            Some(b) if b.passable => b.movement_cost,
            _ => 0.0,
        }
    }
}

/// Parse `"3a6ea5"` or `"#3A6EA5"` into `(r, g, b)`.
fn parse_hex_color(hex: &str) -> Result<(u8, u8, u8)> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        bail!("color must be 6 hex digits, got {hex:?}");
    }
    let Ok(r) = u8::from_str_radix(&hex[0..2], 16) else {
        bail!("color {hex:?} has a non-hex digit");
    };
    let Ok(g) = u8::from_str_radix(&hex[2..4], 16) else {
        bail!("color {hex:?} has a non-hex digit");
    };
    let Ok(b) = u8::from_str_radix(&hex[4..6], 16) else {
        bail!("color {hex:?} has a non-hex digit");
    };
    Ok((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> BTreeMap<String, BiomeDef> {
        BTreeMap::from([
            (
                "water".to_string(),
                BiomeDef {
                    id: "water".into(),
                    passable: false,
                    movement_cost: 0.0,
                },
            ),
            (
                "desert".to_string(),
                BiomeDef {
                    id: "desert".into(),
                    passable: true,
                    movement_cost: 1.0,
                },
            ),
            (
                "floodplain".to_string(),
                BiomeDef {
                    id: "floodplain".into(),
                    passable: true,
                    movement_cost: 1.5,
                },
            ),
        ])
    }

    fn legend() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("3a6ea5".to_string(), "water".to_string()),
            ("d9b36c".to_string(), "desert".to_string()),
            ("8fae5c".to_string(), "floodplain".to_string()),
        ])
    }

    fn pixel(rgb: (u8, u8, u8)) -> [u8; 4] {
        [rgb.0, rgb.1, rgb.2, 255]
    }

    #[test]
    fn builds_grid_from_pixels() {
        // 2x2: desert | water
        //      floodplain | desert
        let mut px = Vec::new();
        px.extend_from_slice(&pixel((0xd9, 0xb3, 0x6c)));
        px.extend_from_slice(&pixel((0x3a, 0x6e, 0xa5)));
        px.extend_from_slice(&pixel((0x8f, 0xae, 0x5c)));
        px.extend_from_slice(&pixel((0xd9, 0xb3, 0x6c)));
        let grid = BiomeGrid::from_pixels(&px, 2, 2, &legend(), &defs()).unwrap();

        assert_eq!(grid.width(), 2);
        assert_eq!(grid.height(), 2);
        assert_eq!(grid.biome_id_at(0, 0), "desert");
        assert_eq!(grid.biome_id_at(1, 0), "water");
        assert_eq!(grid.biome_id_at(0, 1), "floodplain");
        assert!(grid.passable_at(0, 0));
        assert!(!grid.passable_at(1, 0));
        assert_eq!(grid.movement_cost_at(0, 1), 1.5);
        assert_eq!(grid.movement_cost_at(1, 0), 0.0);
        assert!(!grid.in_bounds(2, 0));
        assert!(!grid.passable_at(2, 0));
    }

    #[test]
    fn unknown_pixel_color_is_a_lint_error() {
        let px = pixel((0x12, 0x34, 0x56));
        let err = BiomeGrid::from_pixels(&px, 1, 1, &legend(), &defs()).unwrap_err();
        assert!(err.to_string().contains("(0,0)"), "{err}");
        assert!(err.to_string().contains("not in the biome legend"), "{err}");
    }

    #[test]
    fn legend_entry_for_undefined_biome_is_a_lint_error() {
        let px = pixel((0xd9, 0xb3, 0x6c));
        let mut legend = legend();
        legend.insert("123456".to_string(), "nonexistent".to_string());
        let err = BiomeGrid::from_pixels(&px, 1, 1, &legend, &defs()).unwrap_err();
        assert!(err.to_string().contains("unknown biome"), "{err}");
    }

    #[test]
    fn hex_colors_parse_with_or_without_hash() {
        assert_eq!(parse_hex_color("#3A6EA5").unwrap(), (0x3a, 0x6e, 0xa5));
        assert_eq!(parse_hex_color("3a6ea5").unwrap(), (0x3a, 0x6e, 0xa5));
        assert!(parse_hex_color("3a6ea").is_err());
        assert!(parse_hex_color("gggggg").is_err());
    }
}
