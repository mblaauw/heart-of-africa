//! Simulation core: ALL game logic, headless-testable.
//!
//! Invariant (DESIGN.md §8.3): this module and `world/` must never import
//! `macroquad` or `egui`. World state at tick T is a pure function of
//! (seed, player inputs up to T) — no wall-clock time, no unseeded randomness.

pub mod clock;
pub mod player;
