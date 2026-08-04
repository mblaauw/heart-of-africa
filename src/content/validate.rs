//! Content validation (DESIGN.md §9): schema checks and ID resolution across
//! all content. `cargo test` fails on any problem reported here — that is the
//! guardrail that lets content scale without reviewing every line.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use super::load;

/// Validate all content under `content_dir`.
///
/// Returns a list of problems; an empty list means the content is valid.
/// Schema errors and pixel-legend mismatches fail hard (they bubble up as
/// `Err`), while structural problems (bad IDs, off-map positions, rivers not
/// on water) are collected and returned.
pub fn validate_content(content_dir: &Path) -> Result<Vec<String>> {
    let world = load::load_world(content_dir)?;
    let mut problems: Vec<String> = Vec::new();

    if world.settlements.is_empty() {
        problems.push("no settlements defined".to_string());
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for s in &world.settlements {
        if !seen.insert(s.id.as_str()) {
            problems.push(format!("duplicate settlement id {:?}", s.id));
        }
        if !world.grid.in_bounds(s.x, s.y) {
            problems.push(format!(
                "settlement {:?} at ({}, {}) is outside the map",
                s.id, s.x, s.y
            ));
        }
    }

    if world.start().is_none() {
        problems.push(format!(
            "region start_settlement {:?} does not resolve to a settlement",
            world.start_settlement
        ));
    }

    let mut river_ids: HashSet<&str> = HashSet::new();
    for r in &world.rivers {
        if !river_ids.insert(r.id.as_str()) {
            problems.push(format!("duplicate river id {:?}", r.id));
        }
        if r.points.is_empty() {
            problems.push(format!("river {:?} has no points", r.id));
        }
        for &(x, y) in &r.points {
            if !world.grid.in_bounds(x, y) {
                problems.push(format!(
                    "river {:?} point ({x},{y}) is outside the map",
                    r.id
                ));
            } else if world.grid.passable_at(x, y) {
                problems.push(format!(
                    "river {:?} point ({x},{y}) is not on a water tile",
                    r.id
                ));
            }
        }
    }

    Ok(problems)
}

/// Validate the art manifest (DESIGN.md §5).
///
/// Manifest schema errors fail hard; any file that EXISTS must load as an
/// image. Missing files are allowed by design — placeholder art covers them.
pub fn validate_assets(assets_dir: &Path) -> Result<Vec<String>> {
    let manifest = load::load_assets(assets_dir)?;
    let mut problems: Vec<String> = Vec::new();
    for (id, path) in &manifest.entries {
        if !path.exists() {
            continue; // optional by design; placeholder art replaces it
        }
        if let Err(e) = image::open(path) {
            problems.push(format!(
                "asset {id:?} at {} is not a loadable image: {e}",
                path.display()
            ));
        }
    }
    Ok(problems)
}
