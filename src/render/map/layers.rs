//! Draw layers over a [`TileCamera`] (WS1 renderer split): the biome
//! background (per-tile atlas or flat map texture) and the marker layer
//! (settlements, tomb, player) with placeholder fallbacks.

use macroquad::prelude::*;
use macroquad::texture::Texture2D;

use crate::render::map::camera::TileCamera;
use crate::world::World;

/// Atlas layout: one tile per variant column and biome row (32×32), two
/// variants — `hoatool gen-art` writes exactly this shape (art::TILE/VARIANTS).
pub const ATLAS_TILE: u32 = 32;
pub const ATLAS_VARIANTS: u32 = 2;

/// Placeholder marker colors (used until real art exists).
pub const SETTLEMENT_MARKER: Color = Color::new(1.0, 1.0, 1.0, 0.85);
pub const PLAYER_MARKER: Color = Color::new(1.0, 0.84, 0.2, 1.0);
pub const TOMB_MARKER: Color = Color::new(0.85, 0.25, 0.2, 1.0);

/// Draw the biome background under `cam`: per-tile atlas draws when `atlas`
/// is present (culled to the visible window), else the flat map texture
/// scaled to the world size.
pub fn draw_biome(
    world: &World,
    cam: &dyn TileCamera,
    atlas: Option<&Texture2D>,
    flat: Option<&Texture2D>,
    screen: (f32, f32),
) {
    if let Some(atlas) = atlas {
        draw_tiles(world, cam, atlas, screen);
        return;
    }
    if let Some(flat) = flat {
        let (ox, oy) = cam.offset(screen);
        let ts = cam.tile_size();
        draw_texture_ex(
            flat,
            ox,
            oy,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(world.width as f32 * ts, world.height as f32 * ts)),
                ..Default::default()
            },
        );
    }
}

/// Draw the visible tiles from the atlas, culled to the window.
fn draw_tiles(world: &World, cam: &dyn TileCamera, atlas: &Texture2D, screen: (f32, f32)) {
    let ts = cam.tile_size();
    let (ox, oy) = cam.offset(screen);
    let left = (-ox / ts).floor().max(0.0) as u32;
    let top = (-oy / ts).floor().max(0.0) as u32;
    let right = (((screen.0 - ox) / ts).ceil() as u32).min(world.width);
    let bottom = (((screen.1 - oy) / ts).ceil() as u32).min(world.height);

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
                ox + x as f32 * ts,
                oy + y as f32 * ts,
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
pub fn draw_marker(
    cam: &dyn TileCamera,
    screen: (f32, f32),
    x: u32,
    y: u32,
    tex: Option<&Texture2D>,
    label: &str,
) {
    let (px, py) = cam.tile_origin(screen, x, y);
    let tile_size = cam.tile_size();
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

/// Deterministic tile variant: a pure function of (x, y, row) so the map is
/// stable across frames, saves, and replays.
pub fn tile_variant(x: u32, y: u32, row: u32) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_variant_is_deterministic() {
        assert_eq!(tile_variant(3, 7, 2), tile_variant(3, 7, 2));
        assert!(tile_variant(3, 7, 2) < ATLAS_VARIANTS);
    }

    #[test]
    fn tile_variant_changes_with_position() {
        // The two atlas variants must be distributed by position: adjacent
        // tiles with different parity sums land on different variants.
        let v0 = tile_variant(0, 0, 1);
        let v1 = tile_variant(1, 0, 1);
        let v2 = tile_variant(0, 1, 1);
        assert_ne!(v0, v1, "x parity selects a different variant");
        assert_ne!(v0, v2, "y parity selects a different variant");
        assert_ne!(
            tile_variant(3, 7, 0),
            tile_variant(3, 7, 1),
            "row changes variant"
        );
    }
}
