//! Dev tool binary (DESIGN.md §8.2): content validation and asset pipeline.
//!
//! Subcommands:
//! - `gen-map <world.toml> <ascii-spec.txt> <out.png>` — render an ASCII
//!   biome map into the biome PNG. The ASCII file is the diffable source of
//!   truth; the PNG is derived data.
//! - `gen-art <world.toml> <assets-dir> [seed]` — generate the procedural
//!   style-bible tileset, marker art, and ambient audio (deterministic).
//! - `validate <content-dir>` — run content lint (same checks as `cargo test`).

mod art;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

const USAGE: &str = "\
hoatool — Heart of Africa dev tool

usage:
  hoatool gen-map <world.toml> <ascii-spec.txt> <out.png>
  hoatool gen-art <world.toml> <assets-dir> [seed]
  hoatool validate <content-dir> [assets-dir]
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-map") => run_gen_map(&args[2..]),
        Some("gen-art") => run_gen_art(&args[2..]),
        Some("validate") => run_validate(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn run_gen_art(args: &[String]) {
    if !(2..=3).contains(&args.len()) {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let world_toml = &args[0];
    let assets_dir = &args[1];
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Err(e) = art::generate(Path::new(world_toml), Path::new(assets_dir), seed) {
        eprintln!("hoatool gen-art: error: {e:#}");
        std::process::exit(1);
    }
    println!("gen-art: wrote tileset, markers, and ambient audio into {assets_dir} (seed {seed})");
}

fn run_gen_map(args: &[String]) {
    if args.len() != 3 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let world_toml = &args[0];
    let ascii_spec = &args[1];
    let out_png = &args[2];
    if let Err(e) = gen_map(
        Path::new(world_toml),
        Path::new(ascii_spec),
        Path::new(out_png),
    ) {
        eprintln!("hoatool gen-map: error: {e:#}");
        std::process::exit(1);
    }
    println!("gen-map: wrote {}", Path::new(out_png).display());
}

fn run_validate(args: &[String]) {
    let content_dir = match args.first() {
        Some(d) => d,
        None => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let mut failed = false;

    match heart_of_africa::content::validate::validate_content(Path::new(content_dir)) {
        Ok(problems) if problems.is_empty() => {
            println!("validate: content OK");
        }
        Ok(problems) => {
            failed = true;
            for p in &problems {
                eprintln!("validate: content problem: {p}");
            }
        }
        Err(e) => {
            failed = true;
            eprintln!("validate: content error: {e:#}");
        }
    }

    // Optional second argument: assets directory (validates the manifest).
    if let Some(assets_dir) = args.get(1) {
        match heart_of_africa::content::validate::validate_assets(Path::new(assets_dir)) {
            Ok(problems) if problems.is_empty() => {
                println!("validate: assets OK");
            }
            Ok(problems) => {
                failed = true;
                for p in &problems {
                    eprintln!("validate: asset problem: {p}");
                }
            }
            Err(e) => {
                failed = true;
                eprintln!("validate: assets error: {e:#}");
            }
        }
    }

    if failed {
        eprintln!("validate: FAILED");
        std::process::exit(1);
    }
}

/// Read the biome color legend out of world.toml: biome ID -> (r, g, b).
fn biome_colors(world_toml: &Path) -> Result<BTreeMap<String, (u8, u8, u8)>> {
    #[derive(serde::Deserialize)]
    struct LegendToml {
        map: LegendMap,
    }
    #[derive(serde::Deserialize)]
    struct LegendMap {
        legend: BTreeMap<String, String>,
    }

    let text = std::fs::read_to_string(world_toml)
        .with_context(|| format!("reading {}", world_toml.display()))?;
    let raw: LegendToml = toml::from_str(&text).context("parsing world.toml")?;
    raw.map
        .legend
        .into_iter()
        .map(|(hex, id)| Ok((id, heart_of_africa::world::biome::parse_hex_color(&hex)?)))
        .collect()
}

/// Parse an ASCII biome spec:
///
/// ```text
/// # comment
/// ~ = water
/// . = desert
/// ---
/// <rows of one char per tile, all equal length>
/// ```
fn parse_spec(text: &str) -> Result<(BTreeMap<char, String>, Vec<String>)> {
    let mut char_to_biome = BTreeMap::new();
    let mut rows = Vec::new();
    let mut in_rows = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "---" {
            in_rows = true;
            continue;
        }
        if in_rows {
            rows.push(line.to_string());
            continue;
        }
        let (ch, id) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("bad legend line: {line:?}"))?;
        let mut chars = ch.trim().chars();
        let c = chars
            .next()
            .ok_or_else(|| anyhow!("empty legend char in {line:?}"))?;
        if chars.next().is_some() {
            bail!("legend char must be a single character, got {ch:?}");
        }
        if char_to_biome.insert(c, id.trim().to_string()).is_some() {
            bail!("duplicate legend char {c:?}");
        }
    }

    if char_to_biome.is_empty() {
        bail!("no legend entries found");
    }
    let width = rows.first().map(String::len).unwrap_or(0);
    if width == 0 {
        bail!("no map rows found");
    }
    for row in &rows {
        if row.len() != width {
            bail!("ragged rows: expected width {width}, got {}", row.len());
        }
    }
    Ok((char_to_biome, rows))
}

fn gen_map(world_toml: &Path, ascii_spec: &Path, out_png: &Path) -> Result<()> {
    let colors = biome_colors(world_toml)?;
    let text = std::fs::read_to_string(ascii_spec)
        .with_context(|| format!("reading {}", ascii_spec.display()))?;
    let (char_to_biome, rows) = parse_spec(&text)?;

    let height = rows.len() as u32;
    let width = rows[0].len() as u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in &rows {
        for c in row.chars() {
            let biome = char_to_biome
                .get(&c)
                .ok_or_else(|| anyhow!("map row uses char {c:?} which has no legend entry"))?;
            let (r, g, b) = colors.get(biome).ok_or_else(|| {
                anyhow!("biome {biome:?} from the ASCII legend has no color in {world_toml:?}")
            })?;
            rgba.extend_from_slice(&[*r, *g, *b, 255]);
        }
    }

    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| anyhow!("failed to build {width}x{height} image"))?;
    img.save(out_png)
        .with_context(|| format!("writing {}", out_png.display()))?;
    Ok(())
}
