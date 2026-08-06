//! Biome-map and player rendering (DESIGN.md §5, WS1 renderer split).
//!
//! When the `tile.biome` atlas exists in the manifest, the map is drawn
//! per-tile from it (one row per biome, two seeded variants, culled to the
//! visible window). Without it, a single flat map texture is used — the
//! "always runs" guarantee. Settlements, the tomb, and the player use
//! manifest-addressed art when present, else placeholder shapes.
//!
//! Two views share this renderer (WS1):
//! - follow: the gameplay view, centered on the player (unchanged behaviour);
//! - explore: a free map with zoom/pan, fog of war, labels, and routes —
//!   camera state lives in [`MapView`] and is presentation-only.

pub mod camera;
pub mod labels;
pub mod layers;
pub mod overlays;
pub mod tile_cache;

use macroquad::prelude::*;
use macroquad::texture::Texture2D;

use crate::content::load::AssetManifest;
use crate::render::map::camera::{Camera, MapCamera, TileCamera};
use crate::render::map::labels::{LabelItem, PlacedLabel, place_labels};
use crate::render::map::layers::{draw_biome, draw_marker};
use crate::render::map::overlays::{draw_fog, draw_routes, draw_tomb};
use crate::render::map::tile_cache::{Lod, lod_for_tile_size};
use crate::sim::game::Game;
use crate::world::World;

/// Default on-screen size of one map tile, in pixels.
pub const TILE_SIZE: f32 = 32.0;

/// Semantic zoom thresholds for labels (pixels per tile).
const NAME_ZOOM: f32 = 16.0;
const POLITY_ZOOM: f32 = 28.0;

/// Renders the biome map and markers for one world.
pub struct MapRenderer {
    /// Flat fallback texture of the whole map (used without the atlas and
    /// for the low-zoom LOD).
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
        let map_tex = Some(Texture2D::from_rgba8(
            world.width as u16,
            world.height as u16,
            &rgba,
        ));
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

    /// Draw the follow view: map centered on the player, plus settlement,
    /// tomb, and player markers. `screen` is the current window size in
    /// pixels; `tomb` is the revealed tomb tile, if known.
    pub fn draw(
        &self,
        world: &World,
        player_pos: (u32, u32),
        screen: (f32, f32),
        tile_size: f32,
        tomb: Option<(u32, u32)>,
    ) {
        let cam = Camera::new(player_pos, screen, tile_size);
        draw_biome(
            world,
            &cam,
            self.atlas.as_ref(),
            self.map_tex.as_ref(),
            screen,
        );

        for s in &world.settlements {
            draw_marker(&cam, screen, s.x, s.y, self.settlement_tex.as_ref(), "S");
        }
        if let Some((tx, ty)) = tomb {
            draw_tomb(&cam, screen, (tx, ty), self.tomb_tex.as_ref());
        }
        draw_marker(
            &cam,
            screen,
            player_pos.0,
            player_pos.1,
            self.player_tex.as_ref(),
            "P",
        );
    }

    /// Draw the explore view: free camera with LOD, fog of war, settlement
    /// labels (semantic zoom), scouted routes, and the tomb pin.
    pub fn draw_explore(&self, world: &World, game: &Game, cam: &MapCamera, screen: (f32, f32)) {
        let ts = cam.tile_size();
        match lod_for_tile_size(ts) {
            Lod::Flat => draw_biome(world, cam, None, self.map_tex.as_ref(), screen),
            Lod::Atlas => draw_biome(
                world,
                cam,
                self.atlas.as_ref(),
                self.map_tex.as_ref(),
                screen,
            ),
        }

        draw_fog(world, &self.visited(game), cam, screen);

        for s in &world.settlements {
            draw_marker(cam, screen, s.x, s.y, self.settlement_tex.as_ref(), "S");
        }
        if let Some((tx, ty)) = game.tomb_revealed {
            draw_tomb(cam, screen, (tx, ty), self.tomb_tex.as_ref());
        }
        let (px, py) = game.player.pos();
        draw_marker(cam, screen, px, py, self.player_tex.as_ref(), "P");

        for label in self.settlement_labels(world, game, cam, screen) {
            self.draw_label(&label, ts);
        }

        draw_routes(world, game, cam, screen);
    }

    /// Visited tiles as a set, for the fog overlay.
    fn visited(&self, game: &Game) -> std::collections::BTreeSet<(u32, u32)> {
        game.player.visited().iter().copied().collect()
    }

    /// Settlement labels for the current zoom: names from 16 px/tile,
    /// polity sub-labels from 28 px/tile for known labels only.
    fn settlement_labels(
        &self,
        world: &World,
        game: &Game,
        cam: &MapCamera,
        screen: (f32, f32),
    ) -> Vec<PlacedLabel> {
        let ts = cam.tile_size();
        let items: Vec<LabelItem> = world
            .settlements
            .iter()
            .map(|s| LabelItem {
                x: s.x,
                y: s.y,
                name: s.name.clone(),
                polity: Some(s.polity.clone()),
                show_name: ts >= NAME_ZOOM,
                show_polity: ts >= POLITY_ZOOM && game.known_labels.contains(&s.name),
            })
            .collect();
        place_labels(&items, cam, screen)
    }

    /// Draw a placed label: name above the tile, polity sub-label beneath
    /// it, both with a black halo for readability over any biome.
    fn draw_label(&self, label: &PlacedLabel, ts: f32) {
        let font_size = (ts * 0.6).clamp(9.0, 20.0);
        if !label.name.is_empty() {
            draw_text_halo(&label.name, label.x, label.y - 6.0, font_size);
        }
        if let Some(polity) = &label.polity {
            draw_text_halo(polity, label.x, label.y + ts * 0.55 + 8.0, font_size * 0.85);
        }
    }
}

/// Text with a black halo (four offset copies) so it stays readable over
/// any biome; centered on `x` (macroquad draws from the text baseline).
fn draw_text_halo(text: &str, x: f32, y: f32, size: f32) {
    let w = measure_text(text, None, size as u16, 1.0).width;
    let x = x - w / 2.0;
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        draw_text(text, x + dx, y + dy, size, BLACK);
    }
    draw_text(text, x, y, size, WHITE);
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
