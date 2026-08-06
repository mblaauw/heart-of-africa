//! Stable geographic world coordinates (expansion spec, Part III).
//!
//! Three concepts stay separate: the simulation grid (u32 tiles — the
//! source of truth for gameplay), geographic world coordinates (kilometer-
//! like f64, derived from the grid by a fixed scale so the two can never
//! drift), and presentation pixels (map tiles, screen).
//!
//! Conversion rules:
//! - `sim tile (x, y)` -> world: the tile's center, `((x+0.5)*SCALE, (y+0.5)*SCALE)`.
//! - `world -> sim`: floor into the containing tile.
//! - world <-> map pixels: linear over `WorldBounds` into `presentation_size_px`.
//! - world <-> screen: linear from an explicit (center, zoom, screen size)
//!   triple — the camera wraps these in the render layer.

use serde::{Deserialize, Serialize};

/// Kilometers per simulation cell. ~33.3 km/cell over 48×66 cells gives a
/// ~1600×2200 km extent — the plausible straight-line span from the Nile
/// delta to Lake Chad. Travel time derives from these distances (WS1/WS4).
pub const KM_PER_CELL: f64 = 33.3;

/// A position in projected kilometer-like world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldPoint {
    pub x: f64,
    pub y: f64,
}

impl WorldPoint {
    pub fn new(x: f64, y: f64) -> Self {
        WorldPoint { x, y }
    }

    /// Straight-line distance in kilometers.
    pub fn km_to(self, other: WorldPoint) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Manhattan distance in kilometers (used for rough previews).
    pub fn manhattan_km(self, other: WorldPoint) -> f64 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }
}

/// The projected extent of the world in kilometers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldBounds {
    pub min: WorldPoint,
    pub max: WorldPoint,
}

impl WorldBounds {
    pub fn width_km(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height_km(&self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn contains(&self, p: WorldPoint) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

/// Projection between sim tiles, world kilometers, and presentation pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MapProjection {
    pub bounds: WorldBounds,
    /// Size of the full presentation map in pixels (the LOD-0 source).
    pub presentation_size_px: (u32, u32),
}

/// The authored high-resolution source size for the two-region extent
/// (expansion spec: minimum long side 4096; tall extent ~4096×5632).
/// Used as a validation target; the runtime projection is built from
/// the committed manifest (TMJ map size) so the authored source is
/// the truth.
pub const DEFAULT_PRESENTATION_PX: (u32, u32) = (4096, 5632);

/// Projection parameters sourced from the committed map manifest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectionManifest {
    pub presentation_size_px: (u32, u32),
}

impl ProjectionManifest {
    /// Build a `MapProjection` from manifest dimensions and a sim grid.
    pub fn projection(&self, width: u32, height: u32) -> MapProjection {
        MapProjection::new(width, height, self.presentation_size_px)
    }
}

impl MapProjection {
    /// Build the projection for a sim grid of `width`×`height` tiles
    /// and a presentation canvas of `presentation_size_px` pixels.
    ///
    /// The canvas size comes from the committed map manifest (TMJ map
    /// width/height), not from a hard-coded constant — the authored
    /// source is the truth.
    pub fn new(width: u32, height: u32, presentation_size_px: (u32, u32)) -> Self {
        MapProjection {
            bounds: WorldBounds {
                min: WorldPoint::new(0.0, 0.0),
                max: WorldPoint::new(width as f64 * KM_PER_CELL, height as f64 * KM_PER_CELL),
            },
            presentation_size_px,
        }
    }

    /// The containing sim tile of a world position.
    pub fn world_to_sim(&self, p: WorldPoint) -> (u32, u32) {
        let x = ((p.x - self.bounds.min.x) / KM_PER_CELL).floor().max(0.0);
        let y = ((p.y - self.bounds.min.y) / KM_PER_CELL).floor().max(0.0);
        let tiles_x = (self.bounds.width_km() / KM_PER_CELL) as u32;
        let tiles_y = (self.bounds.height_km() / KM_PER_CELL) as u32;
        (
            (x as u32).min(tiles_x.saturating_sub(1)),
            (y as u32).min(tiles_y.saturating_sub(1)),
        )
    }

    /// The world position of a sim tile's center.
    pub fn sim_to_world(&self, x: u32, y: u32) -> WorldPoint {
        WorldPoint::new(
            self.bounds.min.x + (x as f64 + 0.5) * KM_PER_CELL,
            self.bounds.min.y + (y as f64 + 0.5) * KM_PER_CELL,
        )
    }

    /// World position -> full-map presentation pixels.
    pub fn world_to_map_pixels(&self, p: WorldPoint) -> (f64, f64) {
        let (w, h) = (
            self.presentation_size_px.0 as f64,
            self.presentation_size_px.1 as f64,
        );
        (
            (p.x - self.bounds.min.x) / self.bounds.width_km() * w,
            (p.y - self.bounds.min.y) / self.bounds.height_km() * h,
        )
    }

