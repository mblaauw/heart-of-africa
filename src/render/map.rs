//! Biome-map and player rendering with placeholder-art fallback (DESIGN.md §5).
//!
//! When the `tile.biome` atlas exists in the manifest, the map is drawn
//! per-tile from it (one row per biome, two seeded variants, culled to the
//! visible window). Without it, a single flat map texture is used — the
//! "always runs" guarantee. Settlements, the tomb, and the player use
//! manifest-addressed art when present, else placeholder shapes.

use macroquad::prelude::*;
use macroquad::texture::Texture2D;

use crate::content::load::AssetManifest;
use crate::world::World;

/// Default on-screen size of one map tile, in pixels.
pub const TILE_SIZE: f32 = 32.0;

/// Atlas layout: one tile per variant column and biome row (32×32), two
/// variants — `hoatool gen-art` writes exactly this shape (art::TILE/VARIANTS).
const ATLAS_TILE: u32 = 32;
const ATLAS_VARIANTS: u32 = 2;

/// Placeholder marker colors (used until real art exists).
const SETTLEMENT_MARKER: Color = Color::new(1.0, 1.0, 1.0, 0.85);
const PLAYER_MARKER: Color = Color::new(1.0, 0.84, 0.2, 1.0);
const TOMB_MARKER: Color = Color::new(0.85, 0.25, 0.2, 1.0);

/// Renders the biome map and markers for one world.
pub struct MapRenderer {
    /// Flat fallback texture of the whole map (used without the atlas).
    map_tex: Option<Texture2D>,
    /// Per-biome tile atlas (rows = biome index, cols = variant).
    atlas: Option<Texture2D>,
    player_tex: Option<Texture2D>,
    settlement_tex: Option<Texture2D>,
    tomb_tex: Option<Texture2D>,
}

impl MapRenderer {
    /// Build the map textures and resolve marker art from the manifest.
    /// Missing art silently yields `None` → placeholder shapes.
    pub fn new(world: &World, assets: Option<&AssetManifest>) -> MapRenderer {
        let atlas = marker_texture(assets, "tile.biome");
        let map_tex = if atlas.is_some() {
            None
        } else {
            // Flat fallback: one texture per pixel, nearest-neighbor scaled.
            let mut rgba = Vec::with_capacity(world.width as usize * world.height as usize * 4);
            for y in 0..world.height {
                for x in 0..world.width {
                    let (r, g, b) = world
                        .grid
                        .biome_at(x, y)
                        .map(|b| b.color)
                        .unwrap_or((0, 0, 0));
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
            Some(Texture2D::from_rgba8(
                world.width as u16,
                world.height as u16,
                &rgba,
            ))
        };
        let player_tex = marker_texture(assets, "marker.player");
        let settlement_tex = marker_texture(assets, "marker.settlement");
        let tomb_tex = marker_texture(assets, "marker.tomb");
        MapRenderer {
            map_tex,
            atlas,
            player_tex,
            settlement_tex,
            tomb_tex,
        }
    }

    /// Draw the map, centered on the player, plus settlement, tomb, and
    /// player markers. `screen` is the current window size in pixels; `tomb`
    /// is the revealed tomb tile, if known.
    pub fn draw(
        &self,
        world: &World,
        player_pos: (u32, u32),
        screen: (f32, f32),
        tile_size: f32,
        tomb: Option<(u32, u32)>,
    ) {
        let cam = Camera::new(player_pos, screen, tile_size);

        match (&self.atlas, &self.map_tex) {
            (Some(atlas), _) => self.draw_tiles(world, &cam, atlas, screen),
            (None, Some(flat)) => {
                draw_texture_ex(
                    flat,
                    cam.x,
                    cam.y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(
                            world.width as f32 * tile_size,
                            world.height as f32 * tile_size,
                        )),
                        ..Default::default()
                    },
                );
            }
            (None, None) => {}
        }

        for s in &world.settlements {
            self.draw_marker(&cam, s.x, s.y, self.settlement_tex.as_ref(), "S");
        }
        if let Some((tx, ty)) = tomb {
            self.draw_marker(&cam, tx, ty, self.tomb_tex.as_ref(), "T");
        }
        self.draw_marker(
            &cam,
            player_pos.0,
            player_pos.1,
            self.player_tex.as_ref(),
            "P",
        );
    }

