//! Biome-map and player rendering with placeholder-art fallback (DESIGN.md §5).
//!
//! The biome map is drawn from a single texture built once at startup;
//! settlements and the player use manifest-addressed art when present, else a
//! tinted shape + label (the placeholder fallback that keeps the game
//! runnable regardless of art coverage).

use macroquad::prelude::*;
use macroquad::texture::Texture2D;

use crate::content::load::AssetManifest;
use crate::world::World;

/// Default on-screen size of one map tile, in pixels.
pub const TILE_SIZE: f32 = 32.0;

/// Placeholder marker colors (used until real art exists).
const SETTLEMENT_MARKER: Color = Color::new(1.0, 1.0, 1.0, 0.85);
const PLAYER_MARKER: Color = Color::new(1.0, 0.84, 0.2, 1.0);

/// Renders the biome map and markers for one world.
pub struct MapRenderer {
    map_tex: Texture2D,
    player_tex: Option<Texture2D>,
    settlement_tex: Option<Texture2D>,
}

impl MapRenderer {
    /// Build the map texture and resolve marker art from the manifest.
    /// Missing art silently yields `None` → placeholder shapes.
    pub fn new(world: &World, assets: Option<&AssetManifest>) -> MapRenderer {
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
        let map_tex = Texture2D::from_rgba8(world.width as u16, world.height as u16, &rgba);
        let player_tex = marker_texture(assets, "marker.player");
        let settlement_tex = marker_texture(assets, "marker.settlement");
        MapRenderer {
            map_tex,
            player_tex,
            settlement_tex,
        }
    }

    /// Draw the map, centered on the player, plus settlement and player
    /// markers. `screen` is the current window size in pixels.
    pub fn draw(&self, world: &World, player_pos: (u32, u32), screen: (f32, f32), tile_size: f32) {
        let cam = Camera::new(player_pos, screen, tile_size);

        draw_texture_ex(
            &self.map_tex,
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

        for s in &world.settlements {
            self.draw_marker(&cam, s.x, s.y, self.settlement_tex.as_ref(), "S");
        }
        self.draw_marker(
            &cam,
            player_pos.0,
            player_pos.1,
            self.player_tex.as_ref(),
            "P",
        );
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
                let color = if label == "P" {
                    PLAYER_MARKER
                } else {
                    SETTLEMENT_MARKER
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

/// Load a manifest-addressed marker texture; `None` when the ID is not in the
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
