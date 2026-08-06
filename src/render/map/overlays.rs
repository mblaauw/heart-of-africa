//! Map overlays (WS1 renderer split): fog of war over unvisited tiles,
//! scouted caravan routes as polylines, and the tomb pin. The pure part
//! (visible-tile enumeration) is headless-tested; the draws are thin
//! macroquad glue.

use macroquad::prelude::*;

use crate::render::map::camera::TileCamera;
use crate::render::map::layers::draw_marker;
use crate::sim::game::Game;
use crate::world::{Route, World};

/// Fog dims unvisited tiles by this alpha (0 = fully visible).
const FOG_ALPHA: f32 = 0.55;
/// Route colors by state: scouted safe (solid), scouted risky (amber),
/// blocked (grey). Unscouted routes are not drawn.
const ROUTE_COLOR: Color = Color::new(0.95, 0.95, 0.95, 0.5);
const RISKY_ROUTE_COLOR: Color = Color::new(1.0, 0.62, 0.25, 0.6);
const BLOCKED_ROUTE_COLOR: Color = Color::new(0.45, 0.45, 0.45, 0.5);
/// Risky-route dash pattern: `DASH_TILES` edges drawn, `GAP_TILES` skipped.
const DASH_TILES: usize = 3;
const GAP_TILES: usize = 2;

/// The tiles currently inside the window, in world order (y then x),
/// clamped to the map. Pure — no macroquad.
pub fn visible_tiles(
    cam: &dyn TileCamera,
    screen: (f32, f32),
    world: &World,
) -> impl Iterator<Item = (u32, u32)> {
    let ts = cam.tile_size();
    let (ox, oy) = cam.offset(screen);
    let left = (-ox / ts).floor().max(0.0) as u32;
    let top = (-oy / ts).floor().max(0.0) as u32;
    let right = (((screen.0 - ox) / ts).ceil() as u32).min(world.width);
    let bottom = (((screen.1 - oy) / ts).ceil() as u32).min(world.height);
    (top..bottom).flat_map(move |y| (left..right).map(move |x| (x, y)))
}

/// Dim every visible tile the player has not visited.
pub fn draw_fog(
    world: &World,
    visited: &std::collections::BTreeSet<(u32, u32)>,
    cam: &dyn TileCamera,
    screen: (f32, f32),
) {
    let ts = cam.tile_size();
    for (x, y) in visible_tiles(cam, screen, world) {
        if visited.contains(&(x, y)) {
            continue;
        }
        let (px, py) = cam.tile_origin(screen, x, y);
        draw_rectangle(px, py, ts, ts, Color::new(0.0, 0.0, 0.0, FOG_ALPHA));
    }
}

/// The screen-space polyline of a route: committed tiles when present,
/// else a straight line between the endpoint settlements (legacy content).
/// Pure — no macroquad.
pub fn route_polyline(route: &Route, world: &World) -> Vec<(u32, u32)> {
    if !route.tiles.is_empty() {
        return route.tiles.clone();
    }
    let start = world.settlements.iter().find(|s| s.id == route.from);
    let end = world.settlements.iter().find(|s| s.id == route.to);
    match (start, end) {
        (Some(a), Some(b)) => vec![(a.x, a.y), (b.x, b.y)],
        _ => Vec::new(),
    }
}

/// Split a tile polyline into dash/gap edge pairs: `dash` consecutive edges
/// drawn, `gap` skipped, repeating. Risk pattern for risky routes. Pure.
pub fn dash_tiles(pts: &[(u32, u32)], dash: usize, gap: usize) -> Vec<((u32, u32), (u32, u32))> {
    let mut out = Vec::new();
    let mut step = 0usize;
    while step + 1 < pts.len() {
        let mut drawn = 0;
        while step + 1 < pts.len() && drawn < dash {
            out.push((pts[step], pts[step + 1]));
            step += 1;
            drawn += 1;
        }
        step = (step + gap).min(pts.len().saturating_sub(1));
    }
    out
}

