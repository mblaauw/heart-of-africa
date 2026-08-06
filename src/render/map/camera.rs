//! Cameras for the two map views (DESIGN.md §5, WS1 renderer split):
//! the gameplay-follow [`Camera`] (normal view, unchanged behaviour) and
//! the explore-mode [`MapCamera`] (zoom, pan, free look).
//!
//! All camera state is plain f32 math — no macroquad input types inside, so
//! the math is unit-testable and the input mapping stays in the binary.
//! Presentation-only by construction: camera ops never touch the sim.

/// Zoom clamp (tile pixel size) for the explore camera.
pub const MIN_ZOOM: f32 = 8.0;
pub const MAX_ZOOM: f32 = 64.0;

/// Common camera surface used by the draw layers.
pub trait TileCamera {
    /// Screen offset of tile (0, 0).
    fn offset(&self, screen: (f32, f32)) -> (f32, f32);
    /// Tile size in pixels.
    fn tile_size(&self) -> f32;
    /// Pixel origin (top-left) of a tile.
    fn tile_origin(&self, screen: (f32, f32), x: u32, y: u32) -> (f32, f32);

    /// Hit-test: the tile under a screen point, or `None` off the map
    /// (negative tile coordinates). Pure — no macroquad.
    fn tile_at(&self, screen: (f32, f32), point: (f32, f32)) -> Option<(u32, u32)> {
        let ts = self.tile_size();
        if ts <= 0.0 {
            return None;
        }
        let (ox, oy) = self.offset(screen);
        let tx = ((point.0 - ox) / ts).floor();
        let ty = ((point.1 - oy) / ts).floor();
        if tx < 0.0 || ty < 0.0 {
            return None;
        }
        Some((tx as u32, ty as u32))
    }
}

/// Screen-space follow camera: maps tile coordinates to pixel coordinates,
/// centered on a fixed tile (the player). The normal view's camera.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// Screen offset of tile (0, 0).
    x: f32,
    y: f32,
    tile_size: f32,
}

impl Camera {
    /// A camera centered on `center` (tile coords) in a `screen`-sized window.
    pub fn new(center: (u32, u32), screen: (f32, f32), tile_size: f32) -> Camera {
        Camera {
            x: screen.0 / 2.0 - (center.0 as f32 + 0.5) * tile_size,
            y: screen.1 / 2.0 - (center.1 as f32 + 0.5) * tile_size,
            tile_size,
        }
    }
}

impl TileCamera for Camera {
    fn offset(&self, screen: (f32, f32)) -> (f32, f32) {
        let _ = screen;
        (self.x, self.y)
    }

    fn tile_size(&self) -> f32 {
        self.tile_size
    }

    fn tile_origin(&self, _screen: (f32, f32), x: u32, y: u32) -> (f32, f32) {
        (
            self.x + x as f32 * self.tile_size,
            self.y + y as f32 * self.tile_size,
        )
    }
}

/// Explore-mode camera: free pan over the map with wheel/± zoom anchored at
/// the cursor. `center` is the camera's focus in tile units (fractional
/// allowed for smooth panning); `zoom` is the tile pixel size.
#[derive(Debug, Clone, Copy)]
pub struct MapCamera {
    /// Camera focus in tile coordinates (world center by default).
    center: (f32, f32),
    /// Tile size in pixels; clamped to MIN_ZOOM..=MAX_ZOOM.
    zoom: f32,
    /// World size in tiles (clamp bounds).
    world: (u32, u32),
}

impl MapCamera {
    /// A camera centered on the world, at the default zoom.
    pub fn new(world: (u32, u32)) -> MapCamera {
        MapCamera {
            center: (world.0 as f32 / 2.0, world.1 as f32 / 2.0),
            zoom: 32.0,
            world,
        }
    }

    /// Tile coordinates of the world point under a screen pixel.
    pub fn screen_to_tile(&self, screen: (f32, f32), sx: f32, sy: f32) -> (f32, f32) {
        (
            self.center.0 + (sx - screen.0 / 2.0) / self.zoom,
            self.center.1 + (sy - screen.1 / 2.0) / self.zoom,
        )
    }

    /// The camera focus in tile coordinates.
    pub fn center_tile(&self) -> (f32, f32) {
        self.center
    }

    /// Pan by a pixel delta (positive = content follows the drag).
    pub fn pan(&mut self, dx_px: f32, dy_px: f32) {
        self.center.0 -= dx_px / self.zoom;
        self.center.1 -= dy_px / self.zoom;
        self.clamp_center();
    }

