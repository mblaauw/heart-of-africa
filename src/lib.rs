//! Heart of Africa remake — one crate, two binaries (DESIGN.md §8.2).
//!
//! Invariants (non-negotiable, DESIGN.md §8.3):
//! - `sim/` and `world/` NEVER import `macroquad` or `egui`. All game logic
//!   must run headless in `cargo test`.
//! - `content/` loads and validates all TOML/PNG data; a typo in a content
//!   file fails CI by design (§9).
//! - `save/` snapshots/restores game state as JSON, fully reproducible.

pub mod content;
pub mod save;
pub mod sim;
pub mod world;

// Render and UI are pulled in only by the game binary, not the lib core:
// `sim`/`world` must stay macroquad-free even if these exist.
pub mod render;
pub mod ui;
