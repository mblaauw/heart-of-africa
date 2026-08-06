//! Save/load: serde_json snapshots (DESIGN.md §4.10, §8.2).
//!
//! A save contains the world seed, the full game state, and the ordered
//! input log, so a save is fully reproducible and replayable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sim::Input;
use crate::sim::clock::Tick;
use crate::sim::game::{Cache, DiaryEntry, GameStatus};
use crate::sim::player::Player;
use crate::sim::survival::Survival;
use crate::world::GiftPrefState;

/// Current save format version. Old versions are rejected with a clear
/// message (`from_json`); pre-release saves were explicitly invalidated when
/// versioning landed (expansion spec, Part III).
pub const SAVE_VERSION: u32 = 2;

/// A complete, reproducible snapshot of a run (DESIGN.md §4.10).
///
/// Invariant: replaying `input_log` from `(seed, tick 0)` reproduces this
/// exact state; `state_hash` is a pure function of the serialized content,
/// so equal hashes mean identical runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Save {
    /// Save format version; `#[serde(default)]` lets pre-version saves load
    /// as version 0 so they get the rejection message, not a parse error.
    #[serde(default)]
    pub version: u32,
    /// World seed. The clue graph is a pure function of it and is
    /// regenerated on load, so it is not stored here.
    pub seed: u64,
    pub tick: Tick,
    pub player: Player,
    pub survival: Survival,
    /// Settlement ID -> reputation.
    pub reputation: BTreeMap<String, i32>,
    /// Obtained clue IDs.
    pub clues: BTreeSet<String>,
    /// Herrings proven false (DESIGN.md §4.5).
    pub false_clues: BTreeSet<String>,
    /// Exclusive clues taken by rivals.
    pub rivals_consumed: BTreeSet<String>,
    /// The diary — the quest log and clue board (DESIGN.md §4.9). Entries
    /// are append-only in `apply`, so saving it stays deterministic.
    pub diary: Vec<DiaryEntry>,
    pub caches: Vec<Cache>,
    /// Tomb tile once the location clue names it.
    pub tomb_revealed: Option<(u32, u32)>,
    /// Explored sim cells as sector masks over the map tiles
    /// (WS1 fog of war). Each entry is a (sector_x, sector_y)
    /// pair; the sector size is a render-layer constant.
    pub explored_sectors: BTreeSet<(u32, u32)>,
    /// Labels learned from landmarks and regions; persists
    /// across saves and are re-interpreted by the diary (WS2).
    pub known_labels: BTreeSet<String>,
    /// Regional gift preference knowledge (WS2 social preferences).
    pub preferences: BTreeMap<String, GiftPrefState>,
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

/// Deserialize a save from JSON, rejecting unsupported versions.
pub fn from_json(text: &str) -> anyhow::Result<Save> {
    let save: Save = serde_json::from_str(text)?;
    if save.version != SAVE_VERSION {
        anyhow::bail!(
            "save version {} is not supported (expected {SAVE_VERSION}); \
             pre-release saves were invalidated when save versioning landed",
            save.version
        );
    }
    Ok(save)
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
            version: SAVE_VERSION,
            seed: 42,
            tick: Tick::from_hours(7),
            player: Player::new(15, 8),
            survival: Survival::new(),
            reputation: BTreeMap::new(),
            clues: BTreeSet::new(),
            false_clues: BTreeSet::new(),
            rivals_consumed: BTreeSet::new(),
            caches: Vec::new(),
            tomb_revealed: None,
            explored_sectors: BTreeSet::new(),
            known_labels: BTreeSet::new(),
            preferences: BTreeMap::new(),
            status: GameStatus::Playing,
            diary: Vec::new(),
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
    fn unsupported_versions_are_rejected() {
        let mut save = sample();
        save.version = 0; // as a pre-version save would load
        let json = to_json(&save).unwrap();
        let err = from_json(&json).unwrap_err().to_string();
        assert!(err.contains("save version 0"), "{err}");
        assert!(err.contains("expected 2"), "{err}");
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
