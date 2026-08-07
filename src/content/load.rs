//! Content loaders: world.toml, biome map, asset manifest, game content
//! (goods, markets, clues, NPCs) (DESIGN.md §7, §8.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::dialogue::npc::{Condition, DialogueOption, Effect, Gift, Node, Npc};
use crate::sim::clues::{ClueKind, ClueTemplate, GeneratorConfig, GeneratorInput};
use crate::sim::economy::{Category, Goods, Market, MarketEntry};
use crate::sim::game::GameContent;
use crate::sim::survival::Ailment;
use crate::world::biome::{BiomeDef, BiomeGrid, parse_hex_color};
use crate::world::{Place, RegionKnowledge, River, Route, Settlement, World};

/// Name of the world data file, at the content directory root.
pub const WORLD_FILE: &str = "world.toml";

/// Name of the goods catalog, at the content directory root.
pub const GOODS_FILE: &str = "goods.toml";

/// Name of the market factors file, at the content directory root.
pub const MARKETS_FILE: &str = "markets.toml";

/// Name of the clue-graph generator file, at the content directory root.
pub const CLUES_FILE: &str = "clues.toml";

/// Directory of per-NPC dialogue files, under the content directory.
pub const NPCS_DIR: &str = "npcs";

/// Name of the art manifest, at the assets directory root.
pub const ASSETS_FILE: &str = "assets.toml";

/// Current art manifest format version.
pub const ASSETS_VERSION: u32 = 1;

/// Current content format version shared by the game data files.
pub const CONTENT_VERSION: u32 = 1;

/// Current world data format version; `world.toml` must declare it.
pub const WORLD_VERSION: u32 = 1;

/// File name for the committed geography output (Tiled import result).
pub const GEOGRAPHY_FILE: &str = "geography.toml";

/// Current geography format version; `geography.toml` must declare it.
pub const GEOGRAPHY_VERSION: u32 = 1;

/// Directory of per-region knowledge files, under the content directory.
pub const REGIONS_DIR: &str = "regions";

#[derive(serde::Deserialize)]
struct WorldToml {
    version: u32,
    map: MapToml,
    biome: BTreeMap<String, BiomeToml>,
    region: RegionToml,
    #[serde(default)]
    settlements: Vec<SettlementToml>,
    #[serde(default)]
    rivers: Vec<RiverToml>,
    #[serde(default)]
    routes: Vec<RouteToml>,
}

#[derive(serde::Deserialize)]
struct GeographyToml {
    version: u32,
    #[serde(default)]
    settlements: Vec<SettlementToml>,
    #[serde(default)]
    rivers: Vec<RiverToml>,
    #[serde(default)]
    routes: Vec<RouteToml>,
    #[serde(default)]
    regions: Vec<GeographyRegionToml>,
    #[serde(default)]
    landmarks: Vec<GeographyLandmarkToml>,
}

#[derive(serde::Deserialize)]
struct MapToml {
    /// Path to the biome PNG, relative to the content directory.
    png: String,
    /// Normalized hex color (`"3a6ea5"`) -> biome ID.
    legend: BTreeMap<String, String>,
}

#[derive(serde::Deserialize)]
struct BiomeToml {
    passable: bool,
    movement_cost: f32,
}

#[derive(serde::Deserialize)]
struct RegionToml {
    /// Human-readable metadata for the region; parsed, not used in Phase 1.
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    start_settlement: String,
}

#[derive(serde::Deserialize)]
struct SettlementToml {
    id: String,
    name: String,
    polity: String,
    x: u32,
    y: u32,
    #[serde(default)]
    places: Vec<PlaceToml>,
}

#[derive(serde::Deserialize)]
struct PlaceToml {
    id: String,
    kind: String,
    name: String,
}

#[derive(serde::Deserialize)]
struct RiverToml {
    id: String,
    name: String,
    points: Vec<(u32, u32)>,
    #[serde(default)]
    length_km: f64,
    #[serde(default = "default_width_class")]
    width_class: String,
    #[serde(default)]
    navigable: bool,
    seasonal: Option<String>,
}

