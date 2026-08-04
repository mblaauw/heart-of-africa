//! Fixed-tick simulation clock (DESIGN.md §8.3, §12).
//!
//! 1 tick = 1 in-game hour. The clock is a pure counter: it contains no time
//! source, so advancing it is deterministic by construction.

// Implemented in Unit 2 (sim clock): `Tick(u64)` with advance/rollover tests.
