//! Rival explorers (DESIGN.md §4.8), under simplified Phase 3 rules.
//!
//! Positions are a pure function of (seed, day): each rival circuits the
//! graph settlements on a fixed off-screen schedule. On arrival they may
//! consume an exclusive clue — but never in a way that breaks solvability
//! (checked against the graph's proof), so a rival can force the player
//! onto the other leg, never into a dead end.

/// Days between rival hops between settlements.
pub const HOP_DAYS: u64 = 3;

/// Rivals only consume exclusive clues on arrival days at or after this day.
/// Earlier arrivals just spread rumors, giving the player a real chance to
/// race for a leg on foot (DESIGN.md §4.8 "raced", §9 budgeted solvability):
/// a clue a player cannot physically reach before the rival takes it is not
/// a race, it's a lock.
pub const CONSUME_GRACE_DAY: u64 = 9;

/// The rival's current settlement: one hop around the circuit every
/// `HOP_DAYS` days, starting staggered by index.
pub fn position(day: u64, rival_index: usize, order: &[String]) -> &str {
    let slot = (rival_index as u64 + day / HOP_DAYS) % order.len() as u64;
    &order[slot as usize]
}

/// Whether the rival reaches a new settlement today (day 0 is a start, not
/// an arrival).
pub fn arrives_today(day: u64) -> bool {
    day > 0 && day.is_multiple_of(HOP_DAYS)
}