fn default_width_class() -> String {
    "normal".to_string()
}

#[derive(serde::Deserialize)]
struct RouteToml {
    id: String,
    from: String,
    to: String,
    #[serde(default)]
    risky: bool,
    #[serde(default)]
    path_km: f64,
    #[serde(default)]
    tiles: Vec<(u32, u32)>,
    #[serde(default)]
    modes: Vec<String>,
    #[serde(default)]
    seasonal: bool,
    #[serde(default)]
    blocked: bool,
}

#[derive(serde::Deserialize)]
struct GeographyRegionToml {
    id: String,
    polygon: Vec<(f64, f64)>,
}

#[derive(serde::Deserialize)]
struct GeographyLandmarkToml {
    id: String,
    #[serde(default)]
    name: String,
    kind: String,
    point: (f64, f64),
}

/// Load and parse the full world (world.toml + biome PNG) under
/// `content_dir`. Schema errors and unresolvable references fail loudly —
/// content lint is `cargo test` failing on a typo (§9).
///
/// Geography (settlements, rivers, routes) is read from the committed
/// `geography.toml` if present (Tiled import output); otherwise falls
/// back to the geography sections of `world.toml` (legacy path).
pub fn load_world(content_dir: &Path) -> Result<World> {
    let toml_text = std::fs::read_to_string(content_dir.join(WORLD_FILE))
        .with_context(|| format!("reading {}", content_dir.join(WORLD_FILE).display()))?;
    let raw: WorldToml =
        toml::from_str(&toml_text).with_context(|| format!("parsing {}", WORLD_FILE))?;
    if raw.version != WORLD_VERSION {
        anyhow::bail!(
            "{} declares version {}, expected {}",
            WORLD_FILE,
            raw.version,
            WORLD_VERSION
        );
    }

    // Legend colors: normalized hex -> biome ID. Inverted into biome -> RGB
    // so every BiomeDef carries its map color (used by the renderer).
    let mut color_of: BTreeMap<String, (u8, u8, u8)> = BTreeMap::new();
    for (hex, id) in &raw.map.legend {
        color_of.insert(id.clone(), parse_hex_color(hex)?);
    }

    let biome_defs: BTreeMap<String, BiomeDef> = raw
        .biome
        .iter()
        .map(|(id, b)| {
            let def = BiomeDef {
                id: id.clone(),
                passable: b.passable,
                movement_cost: b.movement_cost,
                color: *color_of
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("biome {id:?} has no legend color"))?,
            };
            validate_biome(&def)?;
            Ok((id.clone(), def))
        })
        .collect::<Result<_>>()?;

    let png_path = content_dir.join(&raw.map.png);
    let img = image::open(&png_path)
        .with_context(|| format!("opening biome map {}", png_path.display()))?
        .to_rgba8();
    let grid = BiomeGrid::from_pixels(
        img.as_raw(),
        img.width(),
        img.height(),
        &raw.map.legend,
        &biome_defs,
    )?;

    // Geography: prefer the committed import output (geography.toml) over
    // the legacy world.toml sections. This keeps the authored Tiled layers
    // as the single source of truth for positions and geometry.
    let geo_path = content_dir.join(GEOGRAPHY_FILE);
    let (settlements, rivers, routes, regions, landmarks) = if geo_path.exists() {
        let geo_text = std::fs::read_to_string(&geo_path)
            .with_context(|| format!("reading {}", geo_path.display()))?;
        let geo_raw: GeographyToml =
            toml::from_str(&geo_text).with_context(|| format!("parsing {}", GEOGRAPHY_FILE))?;
        if geo_raw.version != GEOGRAPHY_VERSION {
            anyhow::bail!(
                "{} declares version {}, expected {}",
                GEOGRAPHY_FILE,
                geo_raw.version,
                GEOGRAPHY_VERSION
            );
        }
        let settlements: Vec<Settlement> = geo_raw
            .settlements
            .iter()
            .map(|s| Settlement {
                id: s.id.clone(),
                name: s.name.clone(),
                polity: s.polity.clone(),
                x: s.x,
                y: s.y,
                places: s
                    .places
                    .iter()
                    .map(|p| Place {
                        id: p.id.clone(),
                        kind: p.kind.clone(),
                        name: p.name.clone(),
                    })
                    .collect(),
            })
            .collect();
        let rivers: Vec<River> = geo_raw
            .rivers
            .iter()
            .map(|r| River {
                id: r.id.clone(),
                name: r.name.clone(),
                points: r.points.clone(),
                length_km: r.length_km,
                width_class: r.width_class.clone(),
                navigable: r.navigable,
                seasonal: r.seasonal.clone(),
            })
            .collect();
        let routes: Vec<Route> = geo_raw
            .routes
            .iter()
            .map(|r| Route {
                id: r.id.clone(),
                from: r.from.clone(),
                to: r.to.clone(),
                risky: r.risky,
                path_km: r.path_km,
                tiles: r.tiles.clone(),
                modes: r.modes.clone(),
                seasonal: r.seasonal,
                blocked: r.blocked,
            })
            .collect();
        let regions: Vec<crate::world::Region> = geo_raw
            .regions
            .iter()
            .map(|r| crate::world::Region {
                id: r.id.clone(),
                polygon: crate::world::geo::KmPolygon {
                    id: r.id.clone(),
                    ring: r
                        .polygon
                        .iter()
                        .map(|&(x, y)| crate::world::geo::KmPoint::new(x, y))
                        .collect(),
                },
            })
            .collect();
        let landmarks: Vec<crate::world::Landmark> = geo_raw
            .landmarks
            .iter()
            .map(|l| {
                let x = (l.point.0 / crate::world::coords::KM_PER_CELL) as u32;
                let y = (l.point.1 / crate::world::coords::KM_PER_CELL) as u32;
                crate::world::Landmark {
                    id: l.id.clone(),
                    name: if l.name.is_empty() {
                        l.id.clone()
                    } else {
                        l.name.clone()
                    },
                    kind: l.kind.clone(),
                    point: crate::world::geo::KmPoint::new(l.point.0, l.point.1),
                    x,
                    y,
                }
            })
            .collect();
        (settlements, rivers, routes, regions, landmarks)
    } else {
        // Legacy path: geography lives in world.toml.
        let settlements: Vec<Settlement> = raw
            .settlements
            .iter()
            .map(|s| Settlement {
                id: s.id.clone(),
                name: s.name.clone(),
                polity: s.polity.clone(),
                x: s.x,
                y: s.y,
                places: s
                    .places
                    .iter()
                    .map(|p| Place {
                        id: p.id.clone(),
                        kind: p.kind.clone(),
                        name: p.name.clone(),
                    })
                    .collect(),
            })
            .collect();
        let rivers: Vec<River> = raw
            .rivers
            .iter()
            .map(|r| River {
                id: r.id.clone(),
                name: r.name.clone(),
                points: r.points.clone(),
                length_km: 0.0,
                width_class: "normal".into(),
                navigable: true,
                seasonal: None,
            })
            .collect();
        let routes: Vec<Route> = raw
            .routes
            .iter()
            .map(|r| {
                // Legacy content has no committed route geometry: fill it
                // with a deterministic walk between the endpoints so fast
                // travel and the map overlay agree (preview == executed).
                // Prefer the game's own A* (passable tiles, so content
                // validation stays clean); straight-line as a last resort.
                let from_tile = raw
                    .settlements
                    .iter()
                    .find(|s| s.id == r.from)
                    .map(|s| (s.x, s.y));
                let to_tile = raw
                    .settlements
                    .iter()
                    .find(|s| s.id == r.to)
                    .map(|s| (s.x, s.y));
                let (tiles, path_km) = match (from_tile, to_tile) {
                    (Some(a), Some(b)) => {
                        let tiles = crate::world::pathfind::find_path(&grid, a, b)
                            .unwrap_or_else(|| crate::world::geo::straight_tiles(a, b));
                        let path_km = crate::world::geo::tile_distance_km(
                            a,
                            b,
                            crate::world::coords::KM_PER_CELL,
                        );
                        (tiles, path_km)
                    }
                    _ => (Vec::new(), 0.0),
                };
                Route {
                    id: r.id.clone(),
                    from: r.from.clone(),
                    to: r.to.clone(),
                    risky: r.risky,
                    path_km,
                    tiles,
                    modes: vec!["caravan".into()],
                    seasonal: false,
                    blocked: false,
                }
            })
            .collect();
        (
            settlements,
            rivers,
            routes,
            vec![crate::world::Region {
                id: raw.region.id.clone(),
                polygon: crate::world::geo::KmPolygon {
                    id: raw.region.id.clone(),
                    // Legacy world.toml has no polygon data; a degenerate box
                    // covering the full extent is a placeholder until
                    // geography.toml provides real region geometry.
                    ring: vec![
                        crate::world::geo::KmPoint::new(0.0, 0.0),
                        crate::world::geo::KmPoint::new(
                            img.width() as f64 * crate::world::coords::KM_PER_CELL,
                            0.0,
                        ),
                        crate::world::geo::KmPoint::new(
                            img.width() as f64 * crate::world::coords::KM_PER_CELL,
                            img.height() as f64 * crate::world::coords::KM_PER_CELL,
                        ),
                        crate::world::geo::KmPoint::new(
                            0.0,
                            img.height() as f64 * crate::world::coords::KM_PER_CELL,
                        ),
                        crate::world::geo::KmPoint::new(0.0, 0.0),
                    ],
                },
            }],
            Vec::<crate::world::Landmark>::new(),
        )
    };

    Ok(World {
        width: img.width(),
        height: img.height(),
        grid,
        settlements,
        rivers,
        routes,
        regions,
        landmarks,
        knowledges: load_region_knowledge(content_dir)?,
        start_settlement: raw.region.start_settlement,
    })
}

