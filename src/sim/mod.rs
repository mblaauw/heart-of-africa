//! Simulation core: ALL game logic, headless-testable.
//!
//! Invariant (DESIGN.md §8.3): this module and `world/` must never import
//! `macroquad` or `egui`. World state at tick T is a pure function of
//! (seed, player inputs up to T) — no wall-clock time, no unseeded randomness.

pub mod clock;
pub mod player;

use serde::{Deserialize, Serialize};

use player::Direction;

/// A player input, recorded in application order for replay (DESIGN.md §9).
/// Replays must be byte-identical: a save plus its input log is the whole
/// game history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Input {
    Move(Direction),
}
