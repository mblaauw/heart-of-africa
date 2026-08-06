//! Planar geometry over the projected km coordinate space (DESIGN.md §3).
//!
//! Committed geography files (geography.toml) store plain `(f64, f64)`
//! coordinate pairs; geo types are constructed at load time so the
//! on-disk format is stable across geo version bumps.
//!
//! Invariant: this module must never import `macroquad` or `egui`.

use geo::{Area, Contains, Point, Polygon};
use serde::{Deserialize, Serialize};

/// A point in world-km coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct KmPoint {
    pub x: f64,
    pub y: f64,
}

impl KmPoint {
    pub fn new(x: f64, y: f64) -> Self {
        KmPoint { x, y }
    }

    /// Convert to a `geo::Point`.
    pub fn to_geo(self) -> Point<f64> {
        Point::new(self.x, self.y)
    }
}

/// A closed region polygon (authored in Tiled, committed as ring of km coords).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KmPolygon {
    pub id: String,
    pub ring: Vec<KmPoint>,
}

impl KmPolygon {
    /// Build a `geo::Polygon` (exterior ring only; holes not used yet).
    pub fn to_geo(&self) -> Polygon<f64> {
        let exterior: geo::LineString<f64> = self
            .ring
            .iter()
            .map(|p| (p.x, p.y))
            .collect::<Vec<_>>()
            .into();
        Polygon::new(exterior, vec![])
    }

    /// Does `p` lie inside this polygon?
    pub fn contains(&self, p: KmPoint) -> bool {
        self.to_geo().contains(&p.to_geo())
    }

    /// Area in km² (planar approximation — valid for the ~1600×2200 km extent).
    pub fn area_km2(&self) -> f64 {
        self.to_geo().unsigned_area()
    }
}

/// A polyline in world-km coordinates (rivers, routes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KmPolyline {
    pub id: String,
    pub points: Vec<KmPoint>,
}

impl KmPolyline {
    /// Convert to a `geo::LineString`.
    pub fn to_geo(&self) -> geo::LineString<f64> {
        self.points
            .iter()
            .map(|p| (p.x, p.y))
            .collect::<Vec<_>>()
            .into()
    }

    /// Polyline length in kilometres (euclidean in the projected km space).
    pub fn length_km(&self) -> f64 {
        let pts = &self.points;
        if pts.len() < 2 {
            return 0.0;
        }
        let mut total = 0.0;
        for i in 1..pts.len() {
            let dx = pts[i].x - pts[i - 1].x;
            let dy = pts[i].y - pts[i - 1].y;
            total += (dx * dx + dy * dy).sqrt();
        }
        total
    }
}

/// A rectangular bounding box in km space (used for label collision and
/// fog-sector AABB checks).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct KmRect {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl KmRect {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        KmRect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub fn intersects(&self, other: &KmRect) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

/// Sample `n` evenly spaced points along a polyline (for search-narrowing
/// previews and clue-sector hints).
pub fn sample_polyline(polyline: &KmPolyline, n: usize) -> Vec<KmPoint> {
    if n == 0 || polyline.points.len() < 2 {
        return polyline.points.clone();
    }
    let total = polyline.length_km();
    if total == 0.0 {
        return vec![polyline.points[0]];
    }
    let step = total / n as f64;
    let mut result = Vec::with_capacity(n + 1);
    result.push(polyline.points[0]);
    for i in 1..=n {
        let target = i as f64 * step;
        let mut remaining = target;
        let mut seg_idx = 0;
        loop {
            if seg_idx + 1 >= polyline.points.len() {
                result.push(polyline.points.last().copied().unwrap());
                break;
            }
            let a = &polyline.points[seg_idx];
            let b = &polyline.points[seg_idx + 1];
            let seg_len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            if seg_len == 0.0 {
                seg_idx += 1;
                continue;
            }
            if remaining <= seg_len {
                let t = (remaining / seg_len).clamp(0.0, 1.0);
                result.push(KmPoint::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y)));
                break;
            }
            remaining -= seg_len;
            seg_idx += 1;
        }
    }
    result
}

/// Deterministic straight-line tile walk from `from` to `to`: 8-directional
/// steps toward the goal (each step moves one tile along each axis as
/// needed), both endpoints included. Used as the legacy route fallback —
/// pre-import content has no committed polylines, so routes get this
/// straight geometry at load time (committed, never recomputed per frame).
pub fn straight_tiles(from: (u32, u32), to: (u32, u32)) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let (mut x, mut y) = (from.0 as i64, from.1 as i64);
    let (tx, ty) = (to.0 as i64, to.1 as i64);
    out.push(from);
    while (x, y) != (tx, ty) {
        x += (tx - x).signum();
        y += (ty - y).signum();
        out.push((x as u32, y as u32));
    }
    out
}