/// Content-ID-addressed art manifest (DESIGN.md §5).
///
/// Maps asset IDs to file paths relative to the assets directory. Every
/// entry is optional: a missing or unloadable file means the renderer falls
/// back to placeholder art (tinted shape + label), so the game always runs
/// regardless of art coverage.
#[derive(Debug, Clone, Default)]
pub struct AssetManifest {
    /// Asset ID -> absolute-ish path (assets dir joined with the entry).
    pub entries: BTreeMap<String, PathBuf>,
}

impl AssetManifest {
    /// Resolve an asset ID to its file path, if the manifest names it.
    pub fn path(&self, id: &str) -> Option<&Path> {
        self.entries.get(id).map(PathBuf::as_path)
    }
}

#[derive(serde::Deserialize)]
struct AssetsToml {
    version: u32,
    #[serde(default)]
    assets: BTreeMap<String, String>,
}

/// Load the art manifest from the assets directory. Fails only when the
/// manifest itself is broken; missing art files are NOT an error (§5).
pub fn load_assets(assets_dir: &Path) -> Result<AssetManifest> {
    let path = assets_dir.join(ASSETS_FILE);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let raw: AssetsToml =
        toml::from_str(&text).with_context(|| format!("parsing {}", ASSETS_FILE))?;
    if raw.version != ASSETS_VERSION {
        anyhow::bail!(
            "{} declares version {}, expected {}",
            ASSETS_FILE,
            raw.version,
            ASSETS_VERSION
        );
    }
    let entries = raw
        .assets
        .into_iter()
        .map(|(id, rel)| (id, assets_dir.join(rel)))
        .collect();
    Ok(AssetManifest { entries })
}

