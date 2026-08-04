//! Player state and movement (DESIGN.md §4.2).
//!
//! Movement applies the destination tile's biome cost; water is impassable.
//! The visited list is the fog-of-war record: unique tiles in first-visit
//! order, deterministic by construction.

use serde::{Deserialize, Serialize};

use crate::world::biome::BiomeGrid;

/// Cardinal movement directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    /// Tile-space delta (dx, dy), y growing southward.
    pub fn delta(self) -> (i64, i64) {
        match self {
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::East => (1, 0),
            Direction::West => (-1, 0),
        }
    }

    /// The direction from tile `from` to adjacent tile `to`, if they touch
    /// orthogonally. Used to turn A* paths into movement inputs.
    pub fn between(from: (u32, u32), to: (u32, u32)) -> Option<Direction> {
        let dx = to.0 as i64 - from.0 as i64;
        let dy = to.1 as i64 - from.1 as i64;
        match (dx, dy) {
            (1, 0) => Some(Direction::East),
            (-1, 0) => Some(Direction::West),
            (0, 1) => Some(Direction::South),
            (0, -1) => Some(Direction::North),
            _ => None,
        }
    }
}

/// The expedition's position on the tile map.
///
/// Invariant: the position is always on a passable tile; `try_move` is the
/// only way to change it and rejects impassable and out-of-bounds targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    x: u32,
    y: u32,
    /// Tiles visited so far, unique, in first-visit order (fog of war).
    visited: Vec<(u32, u32)>,
}

impl Player {
    /// A player standing at `(x, y)`; the start tile counts as visited.
    pub fn new(x: u32, y: u32) -> Self {
        Player {
            x,
            y,
            visited: vec![(x, y)],
        }
    }

    pub fn pos(&self) -> (u32, u32) {
        (self.x, self.y)
    }

    /// All tiles visited so far, in first-visit order.
    pub fn visited(&self) -> &[(u32, u32)] {
        &self.visited
    }

    pub fn has_visited(&self, x: u32, y: u32) -> bool {
        self.visited.contains(&(x, y))
    }

    /// Try to move one tile in `dir`. Returns `None` (and changes nothing)
    /// when the target tile is out of bounds or impassable.
    pub fn try_move(&mut self, grid: &BiomeGrid, dir: Direction) -> Option<(u32, u32)> {
        let (dx, dy) = dir.delta();
        let nx = self.x as i64 + dx;
        let ny = self.y as i64 + dy;
        if nx < 0 || ny < 0 {
            return None;
        }
        let (nx, ny) = (nx as u32, ny as u32);
        if !grid.passable_at(nx, ny) {
            return None;
        }
        self.x = nx;
        self.y = ny;
        if !self.has_visited(nx, ny) {
            self.visited.push((nx, ny));
        }
        Some((nx, ny))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::biome::{BiomeDef, BiomeGrid};
    use std::collections::BTreeMap;

    fn grid() -> BiomeGrid {
        let defs = BTreeMap::from([
            (
                "water".to_string(),
                BiomeDef {
                    id: "water".into(),
                    passable: false,
                    movement_cost: 0.0,
                    color: (0x3a, 0x6e, 0xa5),
                },
            ),
            (
                "desert".to_string(),
                BiomeDef {
                    id: "desert".into(),
                    passable: true,
                    movement_cost: 1.0,
                    color: (0xd9, 0xb3, 0x6c),
                },
            ),
        ]);
        let legend = BTreeMap::from([
            ("3a6ea5".to_string(), "water".to_string()),
            ("d9b36c".to_string(), "desert".to_string()),
        ]);
        let mut px = Vec::new();
        for row in ["..~", "..~", "..."] {
            for c in row.chars() {
                let (r, g, b) = if c == '~' {
                    (0x3a, 0x6e, 0xa5)
                } else {
                    (0xd9, 0xb3, 0x6c)
                };
                px.extend_from_slice(&[r, g, b, 255]);
            }
        }
        BiomeGrid::from_pixels(&px, 3, 3, &legend, &defs).unwrap()
    }

    #[test]
    fn start_tile_is_visited() {
        let p = Player::new(1, 1);
        assert_eq!(p.pos(), (1, 1));
        assert!(p.has_visited(1, 1));
        assert_eq!(p.visited().len(), 1);
    }

    #[test]
    fn moves_onto_passable_tiles() {
        let grid = grid();
        let mut p = Player::new(0, 0);
        assert_eq!(p.try_move(&grid, Direction::East), Some((1, 0)));
        assert_eq!(p.pos(), (1, 0));
        assert_eq!(p.try_move(&grid, Direction::South), Some((1, 1)));
        assert_eq!(p.pos(), (1, 1));
    }

    #[test]
    fn water_and_map_edge_block_movement() {
        let grid = grid();
        let mut p = Player::new(0, 0);
        assert!(p.try_move(&grid, Direction::West).is_none()); // off map
        assert!(p.try_move(&grid, Direction::North).is_none()); // off map
        assert_eq!(p.try_move(&grid, Direction::East), Some((1, 0)));
        assert!(p.try_move(&grid, Direction::East).is_none()); // (2,0) is water
        assert_eq!(p.pos(), (1, 0)); // unchanged
    }

    #[test]
    fn visited_tracks_unique_tiles() {
        let grid = grid();
        let mut p = Player::new(1, 1);
        p.try_move(&grid, Direction::South); // (1,2)
        p.try_move(&grid, Direction::North); // back to (1,1)
        assert_eq!(p.visited().len(), 2);
        assert_eq!(p.visited(), &[(1, 1), (1, 2)]);
    }

    #[test]
    fn direction_between_adjacent_tiles() {
        assert_eq!(Direction::between((0, 0), (1, 0)), Some(Direction::East));
        assert_eq!(Direction::between((1, 0), (0, 0)), Some(Direction::West));
        assert_eq!(Direction::between((0, 0), (0, 1)), Some(Direction::South));
        assert_eq!(Direction::between((0, 1), (0, 0)), Some(Direction::North));
        assert_eq!(Direction::between((0, 0), (2, 0)), None);
        assert_eq!(Direction::between((0, 0), (1, 1)), None);
    }

    #[test]
    fn movement_is_deterministic() {
        let grid = grid();
        let mut a = Player::new(0, 0);
        let mut b = Player::new(0, 0);
        let moves = [
            Direction::East,
            Direction::South,
            Direction::South,
            Direction::East,
            Direction::West,
        ];
        for d in moves {
            a.try_move(&grid, d);
            b.try_move(&grid, d);
        }
        assert_eq!(a, b);
    }
}