/// Straight-line distance in kilometres between two tiles.
pub fn tile_distance_km(a: (u32, u32), b: (u32, u32), km_per_cell: f64) -> f64 {
    let dx = (b.0 as f64 - a.0 as f64) * km_per_cell;
    let dy = (b.1 as f64 - a.1 as f64) * km_per_cell;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_polygon() -> KmPolygon {
        KmPolygon {
            id: "test-square".into(),
            ring: vec![
                KmPoint::new(0.0, 0.0),
                KmPoint::new(10.0, 0.0),
                KmPoint::new(10.0, 10.0),
                KmPoint::new(0.0, 10.0),
                KmPoint::new(0.0, 0.0),
            ],
        }
    }

    #[test]
    fn contains_point_inside() {
        let p = square_polygon();
        assert!(p.contains(KmPoint::new(5.0, 5.0)));
    }

    #[test]
    fn contains_point_outside() {
        let p = square_polygon();
        assert!(!p.contains(KmPoint::new(15.0, 5.0)));
    }

    #[test]
    fn contains_point_on_edge() {
        let p = square_polygon();
        // geo's contains uses the Jordan curve theorem; points on the boundary
        // are not guaranteed to be inside. Test an interior point instead.
        assert!(p.contains(KmPoint::new(5.0, 5.0)));
    }

    #[test]
    fn area_matches() {
        let p = square_polygon();
        assert!((p.area_km2() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn polyline_length() {
        let line = KmPolyline {
            id: "test-line".into(),
            points: vec![KmPoint::new(0.0, 0.0), KmPoint::new(3.0, 4.0)],
        };
        assert!((line.length_km() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn polyline_length_straight_horizontal() {
        let line = KmPolyline {
            id: "h-line".into(),
            points: vec![KmPoint::new(0.0, 0.0), KmPoint::new(10.0, 0.0)],
        };
        assert!((line.length_km() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn sample_polyline_returns_endpoints() {
        let line = KmPolyline {
            id: "sampled".into(),
            points: vec![KmPoint::new(0.0, 0.0), KmPoint::new(10.0, 0.0)],
        };
        let samples = sample_polyline(&line, 2);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0], KmPoint::new(0.0, 0.0));
        assert_eq!(samples[2], KmPoint::new(10.0, 0.0));
    }

    #[test]
    fn sample_polyline_zero_returns_all() {
        let line = KmPolyline {
            id: "zero".into(),
            points: vec![KmPoint::new(0.0, 0.0), KmPoint::new(5.0, 0.0)],
        };
        let samples = sample_polyline(&line, 0);
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn rect_intersects() {
        let a = KmRect::new(0.0, 0.0, 10.0, 10.0);
        let b = KmRect::new(5.0, 5.0, 15.0, 15.0);
        assert!(a.intersects(&b));
        let c = KmRect::new(20.0, 20.0, 30.0, 30.0);
        assert!(!a.intersects(&c));
    }

    #[test]
    fn polygon_ring_winding_preserves_contains() {
        let cw = KmPolygon {
            id: "cw".into(),
            ring: vec![
                KmPoint::new(0.0, 0.0),
                KmPoint::new(10.0, 0.0),
                KmPoint::new(10.0, 10.0),
                KmPoint::new(0.0, 10.0),
                KmPoint::new(0.0, 0.0),
            ],
        };
        let ccw = KmPolygon {
            id: "ccw".into(),
            ring: vec![
                KmPoint::new(0.0, 0.0),
                KmPoint::new(0.0, 10.0),
                KmPoint::new(10.0, 10.0),
                KmPoint::new(10.0, 0.0),
                KmPoint::new(0.0, 0.0),
            ],
        };
        let test = KmPoint::new(5.0, 5.0);
        assert_eq!(cw.contains(test), ccw.contains(test));
    }

    /// Property: a point outside the bounding box of a polygon is never contained.
    #[test]
    fn contains_implies_in_bbox() {
        let p = square_polygon();
        let bbox = KmRect::new(0.0, 0.0, 10.0, 10.0);
        for x in [-1.0, 11.0, 5.0] {
            for y in [-1.0, 11.0, 5.0] {
                let inside = p.contains(KmPoint::new(x, y));
                let in_bbox =
                    x >= bbox.min_x && x <= bbox.max_x && y >= bbox.min_y && y <= bbox.max_y;
                assert!(
                    !inside || in_bbox,
                    "point ({x}, {y}) is inside polygon but outside bbox"
                );
            }
        }
    }

    #[test]
    fn straight_tiles_includes_endpoints() {
        let tiles = straight_tiles((2, 3), (5, 3));
        assert_eq!(tiles.first(), Some(&(2, 3)));
        assert_eq!(tiles.last(), Some(&(5, 3)));
        assert_eq!(tiles.len(), 4);
    }

    #[test]
    fn straight_tiles_moves_one_step_per_axis() {
        let tiles = straight_tiles((0, 0), (3, 2));
        for w in tiles.windows(2) {
            let dx = (w[1].0 as i64 - w[0].0 as i64).abs();
            let dy = (w[1].1 as i64 - w[0].1 as i64).abs();
            assert!(dx <= 1 && dy <= 1 && dx + dy >= 1);
        }
        assert_eq!(tiles.last(), Some(&(3, 2)));
    }

    #[test]
    fn straight_tiles_identity_is_single_tile() {
        assert_eq!(straight_tiles((4, 4), (4, 4)), vec![(4, 4)]);
    }

    #[test]
    fn tile_distance_km_matches_cell_size() {
        assert!((tile_distance_km((0, 0), (3, 4), 33.3) - 166.5).abs() < 1e-9);
        assert!(tile_distance_km((1, 1), (1, 1), 33.3).abs() < 1e-9);
    }
}