// ---------------------------------------------------------------------------
// Game content: goods, markets, generator inputs, NPCs
// ---------------------------------------------------------------------------

/// Load all game content (goods, markets, NPCs, clue-generator inputs) from
/// the content directory. Schema errors and unresolvable references fail
/// loudly.
pub fn load_game_content(content_dir: &Path) -> Result<GameContent> {
    let goods = load_goods(content_dir)?;
    Ok(GameContent {
        goods: goods.clone(),
        markets: load_markets(content_dir)?,
        npcs: load_npcs(content_dir, &goods)?,
        generator: load_generator_input(content_dir, &goods)?,
    })
}

#[derive(serde::Deserialize)]
struct GoodsFile {
    version: u32,
    goods: Vec<GoodsToml>,
}

#[derive(serde::Deserialize)]
struct GoodsToml {
    id: String,
    name: String,
    category: Category,
    base_price: u32,
    #[serde(default)]
    gift_value: i32,
    #[serde(default)]
    restores_hydration: f32,
    #[serde(default)]
    restores_nutrition: f32,
    #[serde(default)]
    cures: Option<Ailment>,
}

fn load_goods(content_dir: &Path) -> Result<BTreeMap<String, Goods>> {
    let raw: GoodsFile = read_toml(content_dir, GOODS_FILE)?;
    check_version(GOODS_FILE, raw.version)?;
    let mut goods = BTreeMap::new();
    for g in raw.goods {
        goods.insert(
            g.id.clone(),
            Goods {
                id: g.id,
                name: g.name,
                category: g.category,
                base_price: g.base_price,
                gift_value: g.gift_value,
                restores_hydration: g.restores_hydration,
                restores_nutrition: g.restores_nutrition,
                cures: g.cures,
            },
        );
    }
    Ok(goods)
}

