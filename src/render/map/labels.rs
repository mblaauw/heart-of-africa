//! Map label placement (WS1 renderer split).
//!
//! Pure geometry — no macroquad, so it is unit-tested headless. Placement
//! is deterministic: labels are processed in world order (settlement order
//! in `world.settlements`), and collision rejection uses a screen-space
//! grid, so the same camera state always yields the same labels. Only
//! settlement names are labeled in WS1 (no jitter: positions derive from
//! the camera math alone).

use crate::render::map::camera::TileCamera;

/// One label candidate: a settlement tile plus what to show at the current
/// zoom (semantic zoom decides `show_name` / `show_polity`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelItem {
    pub x: u32,
    pub y: u32,
    pub name: String,
    /// Sub-label (e.g. polity name); only drawn when `show_polity`.
    pub polity: Option<String>,
    pub show_name: bool,
    pub show_polity: bool,
}

/// A placed label in screen space, above its tile.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabel {
    /// Anchor: tile center in pixels.
    pub x: f32,
    pub y: f32,
    pub name: String,
    pub polity: Option<String>,
}

/// Collision grid cell size in pixels: roughly a label's width, so at most
/// one label lands per cell.
const CELL: f32 = 180.0;
/// Labels whose anchor is more than this far off-screen are skipped.
const MARGIN: f32 = 300.0;

/// Place labels for `items` under `cam`. Deterministic: the same input
/// yields the same output, and collisions are rejected in input order
/// (world order), so the result never flickers.
pub fn place_labels(
    items: &[LabelItem],
    cam: &dyn TileCamera,
    screen: (f32, f32),
) -> Vec<PlacedLabel> {
    let mut taken: std::collections::BTreeSet<(i64, i64)> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for item in items {
        if !item.show_name && !item.show_polity {
            continue;
        }
        let (px, py) = cam.tile_origin(screen, item.x, item.y);
        let ts = cam.tile_size();
        let cx = px + ts / 2.0;
        let cy = py;
        if cx < -MARGIN || cx > screen.0 + MARGIN || cy < -MARGIN || cy > screen.1 + MARGIN {
            continue;
        }
        let cell = ((cx / CELL).floor() as i64, (cy / CELL).floor() as i64);
        if !taken.insert(cell) {
            continue;
        }
        out.push(PlacedLabel {
            x: cx,
            y: cy,
            name: if item.show_name {
                item.name.clone()
            } else {
                String::new()
            },
            polity: if item.show_polity {
                item.polity.clone()
            } else {
                None
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::map::camera::{Camera, MapCamera};

    fn items(pairs: &[(u32, u32, &str, &str)]) -> Vec<LabelItem> {
        pairs
            .iter()
            .map(|&(x, y, name, polity)| LabelItem {
                x,
                y,
                name: name.to_string(),
                polity: if polity.is_empty() {
                    None
                } else {
                    Some(polity.to_string())
                },
                show_name: true,
                show_polity: true,
            })
            .collect()
    }

    #[test]
    fn same_input_same_output() {
        let cam = Camera::new((20, 30), (800.0, 600.0), 32.0);
        let its = items(&[(10, 10, "A", "P1"), (30, 40, "B", "P2")]);
        let a = place_labels(&its, &cam, (800.0, 600.0));
        let b = place_labels(&its, &cam, (800.0, 600.0));
        assert_eq!(a, b, "placement must be deterministic");
    }

    #[test]
    fn collision_rejects_second_label_on_same_tile() {
        let cam = Camera::new((20, 30), (800.0, 600.0), 32.0);
        let its = items(&[(20, 30, "A", ""), (20, 30, "B", "")]);
        let placed = place_labels(&its, &cam, (800.0, 600.0));
        assert_eq!(placed.len(), 1, "one label per tile");
        assert_eq!(placed[0].name, "A", "first in world order wins");
    }

    #[test]
    fn off_screen_items_are_culled() {
        let cam = Camera::new((20, 30), (800.0, 600.0), 32.0);
        let its = items(&[(0, 0, "far", ""), (20, 30, "near", "")]);
        let placed = place_labels(&its, &cam, (800.0, 600.0));
        assert!(
            placed.iter().any(|l| l.name == "near"),
            "visible label placed"
        );
        assert!(
            placed.iter().all(|l| l.name != "far"),
            "offscreen label culled"
        );
    }

    #[test]
    fn semantic_zoom_filters_content() {
        let cam = Camera::new((20, 30), (800.0, 600.0), 32.0);
        let mut its = items(&[(20, 30, "A", "P")]);
        its[0].show_name = false;
        its[0].show_polity = true;
        let placed = place_labels(&its, &cam, (800.0, 600.0));
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].name, "", "name hidden at low zoom");
        assert_eq!(placed[0].polity.as_deref(), Some("P"));
    }

    #[test]
    fn nothing_placed_when_all_hidden() {
        let cam = MapCamera::new((48, 66));
        let its = vec![LabelItem {
            x: 24,
            y: 33,
            name: "A".to_string(),
            polity: None,
            show_name: false,
            show_polity: false,
        }];
        assert!(place_labels(&its, &cam, (800.0, 600.0)).is_empty());
    }

    #[test]
    fn label_anchors_above_tile_center() {
        let cam = Camera::new((20, 30), (800.0, 600.0), 32.0);
        let its = items(&[(20, 30, "A", "")]);
        let placed = place_labels(&its, &cam, (800.0, 600.0));
        assert_eq!(placed.len(), 1);
        let (px, py) = cam.tile_origin((800.0, 600.0), 20, 30);
        assert!(
            (placed[0].x - (px + 16.0)).abs() < 1e-3,
            "anchored at tile center x"
        );
        assert!((placed[0].y - py).abs() < 1e-3, "anchored at tile top y");
    }
}
