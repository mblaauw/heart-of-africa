//! Shared test helpers (not a test file: module only, included by others).

use std::path::Path;

/// The repo's content directory, resolved from the crate root so tests work
/// regardless of the working directory.
pub fn content_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/content"))
}
