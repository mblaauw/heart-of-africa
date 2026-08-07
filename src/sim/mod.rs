//! Simulation core: ALL game logic, headless-testable.
//!
//! Invariant (DESIGN.md §8.3): this module and `world/` must never import
//! `macroquad` or `egui`. World state at tick T is a pure function of
//! (seed, player inputs up to T) — no wall-clock time, no unseeded randomness.

pub mod clock;
pub mod clues;
pub mod economy;
pub mod events;
pub mod game;
pub mod player;
pub mod rivals;
pub mod survival;

mod talk;
mod travel;

use serde::{Deserialize, Serialize};

use crate::dialogue::npc::Effect;

use player::{Direction, EquipmentSlot};

/// A player input, recorded in application order for replay (DESIGN.md §9).
/// Replays must be byte-identical: a save plus its input log is the whole
/// game history.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Input {
    /// Walk one tile in a cardinal direction (1 tick = 1 hour).
    Move(Direction),
    /// Rest for one hour.
    Rest,
    /// Buy one unit of a good at the current settlement's market.
    Buy(String),
    /// Sell one unit of a good at the current settlement's market.
    Sell(String),
    /// Use one unit of a carried good (drink, eat, take quinine).
    Use(String),
    /// Speak with an NPC: `(npc_id, option_index)` where `option_index` is
    /// among the *currently visible* options (0 = first visible).
    Talk(String, usize),
    /// Bury one unit of a good at the current tile (DESIGN.md §4.10).
    Bury(String),
    /// Retrieve one unit of a good from a cache at the current tile.
    Retrieve(String),
    /// Caravan fast-travel along a scouted route (DESIGN.md §4.2).
    Travel(String),
    /// A validated LLM action (DESIGN.md §6.4): the narration skin may emit
    /// scripted-tree effects, recorded like any other input so replays stay
    /// byte-identical with the LLM on (§9). Re-validated on apply.
    LlmAction { npc_id: String, effect: Effect },
    /// Narrow the search radius for treasure (WS3).
    SearchArea,
    /// Excavate at current position for treasure (WS3).
    Excavate,
    /// Equip an item to a slot (WS4).
    Equip(EquipmentSlot, String),
    /// Unequip a slot (WS4).
    Unequip(EquipmentSlot),
    /// Embark on a river at current position (WS4).
    Embark,
    /// Disembark from river travel (WS4).
    Disembark,
    /// Pin a clue text to the map (WS5).
    PinClue(String),
    /// Remove a pin from the map (WS5).
    UnpinClue(String),
}