/// Draw scouted routes as polylines tinted by state: safe translucent
/// white (solid), risky amber (dashed), blocked grey (solid).
pub fn draw_routes(world: &World, game: &Game, cam: &dyn TileCamera, screen: (f32, f32)) {
    let ts = cam.tile_size();
    for route in &world.routes {
        if !game.route_scouted(route, world) {
            continue;
        }
        let pts = route_polyline(route, world);
        if pts.len() < 2 {
            continue;
        }
        let color = if route.blocked {
            BLOCKED_ROUTE_COLOR
        } else if route.risky {
            RISKY_ROUTE_COLOR
        } else {
            ROUTE_COLOR
        };
        let pairs: Vec<(u32, u32, u32, u32)> = if route.risky {
            dash_tiles(&pts, DASH_TILES, GAP_TILES)
                .iter()
                .map(|&((ax, ay), (bx, by))| (ax, ay, bx, by))
                .collect()
        } else {
            pts.windows(2)
                .map(|w| (w[0].0, w[0].1, w[1].0, w[1].1))
                .collect()
        };
        for (ax, ay, bx, by) in pairs {
            let (sx, sy) = cam.tile_origin(screen, ax, ay);
            let (ex, ey) = cam.tile_origin(screen, bx, by);
            draw_line(
                sx + ts / 2.0,
                sy + ts / 2.0,
                ex + ts / 2.0,
                ey + ts / 2.0,
                ts * 0.18,
                color,
            );
        }
    }
}

/// The revealed-tomb pin (tinted red until real art exists).
pub fn draw_tomb(
    cam: &dyn TileCamera,
    screen: (f32, f32),
    tile: (u32, u32),
    tex: Option<&macroquad::texture::Texture2D>,
) {
    draw_marker(cam, screen, tile.0, tile.1, tex, "T");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::map::camera::{Camera, MapCamera};
    use crate::world::World;

    fn fixture_world() -> World {
        crate::content::load::load_world(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/content"
        )))
        .expect("content loads in tests")
    }

    #[test]
    fn visible_tiles_cover_window_and_clamp() {
        let world = fixture_world();
        let cam = Camera::new((24, 33), (800.0, 600.0), 32.0);
        let tiles: Vec<_> = visible_tiles(&cam, (800.0, 600.0), &world).collect();
        assert!(!tiles.is_empty());
        for (x, y) in &tiles {
            assert!(*x < world.width && *y < world.height, "clamped to map");
        }
        // A camera far outside the map still yields only valid tiles.
        let far = Camera::new((0, 0), (800.0, 600.0), 32.0);
        let tiles: Vec<_> = visible_tiles(&far, (800.0, 600.0), &world).collect();
        assert!(
            tiles
                .iter()
                .all(|&(x, y)| x < world.width && y < world.height)
        );
    }

    #[test]
    fn visible_tiles_are_in_world_order() {
        let world = fixture_world();
        let cam = MapCamera::new((world.width, world.height));
        let tiles: Vec<_> = visible_tiles(&cam, (800.0, 600.0), &world).collect();
        for w in tiles.windows(2) {
            assert!(
                w[1].1 > w[0].1 || (w[1].1 == w[0].1 && w[1].0 > w[0].0),
                "y then x order"
            );
        }
    }

    #[test]
    fn route_polyline_falls_back_to_straight_line() {
        let world = fixture_world();
        let mut route = world.routes[0].clone();
        route.tiles.clear();
        let pts = route_polyline(&route, &world);
        assert_eq!(pts.len(), 2, "endpoints when no committed tiles");
        let start = world
            .settlements
            .iter()
            .find(|s| s.id == route.from)
            .unwrap();
        let end = world.settlements.iter().find(|s| s.id == route.to).unwrap();
        assert_eq!(pts[0], (start.x, start.y));
        assert_eq!(pts[1], (end.x, end.y));
    }

    #[test]
    fn route_polyline_uses_committed_tiles() {
        let world = fixture_world();
        let mut route = world.routes[0].clone();
        route.tiles = vec![(1, 2), (3, 4), (5, 6)];
        assert_eq!(route_polyline(&route, &world), route.tiles);
    }

    #[test]
    fn dash_tiles_alternates_dash_and_gap() {
        let pts: Vec<(u32, u32)> = (0..11).map(|x| (x, 0)).collect();
        let pairs = dash_tiles(&pts, 3, 2);
        let drawn: Vec<(u32, u32)> = pairs.iter().map(|&(a, _)| a).collect();
        assert_eq!(drawn, vec![(0, 0), (1, 0), (2, 0), (5, 0), (6, 0), (7, 0)]);
        for &(a, b) in &pairs {
            assert_eq!(b.0, a.0 + 1, "consecutive edges only");
        }
    }

    #[test]
    fn dash_tiles_single_edge_is_kept() {
        let pts = vec![(0, 0), (1, 1)];
        assert_eq!(dash_tiles(&pts, 3, 2), vec![((0, 0), (1, 1))]);
    }
}
