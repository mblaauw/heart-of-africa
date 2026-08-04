//! The clue chain (DESIGN.md §4.5).
//!
//! Phase 2 authors one deterministic chain in data (hint → key →
//! corroboration → location); the seeded clue-graph generator with the
//! solvability proof replaces this in Phase 3. Node types follow the design:
//! location hints, corroborations, keys, and (in Phase 3) red herrings.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Clue node kinds (DESIGN.md §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClueKind {
    /// Points toward the next location.
    Hint,
    /// Yields a key item needed elsewhere.
    Key,
    /// Corroborates the trail; needed before the location is trusted.
    Corroboration,
    /// Names the tomb site.
    Location,
}

/// One clue node: gated by reputation and prerequisites, revealed by a
/// specific NPC, optionally granting or requiring an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clue {
    pub id: String,
    pub kind: ClueKind,
    /// Settlement where the clue is revealed.
    pub settlement: String,
    /// NPC who reveals it.
    pub npc: String,
    /// Reputation required in the settlement before the clue is offered.
    #[serde(default)]
    pub rep_gate: i32,
    /// Clue IDs that must be collected first.
    #[serde(default)]
    pub prereq: Vec<String>,
    /// Item granted when the clue is revealed.
    #[serde(default)]
    pub gives_item: Option<String>,
    /// Item the player must carry to receive the clue.
    #[serde(default)]
    pub requires_item: Option<String>,
    /// The in-fiction text of the clue (the diary entry).
    pub text: String,
}

/// The expedition's win condition: the tomb tile and how many clues are
/// needed to name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expedition {
    pub tomb_tile: (u32, u32),
    /// Clues required to identify the tomb (DESIGN.md §4.5: N corroborating
    /// clues; terrain search without them is astronomically unlikely).
    pub win_clues: usize,
}

impl Clue {
    /// Whether `obtained` satisfies the prerequisites of this clue.
    pub fn prereqs_met(&self, obtained: &BTreeSet<String>) -> bool {
        self.prereq.iter().all(|p| obtained.contains(p))
    }
}
