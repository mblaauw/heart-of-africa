//! Macroquad rendering: camera, tiles, sprites, HUD (DESIGN.md §8.2).
//!
//! This module may import `macroquad` freely; it must never feed state back
//! into `sim`/`world` except through validated player inputs.

pub mod map;
