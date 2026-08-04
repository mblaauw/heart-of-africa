//! Shared test helpers (not a test file: module only, included by others).
//! Each integration-test binary compiles this module, so unused helpers
//! there are expected; hence the allow.

#![allow(dead_code)]

use std::path::Path;

/// The repo's content directory, resolved from the crate root so tests work
/// regardless of the working directory.
pub fn content_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/content"))
}

/// The repo's assets directory (the manifest lives here).
pub fn assets_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"))
}