    /// Draw the visible tiles from the atlas, culled to the window.
    fn draw_tiles(&self, world: &World, cam: &Camera, atlas: &Texture2D, screen: (f32, f32)) {
        let ts = cam.tile_size;
        let left = (-cam.x / ts).floor().max(0.0) as u32;
        let top = (-cam.y / ts).floor().max(0.0) as u32;
        let right = (((screen.0 - cam.x) / ts).ceil() as u32).min(world.width);
        let bottom = (((screen.1 - cam.y) / ts).ceil() as u32).min(world.height);

        for y in top..bottom {
            for x in left..right {
                let Some(row) = world.grid.biome_index_at(x, y) else {
                    continue;
                };
                let row = row as u32;
                let variant = tile_variant(x, y, row) % ATLAS_VARIANTS;
                let source = Rect::new(
                    (variant * ATLAS_TILE) as f32,
                    (row * ATLAS_TILE) as f32,
                    ATLAS_TILE as f32,
                    ATLAS_TILE as f32,
                );
                draw_texture_ex(
                    atlas,
                    cam.x + x as f32 * ts,
                    cam.y + y as f32 * ts,
                    WHITE,
                    DrawTextureParams {
                        source: Some(source),
                        dest_size: Some(vec2(ts, ts)),
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// Draw one marker: the manifest texture if available, else a tinted
    /// rect + label (placeholder fallback, DESIGN.md §5).
    fn draw_marker(&self, cam: &Camera, x: u32, y: u32, tex: Option<&Texture2D>, label: &str) {
        let (px, py) = cam.tile_origin(x, y);
        let tile_size = cam.tile_size;
        match tex {
            Some(tex) => {
                draw_texture_ex(
                    tex,
                    px,
                    py,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(tile_size, tile_size)),
                        ..Default::default()
                    },
                );
            }
            None => {
                let inset = tile_size * 0.15;
                let color = match label {
                    "P" => PLAYER_MARKER,
                    "T" => TOMB_MARKER,
                    _ => SETTLEMENT_MARKER,
                };
                draw_rectangle(
                    px + inset,
                    py + inset,
                    tile_size - inset * 2.0,
                    tile_size - inset * 2.0,
                    color,
                );
                draw_text(
                    label,
                    px + tile_size * 0.38,
                    py + tile_size * 0.68,
                    tile_size * 0.6,
                    BLACK,
                );
            }
        }
    }
}

/// Deterministic tile variant: a pure function of (x, y, row) so the map is
/// stable across frames, saves, and replays.
fn tile_variant(x: u32, y: u32, row: u32) -> u32 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in [x as u64, y as u64, row as u64]
        .iter()
        .flat_map(|v| v.to_le_bytes())
    {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash % ATLAS_VARIANTS as u64) as u32
}

/// Load a manifest-addressed texture; `None` when the ID is not in the
/// manifest or the file is missing/unreadable (placeholder fallback).
fn marker_texture(assets: Option<&AssetManifest>, id: &str) -> Option<Texture2D> {
    let path = assets?.path(id)?;
    let img = image::open(path).ok()?.to_rgba8();
    Some(Texture2D::from_rgba8(
        img.width() as u16,
        img.height() as u16,
        img.as_raw(),
    ))
}

/// Screen-space camera: maps tile coordinates to pixel coordinates.
struct Camera {
    /// Screen offset of tile (0, 0).
    x: f32,
    y: f32,
    tile_size: f32,
}

impl Camera {
    /// A camera centered on `center` (tile coords) in a `screen`-sized window.
    fn new(center: (u32, u32), screen: (f32, f32), tile_size: f32) -> Camera {
        Camera {
            x: screen.0 / 2.0 - (center.0 as f32 + 0.5) * tile_size,
            y: screen.1 / 2.0 - (center.1 as f32 + 0.5) * tile_size,
            tile_size,
        }
    }

    /// Pixel origin (top-left) of a tile.
    fn tile_origin(&self, x: u32, y: u32) -> (f32, f32) {
        (
            self.x + x as f32 * self.tile_size,
            self.y + y as f32 * self.tile_size,
        )
    }
}