    /// Zoom by a factor, keeping the world point under `(sx, sy)` fixed.
    pub fn zoom_at(&mut self, factor: f32, screen: (f32, f32), sx: f32, sy: f32) {
        let anchor = self.screen_to_tile(screen, sx, sy);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        // Re-anchor: the world point under the cursor must stay put.
        self.center.0 = anchor.0 - (sx - screen.0 / 2.0) / self.zoom;
        self.center.1 = anchor.1 - (sy - screen.1 / 2.0) / self.zoom;
        self.clamp_center();
    }

    /// Center the camera on a tile (player or known-world center).
    pub fn center_on(&mut self, tx: f32, ty: f32) {
        self.center = (tx, ty);
        self.clamp_center();
    }

    /// Clamp the camera focus to the world bounds (no overscroll).
    fn clamp_center(&mut self) {
        let (w, h) = (self.world.0 as f32 - 1.0, self.world.1 as f32 - 1.0);
        self.center.0 = self.center.0.clamp(0.0, w.max(0.0));
        self.center.1 = self.center.1.clamp(0.0, h.max(0.0));
    }
}

impl TileCamera for MapCamera {
    fn offset(&self, screen: (f32, f32)) -> (f32, f32) {
        (
            screen.0 / 2.0 - self.center.0 * self.zoom,
            screen.1 / 2.0 - self.center.1 * self.zoom,
        )
    }

    fn tile_size(&self) -> f32 {
        self.zoom
    }

    fn tile_origin(&self, screen: (f32, f32), x: u32, y: u32) -> (f32, f32) {
        let (ox, oy) = self.offset(screen);
        (ox + x as f32 * self.zoom, oy + y as f32 * self.zoom)
    }
}

/// The two map views: gameplay follow (default) and free explore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Follow,
    Explore,
}

/// Bounds of the known world (visited tiles), for the Home key.
#[derive(Debug, Clone, Copy, Default)]
pub struct KnownBounds {
    pub min: Option<(u32, u32)>,
    pub max: Option<(u32, u32)>,
}

impl KnownBounds {
    /// Bounds over a list of visited tiles; `None` when empty.
    pub fn from_visited(visited: &[(u32, u32)]) -> KnownBounds {
        let mut b = KnownBounds::default();
        for &(x, y) in visited {
            match b.min {
                Some((mx, my)) => {
                    b.min = Some((mx.min(x), my.min(y)));
                    b.max = Some((b.max.unwrap().0.max(x), b.max.unwrap().1.max(y)));
                }
                None => {
                    b.min = Some((x, y));
                    b.max = Some((x, y));
                }
            }
        }
        b
    }

    /// Tile coordinates of the known-world center, or the given fallback.
    pub fn center(&self, fallback: (f32, f32)) -> (f32, f32) {
        match (self.min, self.max) {
            (Some((mx, my)), Some((mx2, my2))) => {
                ((mx + mx2) as f32 / 2.0, (my + my2) as f32 / 2.0)
            }
            _ => fallback,
        }
    }
}

/// The presentation-only map view state: which mode and, in explore mode,
/// the camera. Moved by [`apply_view_input`]; drawn by the renderer.
#[derive(Debug, Clone)]
pub struct MapView {
    pub mode: ViewMode,
    pub camera: MapCamera,
}

impl MapView {
    pub fn new(world: (u32, u32)) -> MapView {
        MapView {
            mode: ViewMode::Follow,
            camera: MapCamera::new(world),
        }
    }

    /// Enter explore mode, centered on the player.
    pub fn enter_explore(&mut self, player: (u32, u32)) {
        self.camera
            .center_on(player.0 as f32 + 0.5, player.1 as f32 + 0.5);
        self.mode = ViewMode::Explore;
    }

    /// Return to the gameplay follow view.
    pub fn exit_explore(&mut self) {
        self.mode = ViewMode::Follow;
    }
}

/// Per-frame camera input, read from the window by the binary and applied
/// here. Pure state transition — never touches the sim or the save.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewInput {
    pub tab: bool,
    pub esc: bool,
    pub f: bool,
    pub home: bool,
    pub zoom_in: bool,
    pub zoom_out: bool,
    /// Mouse wheel scroll (positive = zoom in).
    pub wheel: f32,
    /// Drag delta in pixels while the mouse is held.
    pub drag: Option<(f32, f32)>,
    /// Cursor position in pixels (zoom anchor).
    pub mouse: (f32, f32),
    pub screen: (f32, f32),
}

