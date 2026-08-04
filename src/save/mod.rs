//! Save/load: serde_json snapshots (DESIGN.md §4.10, §8.2).
//!
//! A save contains the world seed, the full game state, and the ordered
//! input log, so a save is fully reproducible and replayable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sim::Input;
use crate::sim::clock::Tick;
use crate::sim::game::{Cache, GameStatus};
use crate::sim::player::Player;
use crate::sim::survival::Survival;

/// A complete, reproducible snapshot of a run (DESIGN.md §4.10).
///
/// Invariant: replaying `input_log` from `(seed, tick 0)` reproduces this
/// exact state; `state_hash` is a pure function of the serialized content,
/// so equal hashes mean identical runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Save {
    /// World generation seed. Consumed by worldgen in Phase 3; recorded from
    /// Phase 1 for replay compatibility.
    pub seed: u64,
    pub tick: Tick,
    pub player: Player,
    pub survival: Survival,
    /// Settlement ID -> reputation.
    pub reputation: BTreeMap<String, i32>,
    /// Obtained clue IDs.
    pub clues: BTreeSet<String>,
    pub caches: Vec<Cache>,
    /// Tomb tile once the location clue names it.
    pub tomb_revealed: Option<(u32, u32)>,
    pub status: GameStatus,
    /// Ordered player inputs, in application order (the replay log).
    pub input_log: Vec<Input>,
}

impl Save {
    /// Deterministic FNV-1a hash over the canonical JSON of this save.
    pub fn state_hash(&self) -> u64 {
        let json = serde_json::to_vec(self).expect("save always serializes");
        let mut hash: u64 = 0xcbf29ce484222325;
        for &b in &json {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}

/// Serialize a save to pretty JSON.
pub fn to_json(save: &Save) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(save)?)
}

/// Deserialize a save from JSON.
pub fn from_json(text: &str) -> anyhow::Result<Save> {
    Ok(serde_json::from_str(text)?)
}

/// Write a save to a file (save-anywhere, DESIGN.md §4.10).
pub fn write_save(path: &Path, save: &Save) -> anyhow::Result<()> {
    std::fs::write(path, to_json(save)?)?;
    Ok(())
}

/// Read a save from a file.
pub fn read_save(path: &Path) -> anyhow::Result<Save> {
    from_json(&std::fs::read_to_string(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::player::Direction;

    fn sample() -> Save {
        Save {
            seed: 42,
            tick: Tick::from_hours(7),
            player: Player::new(15, 8),
            survival: Survival::new(),
            reputation: BTreeMap::new(),
            clues: BTreeSet::new(),
            caches: Vec::new(),
            tomb_revealed: None,
            status: GameStatus::Playing,
            input_log: vec![Input::Move(Direction::South), Input::Move(Direction::East)],
        }
    }

    #[test]
    fn round_trip_preserves_state() {
        let save = sample();
        let json = to_json(&save).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back, save);
    }

    #[test]
    fn round_trip_preserves_hash() {
        let save = sample();
        let back = from_json(&to_json(&save).unwrap()).unwrap();
        assert_eq!(back.state_hash(), save.state_hash());
    }

    #[test]
    fn hash_changes_with_state() {
        let mut a = sample();
        let b = a.clone();
        a.tick.advance();
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn file_round_trip() {
        let path = std::env::temp_dir().join(format!("hoa-save-test-{}.json", std::process::id()));
        let save = sample();
        write_save(&path, &save).unwrap();
        let back = read_save(&path).unwrap();
        assert_eq!(back, save);
        let _ = std::fs::remove_file(&path);
    }
}