    /// Full-map presentation pixels -> world position.
    pub fn map_pixels_to_world(&self, px: f64, py: f64) -> WorldPoint {
        let (w, h) = (
            self.presentation_size_px.0 as f64,
            self.presentation_size_px.1 as f64,
        );
        WorldPoint::new(
            self.bounds.min.x + px / w * self.bounds.width_km(),
            self.bounds.min.y + py / h * self.bounds.height_km(),
        )
    }

    /// World position -> screen pixels for a camera at `center` with
    /// `zoom` (screen km per pixel) in a `screen_px` viewport.
    pub fn world_to_screen(
        &self,
        p: WorldPoint,
        center: WorldPoint,
        zoom: f64,
        screen_px: (f64, f64),
    ) -> (f64, f64) {
        let k = zoom;
        (
            screen_px.0 / 2.0 + (p.x - center.x) / k,
            screen_px.1 / 2.0 + (p.y - center.y) / k,
        )
    }

    /// Screen pixels -> world position (inverse of `world_to_screen`).
    pub fn screen_to_world(
        &self,
        sx: f64,
        sy: f64,
        center: WorldPoint,
        zoom: f64,
        screen_px: (f64, f64),
    ) -> WorldPoint {
        let k = zoom;
        WorldPoint::new(
            center.x + (sx - screen_px.0 / 2.0) * k,
            center.y + (sy - screen_px.1 / 2.0) * k,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real world: 48×66 cells (Lower Egypt + Sahara/Sahel corridor).
    fn proj() -> MapProjection {
        MapProjection::new(48, 66, DEFAULT_PRESENTATION_PX)
    }

    #[test]
    fn bounds_are_kilometer_sized() {
        let p = proj();
        assert!((p.bounds.width_km() - 48.0 * KM_PER_CELL).abs() < 1e-9);
        assert!((p.bounds.height_km() - 66.0 * KM_PER_CELL).abs() < 1e-9);
        assert!(p.bounds.width_km() > 1500.0 && p.bounds.height_km() > 2000.0);
    }

    #[test]
    fn sim_world_round_trips() {
        let p = proj();
        for (x, y) in [(0u32, 0u32), (15, 8), (16, 28), (47, 65)] {
            let wp = p.sim_to_world(x, y);
            assert_eq!(p.world_to_sim(wp), (x, y), "tile {x},{y} round trips");
        }
        // Interior world points map back to the tile they are inside of.
        let wp = WorldPoint::new(15.0 * KM_PER_CELL, 8.0 * KM_PER_CELL);
        assert_eq!(p.world_to_sim(wp), (15, 8));
    }

    #[test]
    fn world_to_sim_clamps_to_bounds() {
        let p = proj();
        assert_eq!(p.world_to_sim(WorldPoint::new(-50.0, 5.0)), (0, 0));
        assert_eq!(p.world_to_sim(WorldPoint::new(1e6, 1e6)), (47, 65));
    }

    #[test]
    fn pixel_round_trips() {
        let p = proj();
        let cases = [
            WorldPoint::new(0.0, 0.0),
            WorldPoint::new(800.0, 1100.0),
            WorldPoint::new(1598.4, 2197.8),
        ];
        for wp in cases {
            let (px, py) = p.world_to_map_pixels(wp);
            let back = p.map_pixels_to_world(px, py);
            assert!((back.x - wp.x).abs() < 1e-6 && (back.y - wp.y).abs() < 1e-6);
        }
        // Pixels stay inside the presentation size.
        let (px, py) = p.world_to_map_pixels(WorldPoint::new(1598.4, 2197.8));
        assert!(px <= 4096.0 + 1e-9 && py <= 5632.0 + 1e-9);
    }

    #[test]
    fn screen_round_trips() {
        let p = proj();
        let center = WorldPoint::new(800.0, 1100.0);
        let screen = (1920.0, 1080.0);
        let zoom = 1.0; // 1 km per screen pixel
        let wp = WorldPoint::new(900.0, 1150.0);
        let (sx, sy) = p.world_to_screen(wp, center, zoom, screen);
        let back = p.screen_to_world(sx, sy, center, zoom, screen);
        assert!((back.x - wp.x).abs() < 1e-9 && (back.y - wp.y).abs() < 1e-9);
    }

    #[test]
    fn plausible_distances() {
        let p = proj();
        // Cairo (15,8) -> Aswan (16,28): ~21 cells south -> ~700 km.
        let cairo = p.sim_to_world(15, 8);
        let aswan = p.sim_to_world(16, 28);
        let d = cairo.km_to(aswan);
        assert!(d > 650.0 && d < 750.0, "Cairo-Aswan ~700 km, got {d}");
    }
}
