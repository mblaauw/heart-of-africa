//! Player state and movement (DESIGN.md §4.2).
//!
//! Movement applies terrain cost from `world`; water is impassable except at
//! settlement crossings. The visited set is the fog-of-war record.

// Implemented in Unit 4 (player movement).