#[derive(serde::Deserialize)]
struct MarketsFile {
    version: u32,
    market: Vec<MarketToml>,
}

#[derive(serde::Deserialize)]
struct MarketToml {
    settlement: String,
    #[serde(default)]
    item: Vec<ItemFactorToml>,
}

#[derive(serde::Deserialize)]
struct ItemFactorToml {
    id: String,
    #[serde(default = "one")]
    demand: f32,
    #[serde(default = "one")]
    supply: f32,
}

fn one() -> f32 {
    1.0
}

fn load_markets(content_dir: &Path) -> Result<BTreeMap<String, Market>> {
    let raw: MarketsFile = read_toml(content_dir, MARKETS_FILE)?;
    check_version(MARKETS_FILE, raw.version)?;
    let mut markets = BTreeMap::new();
    for m in raw.market {
        let items = m
            .item
            .into_iter()
            .map(|i| {
                (
                    i.id,
                    MarketEntry {
                        demand: i.demand,
                        supply: i.supply,
                    },
                )
            })
            .collect();
        markets.insert(
            m.settlement.clone(),
            Market {
                settlement_id: m.settlement,
                items,
            },
        );
    }
    Ok(markets)
}

#[derive(serde::Deserialize)]
struct CluesFile {
    version: u32,
    expedition: ExpeditionToml,
    generator: GeneratorToml,
    #[serde(default)]
    template: Vec<ClueTemplateToml>,
}

#[derive(serde::Deserialize)]
struct ExpeditionToml {
    tomb_candidates: Vec<(u32, u32)>,
}

#[derive(serde::Deserialize)]
struct GeneratorToml {
    settlements: Vec<String>,
    key_item: String,
    rivals: Vec<String>,
    #[serde(default)]
    herrings: u32,
    rep_gate_min: i32,
    rep_gate_max: i32,
}

#[derive(serde::Deserialize)]
struct ClueTemplateToml {
    kind: ClueKind,
    #[serde(default)]
    red_herring: bool,
    text: String,
}

/// Load the seeded clue-graph generator inputs (config, templates, tomb
/// candidates, key goods). `None` when no clues file exists.
pub fn load_generator_input(
    content_dir: &Path,
    goods: &BTreeMap<String, Goods>,
) -> Result<Option<GeneratorInput>> {
    let raw: CluesFile = read_toml(content_dir, CLUES_FILE)?;
    check_version(CLUES_FILE, raw.version)?;
    let key_goods = goods.get(&raw.generator.key_item).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "generator key_item {:?} is not a good",
            raw.generator.key_item
        )
    })?;
    Ok(Some(GeneratorInput {
        config: GeneratorConfig {
            settlements: raw.generator.settlements,
            key_item: raw.generator.key_item,
            rivals: raw.generator.rivals,
            herrings: raw.generator.herrings,
            rep_gate_min: raw.generator.rep_gate_min,
            rep_gate_max: raw.generator.rep_gate_max,
        },
        templates: raw
            .template
            .into_iter()
            .map(|t| ClueTemplate {
                kind: t.kind,
                red_herring: t.red_herring,
                text: t.text,
            })
            .collect(),
        tomb_candidates: raw.expedition.tomb_candidates,
        key_goods,
    }))
}

