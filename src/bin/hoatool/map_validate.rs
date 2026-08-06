//! `hoatool map validate <content-dir>` — validate geography.toml
//! against the biome grid: settlements on passable tiles, rivers on
//! water, route endpoints resolve, regions and landmarks are valid,
//! and the sim extent matches the visual extent.

use std::path::Path;

use anyhow::{Result, bail};

/// Run `hoatool map validate <content-dir>`.
pub fn run_map_validate(args: &[String]) -> Result<()> {
    let content_dir = match args.first() {
        Some(d) => d,
        None => bail!("usage: hoatool map validate <content-dir>"),
    };
    let problems = validate_geography(Path::new(content_dir))?;
    if problems.is_empty() {
        println!("map validate: geography.toml OK");
        Ok(())
    } else {
        for p in &problems {
            eprintln!("map validate: {p}");
        }
        bail!("map validate: {} problem(s)", problems.len());
    }
}

/// Validate geography.toml against the biome grid and world.toml.
fn validate_geography(content_dir: &Path) -> Result<Vec<String>> {
    let world = heart_of_africa::content::load::load_world(content_dir)?;
    let mut problems = Vec::new();

    // Settlements must be on passable tiles.
    for s in &world.settlements {
        if !world.grid.passable_at(s.x, s.y) {
            problems.push(format!(
                "settlement {:?} at ({}, {}) is on an impassable tile",
                s.id, s.x, s.y
            ));
        }
    }

    // Rivers must have points on water tiles.
    for r in &world.rivers {
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

    // Routes: endpoints resolve to settlements; tiles are passable.
    for route in &world.routes {
        if world.settlement(&route.from).is_none() {
            problems.push(format!(
                "route {:?} from unknown settlement {:?}",
                route.id, route.from
            ));
        }
        if world.settlement(&route.to).is_none() {
            problems.push(format!(
                "route {:?} to unknown settlement {:?}",
                route.id, route.to
            ));
        }
        if route.from == route.to {
            problems.push(format!(
                "route {:?} connects a settlement to itself",
                route.id
            ));
        }
        for &(x, y) in &route.tiles {
            if !world.grid.in_bounds(x, y) {
                problems.push(format!(
                    "route {:?} tile ({x},{y}) is outside the map",
                    route.id
                ));
            } else if !world.grid.passable_at(x, y) {
                problems.push(format!(
                    "route {:?} tile ({x},{y}) is on an impassable tile",
                    route.id
                ));
            }
        }
    }

    // Regions: polygons must have at least 3 points and be closed
    // (first == last).
    for region in &world.regions {
        if region.polygon.ring.len() < 3 {
            problems.push(format!("region {:?} has fewer than 3 points", region.id));
        }
        let closed = region
            .polygon
            .ring
            .first()
            .zip(region.polygon.ring.last())
            .map(|(first, last)| {
                (first.x - last.x).abs() <= 1e-9 && (first.y - last.y).abs() <= 1e-9
            })
            .unwrap_or(false);
        if !closed {
            problems.push(format!(
                "region {:?} polygon is not closed (first != last)",
                region.id
            ));
        }
    }

    // Landmarks: points must be within the world bounds.
    for lm in &world.landmarks {
        let (tx, ty) = world_to_tile(lm.point);
        if tx >= world.width || ty >= world.height {
            problems.push(format!(
                "landmark {:?} at km ({:.1}, {:.1}) maps to tile ({}, {}) \
                 outside the {}×{} map",
                lm.id, lm.point.x, lm.point.y, tx, ty, world.width, world.height
            ));
        }
    }

    // Sim extent vs visual extent: the world dimensions from the
    // biome PNG must match the projection implied by the geography.
    // The sim grid is width×height tiles; the visual extent is
    // width*KM_PER_CELL × height*KM_PER_CELL km.
    let expected_km_width = world.width as f64 * heart_of_africa::world::KM_PER_CELL;
    let expected_km_height = world.height as f64 * heart_of_africa::world::KM_PER_CELL;
    for region in &world.regions {
        for point in &region.polygon.ring {
            if point.x < 0.0
                || point.x > expected_km_width
                || point.y < 0.0
                || point.y > expected_km_height
            {
                problems.push(format!(
                    "region {:?} polygon point ({:.1}, {:.1}) is outside \
                     the sim extent (0..{:.1}, 0..{:.1}) km",
                    region.id, point.x, point.y, expected_km_width, expected_km_height
                ));
            }
        }
    }
    for lm in &world.landmarks {
        if lm.point.x < 0.0
            || lm.point.x > expected_km_width
            || lm.point.y < 0.0
            || lm.point.y > expected_km_height
        {
            problems.push(format!(
                "landmark {:?} at km ({:.1}, {:.1}) is outside the sim extent",
                lm.id, lm.point.x, lm.point.y
            ));
        }
    }

    Ok(problems)
}

/// Convert a world-km point to a sim tile coordinate.
fn world_to_tile(p: heart_of_africa::world::geo::KmPoint) -> (u32, u32) {
    let x = (p.x / heart_of_africa::world::KM_PER_CELL).floor().max(0.0) as u32;
    let y = (p.y / heart_of_africa::world::KM_PER_CELL).floor().max(0.0) as u32;
    (x, y)
}
