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
  hoatool new-npc <content-dir> <id> <name> <settlement> [--gift coffee|beads]
  hoatool llm-demo        (needs --features llm; live garnish check)
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-map") => run_gen_map(&args[2..]),
        Some("gen-art") => run_gen_art(&args[2..]),
        Some("validate") => run_validate(&args[2..]),
        Some("new-npc") => run_new_npc(&args[2..]),
        #[cfg(feature = "llm")]
        Some("llm-demo") => run_llm_demo(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

/// Scaffold a new named NPC from the standard template: a complete dialogue
/// tree (greet / gift_ack / tomb_answer), the synthesized `gift` field, and
/// the farewell option — so future content follows the template instead of
/// copy-paste.
fn run_new_npc(args: &[String]) {
    let (content_dir, id, name, settlement) = match args.first() {
        Some(dir) => {
            let id = args.get(1).map(String::as_str).unwrap_or("");
            let name = args.get(2).map(String::as_str).unwrap_or("");
            let settlement = args.get(3).map(String::as_str).unwrap_or("");
            if id.is_empty() || name.is_empty() || settlement.is_empty() {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
            (dir, id, name, settlement)
        }
        None => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let gift = match args.iter().position(|a| a == "--gift") {
        Some(i) => args.get(i + 1).map(String::as_str).unwrap_or("coffee"),
        None => "coffee",
    };
    if let Err(e) = new_npc(Path::new(content_dir), id, name, settlement, gift) {
        eprintln!("hoatool new-npc: error: {e:#}");
        std::process::exit(1);
    }
    println!("new-npc: wrote content/npcs/{id}.toml");
}

fn new_npc(content_dir: &Path, id: &str, name: &str, settlement: &str, gift: &str) -> Result<()> {
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        bail!("NPC id must be lowercase letters, digits, or underscores");
    }
    let world = heart_of_africa::content::load::load_world(content_dir)
        .context("loading world (for settlement check)")?;
    if world.settlement(settlement).is_none() {
        bail!("settlement {settlement:?} is not in world.toml");
    }
    let content = heart_of_africa::content::load::load_game_content(content_dir)
        .context("loading game content (for gift check)")?;
    if !content.goods.contains_key(gift) {
        bail!("gift item {gift:?} is not a good");
    }
    let rep_delta = if gift == "beads" { 5 } else { 3 };
    let settlement_name = world
        .settlement(settlement)
        .map(|s| s.name.as_str())
        .unwrap_or(settlement);

    let out = content_dir.join("npcs").join(format!("{id}.toml"));
    if out.exists() {
        bail!("{} already exists", out.display());
    }
    let text = format!(
        "# {settlement}'s named NPC. The tomb question is a runtime slot filled\n\
         # by the seeded clue graph.\n\
         \n\
         id = \"{id}\"\n\
         name = \"{name}\"\n\
         settlement = \"{settlement}\"\n\
         opening = \"greet\"\n\
         tomb_answer = \"tomb_answer\"\n\
         gift = {{ item = \"{gift}\", rep_delta = {rep_delta} }}\n\
         \n\
         [[node]]\n\
         id = \"greet\"\n\
         text = \"{name} studies the traveler with patient eyes. 'A traveler here — what brings you to {settlement_name}?'\"\n\
         \n\
         [[node.option]]\n\
         text = \"Farewell.\"\n\
         effect = {{ end = true }}\n\
         \n\
         [[node]]\n\
         id = \"gift_ack\"\n\
         text = \"'A thoughtful gift. You are remembered kindly.'\"\n\
         \n\
         [[node.option]]\n\
         text = \"Farewell.\"\n\
         effect = {{ end = true }}\n\
         \n\
         [[node]]\n\
         id = \"tomb_answer\"\n\
         text = \"{name} nods slowly. 'The old kings are a guarded subject. But you have earned the telling — hear it, and mark it well.'\"\n\
         \n\
         [[node.option]]\n\
         text = \"Farewell.\"\n\
         effect = {{ end = true }}\n",
        settlement = settlement,
        id = id,
        name = name,
        gift = gift,
        rep_delta = rep_delta,
        settlement_name = settlement_name,
    );
    std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Live verification of the LLM garnish (DESIGN.md §6): one real request to
/// the OpenCode API through the full validator, using the repo's content.
/// Needs `--features llm` and `HOA_OPENCODE_API_KEY`.
#[cfg(feature = "llm")]
fn run_llm_demo() {
    use heart_of_africa::content::load::{load_game_content, load_world};
    use heart_of_africa::dialogue::llm::{self, LlmContext, LlmGate};
    use heart_of_africa::sim::game::Game;

    let config = llm::LlmConfig::load();
    if !config.enabled {
        eprintln!("llm-demo: config.toml has llm.enabled = false — enable it first");
        std::process::exit(1);
    }
    let Some(key) = llm::LlmConfig::api_key() else {
        eprintln!("llm-demo: HOA_OPENCODE_API_KEY is not set");
        std::process::exit(1);
    };

    let world = match load_world(Path::new("content")) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("llm-demo: {e:#}");
            std::process::exit(1);
        }
    };
    let content = match load_game_content(Path::new("content")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("llm-demo: {e:#}");
            std::process::exit(1);
        }
    };
    let game = Game::new(0, content, &world);
    let npc = game
        .content
        .npcs
        .get("cairo_hakim")
        .expect("cairo NPC exists");
    let start = game.graph.start_clue.clone();
    let clue = game.graph.clues.get(&start).expect("start clue exists");
    let ctx = LlmContext {
        npc_id: npc.id.clone(),
        npc_name: npc.name.clone(),
        settlement: npc.settlement.clone(),
        reputation: 5,
        node_text: npc.opening().text.clone(),
        options: vec!["Farewell.".to_string()],
        pending_clue: Some((clue.id.clone(), clue.text.clone())),
    };
    let gates = LlmGate {
        pending_clue: Some((clue.id.clone(), clue.text.clone())),
        all_clue_texts: game.graph.clues.values().map(|c| c.text.clone()).collect(),
    };

    println!(
        "llm-demo: {} via {} (timeout {}ms)...",
        config.model, config.provider, config.timeout_ms
    );
    let body = llm::build_request(&config, &ctx);
    match llm::request(&config, &key, &body) {
        Ok(raw) => {
            let Some(content) = llm::parse_response(&raw) else {
                eprintln!("llm-demo: response envelope had no content (fail closed):\n{raw}");
                std::process::exit(1);
            };
            match llm::validate(&content, &gates) {
                Some(reply) => {
                    println!("validated reply:");
                    println!("  say: {}", reply.say);
                    println!("  actions: {:?}", reply.actions);
                    println!("llm-demo: OK — the garnish answers through the validator");
                }
                None => {
                    eprintln!("llm-demo: reply failed validation (fail closed):\n{content}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("llm-demo: request failed: {e}");
            std::process::exit(1);
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