#[derive(serde::Deserialize)]
struct NpcToml {
    id: String,
    name: String,
    settlement: String,
    opening: String,
    #[serde(default)]
    tomb_answer: String,
    #[serde(default)]
    gift: Option<Gift>,
    #[serde(default)]
    node: Vec<NodeToml>,
}

#[derive(serde::Deserialize)]
struct GiftToml {
    item: String,
    rep_delta: i32,
}

impl From<GiftToml> for Gift {
    fn from(g: GiftToml) -> Self {
        Gift {
            item: g.item,
            rep_delta: g.rep_delta,
        }
    }
}

#[derive(serde::Deserialize)]
struct NodeToml {
    id: String,
    text: String,
    #[serde(default)]
    option: Vec<OptionToml>,
}

#[derive(serde::Deserialize)]
struct OptionToml {
    text: String,
    #[serde(default)]
    go_to: Option<String>,
    /// TOML `if` is a Rust keyword; renamed.
    #[serde(rename = "if", default)]
    r#if: Option<Condition>,
    #[serde(default)]
    effect: Option<Effect>,
}

fn load_npcs(content_dir: &Path, goods: &BTreeMap<String, Goods>) -> Result<BTreeMap<String, Npc>> {
    let npcs_dir = content_dir.join(NPCS_DIR);
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&npcs_dir)
        .with_context(|| format!("reading {}", npcs_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    entries.sort(); // deterministic load order

    let mut npcs = BTreeMap::new();
    for path in entries {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let raw: NpcToml =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let mut nodes: BTreeMap<String, Node> = raw
            .node
            .into_iter()
            .map(|n| {
                let options = n
                    .option
                    .into_iter()
                    .map(|o| DialogueOption {
                        text: o.text,
                        go_to: o.go_to,
                        condition: o.r#if.unwrap_or_default(),
                        effect: o.effect.unwrap_or_default(),
                    })
                    .collect();
                (
                    n.id.clone(),
                    Node {
                        id: n.id,
                        text: n.text,
                        options,
                    },
                )
            })
            .collect();

        // Gift synthesis: the accepted gift becomes an option in every node
        // (data files never repeat the block; §4.5).
        let gift = raw.gift;
        if let Some(gift) = &gift {
            let goods = goods.get(&gift.item).ok_or_else(|| {
                anyhow::anyhow!("NPC {:?} gift item {:?} is not a good", raw.id, gift.item)
            })?;
            let option = DialogueOption {
                text: format!("Give {} as a gift.", goods.name),
                go_to: Some("gift_ack".to_string()),
                condition: Condition {
                    has_item: Some(gift.item.clone()),
                    ..Default::default()
                },
                effect: Effect {
                    take_item: Some(gift.item.clone()),
                    rep_delta: Some(gift.rep_delta),
                    ..Default::default()
                },
            };
            for node in nodes.values_mut() {
                node.options.push(option.clone());
            }
        }

        npcs.insert(
            raw.id.clone(),
            Npc {
                id: raw.id,
                name: raw.name,
                settlement: raw.settlement,
                opening: raw.opening,
                tomb_answer: raw.tomb_answer,
                gift,
                nodes,
            },
        );
    }
    Ok(npcs)
}

/// Read and parse a TOML file from the content directory.
fn read_toml<T: serde::de::DeserializeOwned>(content_dir: &Path, file: &str) -> Result<T> {
    let path = content_dir.join(file);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {file}"))
}

fn check_version(file: &str, version: u32) -> Result<()> {
    if version != CONTENT_VERSION {
        anyhow::bail!("{file} declares version {version}, expected {CONTENT_VERSION}");
    }
    Ok(())
}

/// TOML shape for a single region-knowledge file under `regions/`.
#[derive(serde::Deserialize)]
struct RegionKnowledgeToml {
    version: u32,
    region: String,
    #[serde(default)]
    direction_terms: Vec<String>,
    #[serde(default)]
    distance_terms: Vec<String>,
    #[serde(default)]
    preferred_gifts: Vec<String>,
    #[serde(default)]
    disliked_gifts: Vec<String>,
    #[serde(default)]
    terrain_lore: Vec<String>,
    #[serde(default)]
    landmark_terms: Vec<String>,
    #[serde(default)]
    etiquette: Vec<String>,
}

/// Load per-region knowledge files from `content/regions/`.
/// Empty when the directory is absent (graceful — regional knowledge is
/// additive). A malformed file or a reference to a missing region id is a
/// content-lint error.
pub fn load_region_knowledge(content_dir: &Path) -> Result<BTreeMap<String, RegionKnowledge>> {
    let dir = content_dir.join(REGIONS_DIR);
    if !dir.is_dir() {
        return Ok(BTreeMap::new());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    entries.sort(); // deterministic load order
    let mut knowledges = BTreeMap::new();
    for path in entries {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let raw: RegionKnowledgeToml =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if raw.version != CONTENT_VERSION {
            anyhow::bail!(
                "{} declares version {}, expected {}",
                path.display(),
                raw.version,
                CONTENT_VERSION
            );
        }
        knowledges.insert(
            raw.region.clone(),
            RegionKnowledge {
                direction_terms: raw.direction_terms,
                distance_terms: raw.distance_terms,
                preferred_gifts: raw.preferred_gifts,
                disliked_gifts: raw.disliked_gifts,
                terrain_lore: raw.terrain_lore,
                landmark_terms: raw.landmark_terms,
                etiquette: raw.etiquette,
            },
        );
    }
    Ok(knowledges)
}

/// Biome definitions must satisfy: passable biomes cost >= 1.0, impassable
/// biomes cost 0.0. Violations are content-lint errors.
fn validate_biome(def: &BiomeDef) -> Result<()> {
    if def.passable && def.movement_cost < 1.0 {
        anyhow::bail!(
            "biome {:?}: passable biomes must cost at least 1.0, got {}",
            def.id,
            def.movement_cost
        );
    }
    if !def.passable && def.movement_cost != 0.0 {
        anyhow::bail!(
            "biome {:?}: impassable biomes must cost 0.0, got {}",
            def.id,
            def.movement_cost
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_costs_are_enforced() {
        let ok = BiomeDef {
            id: "desert".into(),
            passable: true,
            movement_cost: 1.0,
            color: (0xd9, 0xb3, 0x6c),
        };
        assert!(validate_biome(&ok).is_ok());

        let bad = BiomeDef {
            id: "water".into(),
            passable: false,
            movement_cost: 5.0,
            color: (0x3a, 0x6e, 0xa5),
        };
        assert!(validate_biome(&bad).is_err());

        let bad = BiomeDef {
            id: "desert".into(),
            passable: true,
            movement_cost: 0.5,
            color: (0xd9, 0xb3, 0x6c),
        };
        assert!(validate_biome(&bad).is_err());
    }

    #[test]
    fn region_knowledge_loads_from_toml() {
        let dir = std::env::current_dir().unwrap().join("content");
        let knowledges = load_region_knowledge(&dir).unwrap();
        assert!(
            knowledges.contains_key("lower-egypt-nile"),
            "nile region knowledge must load: keys {:?}",
            knowledges.keys().collect::<Vec<_>>()
        );
        let k = &knowledges["lower-egypt-nile"];
        assert!(!k.direction_terms.is_empty());
        assert!(!k.preferred_gifts.contains(&"beads".to_string()));
        assert!(k.disliked_gifts.contains(&"beads".to_string()));
    }
}