/// Apply one frame of camera input to the view. Presentation-only: returns
/// nothing, changes nothing outside `view`. `known` is the player's visited
/// bounds, computed by the caller (Home centers on it).
pub fn apply_view_input(
    view: &mut MapView,
    input: ViewInput,
    player: (u32, u32),
    known: KnownBounds,
) {
    if input.tab {
        match view.mode {
            ViewMode::Follow => view.enter_explore(player),
            ViewMode::Explore => view.exit_explore(),
        }
        return;
    }
    if view.mode == ViewMode::Follow {
        return;
    }
    if input.esc {
        view.exit_explore();
        return;
    }
    if input.f {
        view.camera
            .center_on(player.0 as f32 + 0.5, player.1 as f32 + 0.5);
    }
    if input.home {
        let c = known.center((player.0 as f32 + 0.5, player.1 as f32 + 0.5));
        view.camera.center_on(c.0, c.1);
    }
    let factor = wheel_factor(input.wheel) * key_zoom_factor(input.zoom_in, input.zoom_out);
    if (factor - 1.0).abs() > f32::EPSILON {
        view.camera
            .zoom_at(factor, input.screen, input.mouse.0, input.mouse.1);
    }
    if let Some((dx, dy)) = input.drag {
        view.camera.pan(dx, dy);
    }
}

/// Wheel scroll to zoom factor: one notch ≈ 12% per step.
fn wheel_factor(wheel: f32) -> f32 {
    if wheel == 0.0 {
        1.0
    } else {
        (1.12_f32).powf(wheel)
    }
}

