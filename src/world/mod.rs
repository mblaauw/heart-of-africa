//! World data: biome grid, pathfinding (DESIGN.md §8.2, §8.3).
//!
//! Invariant: this module must never import `macroquad` or `egui`.

pub mod biome;
pub mod pathfind;
