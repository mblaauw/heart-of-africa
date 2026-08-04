//! The event director (DESIGN.md §4.8): the off-screen world ticks daily.
//!
//! Everything is a pure function of `(seed, day)` — weather, market drift,
//! and encounter rolls are re-derived from a stateless seeded stream, so no
//! RNG state ever enters the save and replays stay byte-identical.

use rand::RngExt;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

/// Daily weather state affecting travel (DESIGN.md §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weather {
    Clear,
    /// Desert travel costs extra hydration.
    Heatwave,
    /// All travel costs extra rest.
    Duststorm,
}

impl Weather {
    pub fn label(self) -> &'static str {
        match self {
            Weather::Clear => "clear",
            Weather::Heatwave => "heatwave",
            Weather::Duststorm => "duststorm",
        }
    }
}

/// A stateless per-(seed, day, key) RNG stream: deterministic, replayable.
fn stream(seed: u64, day: u64, key: &str) -> ChaCha20Rng {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8..16].copy_from_slice(&day.to_le_bytes());
    // Fold the key into the stream so per-item draws are independent.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in key.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    bytes[16..24].copy_from_slice(&hash.to_le_bytes());
    ChaCha20Rng::from_seed(bytes)
}

/// Weather of a given day.
pub fn weather(seed: u64, day: u64) -> Weather {
    match stream(seed, day, "weather").random_range(0..10u32) {
        0..=1 => Weather::Heatwave,
        2 => Weather::Duststorm,
        _ => Weather::Clear,
    }
}

/// Daily market drift: a factor multiplied onto a base supply/demand factor
/// (DESIGN.md §4.4: "updated on its own tick"; drift is the tick).
pub fn drifted(seed: u64, day: u64, item_id: &str, base: f32) -> f32 {
    let mut rng = stream(seed, day, &format!("drift:{item_id}"));
    let delta = rng.random_range(0.85f32..1.15);
    let drifted = base * delta;
    // Keep factors within the validated band.
    drifted.clamp(0.25, 4.0)
}

/// An encounter roll for risky travel (DESIGN.md §4.2: telegraphed risk).
/// Returns true when the caravan is waylaid.
pub fn encounter(seed: u64, tick: u64) -> bool {
    stream(seed, tick, "encounter").random_range(0..4u32) == 0
}
