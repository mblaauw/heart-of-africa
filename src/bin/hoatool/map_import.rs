//! `hoatool map import` — parse a Tiled JSON export (TMJ) and produce
//! committed geography.toml + a manifest.toml (the LOD pyramid is generated
//! by `hoatool gen-map`, not by import).
//!
//! The parser itself lives in the library (`content::import`) so the
//! integration suite can exercise it; this file is the thin CLI over it.

use std::path::Path;

use anyhow::{Context, Result, bail};

use heart_of_africa::content::import::import_tiled;
use heart_of_africa::content::load::{GEOGRAPHY_FILE, load_world};

/// Run `hoatool map import <world.toml> <tiled-project.tsx> <out-dir>`.
pub fn run_map_import(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        bail!("usage: hoatool map import <world.toml> <tiled-project.tsx> <out-dir>");
    }
    let world_toml = Path::new(&args[0]);
    let tsx_path = Path::new(&args[1]);
    let out_dir = Path::new(&args[2]);

    let world = load_world(
        world_toml
            .parent()
            .context("world.toml must have a parent directory")?,
    )
    .context("loading world for biome grid")?;

    let (geography, manifest) =
        import_tiled(tsx_path, &world).context("importing Tiled project")?;

    std::fs::create_dir_all(out_dir).context("creating output directory")?;

    let geo_path = out_dir.join(GEOGRAPHY_FILE);
    let geo_text = toml::to_string_pretty(&geography).context("serializing geography.toml")?;
    std::fs::write(&geo_path, geo_text)
        .with_context(|| format!("writing {}", geo_path.display()))?;

    let manifest_path = out_dir.join("manifest.toml");
    let manifest_text = toml::to_string_pretty(&manifest).context("serializing manifest.toml")?;
    std::fs::write(&manifest_path, manifest_text)
        .with_context(|| format!("writing {}", manifest_path.display()))?;

    println!(
        "map import: wrote {} + manifest.toml into {}",
        GEOGRAPHY_FILE,
        out_dir.display()
    );
    Ok(())
}