/// ± key zoom: one step ≈ 25%.
fn key_zoom_factor(zoom_in: bool, zoom_out: bool) -> f32 {
    match (zoom_in, zoom_out) {
        (true, _) => 1.25,
        (false, true) => 1.0 / 1.25,
        (false, false) => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_camera_centers_on_tile() {
        let cam = Camera::new((10, 10), (800.0, 600.0), 32.0);
        let (ox, oy) = cam.offset((800.0, 600.0));
        assert_eq!(ox, 400.0 - 10.5 * 32.0);
        assert_eq!(oy, 300.0 - 10.5 * 32.0);
        let (px, py) = cam.tile_origin((800.0, 600.0), 10, 10);
        assert!((px - 400.0 + 16.0).abs() < 1e-3);
        assert!((py - 300.0 + 16.0).abs() < 1e-3);
    }

    #[test]
    fn map_camera_zoom_clamps() {
        let mut cam = MapCamera::new((48, 66));
        cam.zoom_at(1000.0, (800.0, 600.0), 400.0, 300.0);
        assert_eq!(cam.zoom, MAX_ZOOM);
        cam.zoom_at(0.0001, (800.0, 600.0), 400.0, 300.0);
        assert_eq!(cam.zoom, MIN_ZOOM);
    }

    #[test]
    fn zoom_at_keeps_cursor_anchor_fixed() {
        let mut cam = MapCamera::new((48, 66));
        let (sx, sy) = (200.0, 400.0);
        let screen = (800.0, 600.0);
        let before = cam.screen_to_tile(screen, sx, sy);
        cam.zoom_at(1.7, screen, sx, sy);
        let after = cam.screen_to_tile(screen, sx, sy);
        assert!((before.0 - after.0).abs() < 1e-3);
        assert!((before.1 - after.1).abs() < 1e-3);
        assert!(cam.zoom > 32.0);
    }

    #[test]
    fn pan_moves_center_opposite_to_drag() {
        let mut cam = MapCamera::new((48, 66));
        cam.center_on(20.0, 20.0);
        cam.pan(64.0, -32.0);
        assert!((cam.center.0 - (20.0 - 2.0)).abs() < 1e-3);
        assert!((cam.center.1 - (20.0 + 1.0)).abs() < 1e-3);
    }

    #[test]
    fn center_clamps_to_world() {
        let mut cam = MapCamera::new((48, 66));
        cam.center_on(-100.0, 500.0);
        assert_eq!(cam.center, (0.0, 65.0));
        cam.center_on(100.0, -500.0);
        assert_eq!(cam.center, (47.0, 0.0));
    }

    #[test]
    fn tile_origin_matches_offset_math() {
        let cam = MapCamera::new((48, 66));
        let (ox, oy) = cam.offset((800.0, 600.0));
        let (px, py) = cam.tile_origin((800.0, 600.0), 3, 7);
        assert!((px - (ox + 3.0 * 32.0)).abs() < 1e-3);
        assert!((py - (oy + 7.0 * 32.0)).abs() < 1e-3);
    }

    #[test]
    fn known_bounds_from_visited() {
        let visited = vec![(5, 5), (1, 9), (3, 2)];
        let b = KnownBounds::from_visited(&visited);
        assert_eq!(b.min, Some((1, 2)));
        assert_eq!(b.max, Some((5, 9)));
        let c = b.center((0.0, 0.0));
        assert!((c.0 - 3.0).abs() < 1e-3);
        assert!((c.1 - 5.5).abs() < 1e-3);
        assert_eq!(
            KnownBounds::default().center((7.0, 7.0)),
            (7.0, 7.0),
            "empty bounds fall back"
        );
    }

    #[test]
    fn wheel_and_key_zoom_factors() {
        assert_eq!(wheel_factor(0.0), 1.0);
        assert!(wheel_factor(1.0) > 1.0);
        assert!(wheel_factor(-1.0) < 1.0);
        assert!((wheel_factor(2.0) - wheel_factor(1.0).powi(2)).abs() < 1e-3);
        assert_eq!(key_zoom_factor(true, false), 1.25);
        assert_eq!(key_zoom_factor(false, true), 1.0 / 1.25);
        assert_eq!(key_zoom_factor(false, false), 1.0);
    }

    #[test]
    fn tab_toggles_explore_and_centers_on_player() {
        let mut view = MapView::new((48, 66));
        assert_eq!(view.mode, ViewMode::Follow);
        let input = ViewInput {
            tab: true,
            ..Default::default()
        };
        apply_view_input(&mut view, input, (12, 34), KnownBounds::default());
        assert_eq!(view.mode, ViewMode::Explore);
        assert!((view.camera.center.0 - 12.5).abs() < 1e-3);
        assert!((view.camera.center.1 - 34.5).abs() < 1e-3);
        apply_view_input(&mut view, input, (12, 34), KnownBounds::default());
        assert_eq!(view.mode, ViewMode::Follow);
    }

    #[test]
    fn follow_mode_ignores_camera_input() {
        let mut view = MapView::new((48, 66));
        let input = ViewInput {
            zoom_in: true,
            drag: Some((50.0, 0.0)),
            ..Default::default()
        };
        apply_view_input(&mut view, input, (1, 1), KnownBounds::default());
        assert_eq!(view.camera.zoom, 32.0, "no zoom in follow mode");
        view.enter_explore((1, 1));
        let input = ViewInput {
            esc: true,
            ..Default::default()
        };
        apply_view_input(&mut view, input, (1, 1), KnownBounds::default());
        assert_eq!(view.mode, ViewMode::Follow);
        let input = ViewInput {
            zoom_in: true,
            ..Default::default()
        };
        apply_view_input(&mut view, input, (1, 1), KnownBounds::default());
        assert_eq!(view.camera.zoom, 32.0);
    }

    #[test]
    fn esc_exits_explore() {
        let mut view = MapView::new((48, 66));
        view.enter_explore((3, 3));
        apply_view_input(
            &mut view,
            ViewInput {
                esc: true,
                ..Default::default()
            },
            (3, 3),
            KnownBounds::default(),
        );
        assert_eq!(view.mode, ViewMode::Follow);
    }

    #[test]
    fn tile_at_round_trips_tile_origin() {
        let cam = Camera::new((10, 10), (800.0, 600.0), 32.0);
        let (px, py) = cam.tile_origin((800.0, 600.0), 10, 10);
        assert_eq!(
            cam.tile_at((800.0, 600.0), (px + 1.0, py + 1.0)),
            Some((10, 10))
        );
        assert_eq!(
            cam.tile_at((800.0, 600.0), (px - 1.0, py - 1.0)),
            Some((9, 9))
        );
    }

    #[test]
    fn tile_at_off_map_is_none() {
        let cam = MapCamera::new((48, 66));
        let (ox, oy) = cam.offset((800.0, 600.0));
        assert_eq!(cam.tile_at((800.0, 600.0), (ox - 1.0, oy - 1.0)), None);
        assert_eq!(
            cam.tile_at((800.0, 600.0), (ox + 1.0, oy + 1.0)),
            Some((0, 0))
        );
    }

    #[test]
    fn tile_at_matches_visible_tiles() {
        let cam = MapCamera::new((48, 66));
        let (px, py) = cam.tile_origin((800.0, 600.0), 7, 9);
        assert_eq!(
            cam.tile_at((800.0, 600.0), (px + 16.0, py + 16.0)),
            Some((7, 9))
        );
    }
}
