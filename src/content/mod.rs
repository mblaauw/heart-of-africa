//! TOML/PNG content loaders + validation (DESIGN.md §7, §9).
//!
//! Every referenced ID must resolve; `cargo test` fails on a typo in a content
//! file — that is the intended guardrail.

pub mod import;
pub mod load;
pub mod validate;
