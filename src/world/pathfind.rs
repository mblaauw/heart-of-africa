//! Hand-rolled A* pathfinding over the biome grid (DESIGN.md §8.4).
//!
//! Deliberately no `pathfinding` crate: a fully visible, integer-cost A* is a
//! pattern agents implement reliably. Determinism: ties are broken by tile
//! position, so the returned path is a pure function of (grid, start, goal).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use super::biome::BiomeGrid;

/// Movement costs are f32 in data; A* runs on integer milli-costs so the
/// priority-queue ordering is total and deterministic.
const COST_SCALE: u32 = 100;

fn cost_milli(grid: &BiomeGrid, x: u32, y: u32) -> u32 {
    (grid.movement_cost_at(x, y) * COST_SCALE as f32).round() as u32
}

fn manhattan(a: (u32, u32), b: (u32, u32)) -> u32 {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

/// Shortest path from `start` to `goal`, inclusive of both endpoints, or
/// `None` when the goal is unreachable (out of bounds, impassable, or sealed).
///
/// Cost of a path is the sum of movement costs of entered tiles; the start
/// tile costs nothing. Four-neighbor movement.
pub fn find_path(grid: &BiomeGrid, start: (u32, u32), goal: (u32, u32)) -> Option<Vec<(u32, u32)>> {
    if start == goal {
        return Some(vec![start]);
    }
    if !grid.in_bounds(start.0, start.1) || !grid.in_bounds(goal.0, goal.1) {
        return None;
    }
    if !grid.passable_at(goal.0, goal.1) {
        return None;
    }

    // (f, x, y) with f = g + h; Reverse so the heap pops the smallest f.
    let mut open: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut best: HashMap<(u32, u32), u32> = HashMap::new();
    let mut came_from: HashMap<(u32, u32), (u32, u32)> = HashMap::new();

    best.insert(start, 0);
    open.push(Reverse((manhattan(start, goal), start.0, start.1)));

    while let Some(Reverse((_, x, y))) = open.pop() {
        let pos = (x, y);
        if pos == goal {
            return Some(reconstruct(&came_from, goal));
        }
        let g = best[&pos];
        for (nx, ny) in neighbors(x, y) {
            if !grid.passable_at(nx, ny) {
                continue;
            }
            let tentative = g + cost_milli(grid, nx, ny);
            if tentative < *best.get(&(nx, ny)).unwrap_or(&u32::MAX) {
                best.insert((nx, ny), tentative);
                came_from.insert((nx, ny), pos);
                let f = tentative + manhattan((nx, ny), goal);
                open.push(Reverse((f, nx, ny)));
            }
        }
    }
    None
}

fn neighbors(x: u32, y: u32) -> [(u32, u32); 4] {
    [
        (x + 1, y),
        (x.wrapping_sub(1), y),
        (x, y + 1),
        (x, y.wrapping_sub(1)),
    ]
}

fn reconstruct(came_from: &HashMap<(u32, u32), (u32, u32)>, goal: (u32, u32)) -> Vec<(u32, u32)> {
    let mut path = vec![goal];
    let mut cur = goal;
    while let Some(&prev) = came_from.get(&cur) {
        path.push(prev);
        cur = prev;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::{BiomeDef, BiomeGrid};
    use std::collections::BTreeMap;

    /// Build a grid from ASCII rows: '.' desert (cost 1), ',' floodplain (1.5),
    /// '~' water (impassable).
    fn grid_from_rows(rows: &[&str]) -> BiomeGrid {
        let defs = BTreeMap::from([
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
        ]);
        let legend = BTreeMap::from([
            ("3a6ea5".to_string(), "water".to_string()),
            ("d9b36c".to_string(), "desert".to_string()),
            ("8fae5c".to_string(), "floodplain".to_string()),
        ]);
        let colors: BTreeMap<char, (u8, u8, u8)> = BTreeMap::from([
            ('~', (0x3a, 0x6e, 0xa5)),
            ('.', (0xd9, 0xb3, 0x6c)),
            (',', (0x8f, 0xae, 0x5c)),
        ]);
        let width = rows[0].len() as u32;
        let height = rows.len() as u32;
        let mut px = Vec::new();
        for row in rows {
            assert_eq!(row.len() as u32, width, "ragged test grid");
            for c in row.chars() {
                let (r, g, b) = colors[&c];
                px.extend_from_slice(&[r, g, b, 255]);
            }
        }
        BiomeGrid::from_pixels(&px, width, height, &legend, &defs).unwrap()
    }

    #[test]
    fn straight_desert_path_is_the_manhattan_walk() {
        let grid = grid_from_rows(&["....", "....", "...."]);
        let path = find_path(&grid, (0, 0), (3, 2)).unwrap();
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(3, 2)));
        assert_eq!(path.len(), 6); // 5 moves + start
    }

    #[test]
    fn start_equals_goal() {
        let grid = grid_from_rows(&["....", "...."]);
        assert_eq!(find_path(&grid, (1, 1), (1, 1)), Some(vec![(1, 1)]));
    }

    #[test]
    fn path_goes_around_water() {
        // A vertical wall of water splits the map; the path must go around.
        let grid = grid_from_rows(&["..~..", "..~..", "..~..", "..~..", "....."]);
        let path = find_path(&grid, (0, 0), (4, 4)).unwrap();
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(4, 4)));
        for &(x, y) in &path {
            assert!(grid.passable_at(x, y), "path crosses water at ({x},{y})");
        }
    }

    #[test]
    fn floodplain_is_avoided_when_cheaper_route_exists() {
        // The direct row is floodplain (1.5); the detour over desert is cheaper.
        let grid = grid_from_rows(&["........", ",,,,,,,,", "........"]);
        let path = find_path(&grid, (0, 0), (7, 0)).unwrap();
        assert!(
            path.iter().all(|&(_x, y)| y != 1),
            "took the floodplain row"
        );
    }

    #[test]
    fn unreachable_goal_returns_none() {
        // Island of water in the middle; goal sealed off.
        let grid = grid_from_rows(&[".....", "..~..", ".~~~.", "..~..", "....."]);
        assert!(find_path(&grid, (0, 0), (2, 2)).is_none());
        assert!(find_path(&grid, (0, 0), (99, 99)).is_none());
    }

    #[test]
    fn impassable_goal_returns_none() {
        let grid = grid_from_rows(&["..~.."]);
        assert!(find_path(&grid, (0, 0), (2, 0)).is_none());
    }

    #[test]
    fn path_is_deterministic() {
        let grid = grid_from_rows(&["..~~..", "..~~..", "......", "..~~..", "..~~..", "......"]);
        let a = find_path(&grid, (0, 0), (5, 5)).unwrap();
        let b = find_path(&grid, (0, 0), (5, 5)).unwrap();
        assert_eq!(a, b);
    }
}
