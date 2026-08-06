//! The seeded clue graph (DESIGN.md §4.5).
//!
//! Phase 3: the mystery is generated per playthrough from the world seed —
//! role assignment, tomb site, and texts all vary. The generator produces a
//! diamond DAG with two disjoint paths to the location clue, plus falsifiable
//! red herrings, and *proves solvability* before accepting a seed (§9): with
//! any subset of exclusives consumed by rivals, at least one complete path
//! to the tomb identification must remain.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use rand::RngExt;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

use crate::dialogue::npc::Npc;
use crate::sim::economy::Goods;
use crate::world::geo::tile_distance_km;
use crate::world::{KM_PER_CELL, World};

/// Clue node kinds (DESIGN.md §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClueKind {
    /// Points toward the next step.
    Hint,
    /// Yields a key item needed elsewhere.
    Key,
    /// Corroborates the trail; needed before the location is trusted.
    Corroboration,
    /// Names the tomb site.
    Location,
}

/// One clue node: gated by reputation and prerequisites, revealed by a
/// specific NPC, optionally granting or requiring an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clue {
    pub id: String,
    pub kind: ClueKind,
    /// Settlement where the clue is revealed.
    pub settlement: String,
    /// NPC who reveals it.
    pub npc: String,
    /// Reputation required in the settlement before the clue is offered.
    pub rep_gate: i32,
    /// Clue IDs that must ALL be collected first.
    pub prereq: Vec<String>,
    /// Alternative prerequisites: ANY one suffices (the diamond's OR-leg).
    #[serde(default)]
    pub prereq_any: Vec<String>,
    /// Item granted when the clue is revealed.
    pub gives_item: Option<String>,
    /// Item the player must carry to receive the clue.
    pub requires_item: Option<String>,
    /// The in-fiction text of the clue (the diary entry).
    pub text: String,
    /// Red herrings are marked internally; they are falsifiable via
    /// `contradicted_by` (DESIGN.md §4.5).
    pub red_herring: bool,
    /// When this clue is obtained, the herring it contradicts is proven false.
    pub contradicts_herring: Option<String>,
}

/// The expedition's win condition: the tomb tile. The corroboration
/// requirement is structural — `ClueGraph::win_condition_met`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expedition {
    pub tomb_tile: (u32, u32),
}

/// Eight-compass relation between two tiles (grid: +x east, +y south).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompassRelation {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl CompassRelation {
    pub fn phrase(self) -> &'static str {
        match self {
            CompassRelation::North => "north",
            CompassRelation::NorthEast => "northeast",
            CompassRelation::East => "east",
            CompassRelation::SouthEast => "southeast",
            CompassRelation::South => "south",
            CompassRelation::SouthWest => "southwest",
            CompassRelation::West => "west",
            CompassRelation::NorthWest => "northwest",
        }
    }
}

/// The octant (45° wedge) of `to` as seen from `from`.
pub fn relation_between(from: (u32, u32), to: (u32, u32)) -> CompassRelation {
    let dx = to.0 as f64 - from.0 as f64;
    let dy = to.1 as f64 - from.1 as f64;
    // atan2(y, x): 0 = east, π/2 = south, ±π = west, -π/2 = north.
    let angle = dy.atan2(dx);
    match (angle * 8.0 / std::f64::consts::TAU).round() as i64 {
        0 | 8 => CompassRelation::East,
        1 => CompassRelation::SouthEast,
        2 => CompassRelation::South,
        3 => CompassRelation::SouthWest,
        4 | -4 => CompassRelation::West,
        -3 => CompassRelation::NorthWest,
        -2 => CompassRelation::North,
        -1 => CompassRelation::NorthEast,
        _ => CompassRelation::East, // unreachable; atan2 output is bounded
    }
}

/// How far a hint claims the tomb is from its origin, in marching days.
/// Band boundaries are kilometres (see `from_km`); the same table drives
/// the players' search sectors, so hint and gameplay always agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceBand {
    ShortWalk,
    HalfDay,
    OneDay,
    TwoDays,
    LongJourney,
}

impl DistanceBand {
    /// Band boundaries, km. A march day ≈ 8 walking tiles ≈ 265 km; bands
    /// are coarse so a hint never overclaims precision.
    const KM_LIMITS: [f64; 4] = [100.0, 250.0, 500.0, 1000.0];

    pub fn from_km(km: f64) -> DistanceBand {
        // Partitions: [0,100) [100,250) [250,500) [500,1000] (1000,∞).
        // A band's lower limit belongs to the higher band, so a hint never
        // overclaims at the boundary.
        let limits = DistanceBand::KM_LIMITS;
        if km < limits[0] {
            DistanceBand::ShortWalk
        } else if km < limits[1] {
            DistanceBand::HalfDay
        } else if km < limits[2] {
            DistanceBand::OneDay
        } else if km <= limits[3] {
            DistanceBand::TwoDays
        } else {
            DistanceBand::LongJourney
        }
    }

    /// Inclusive km range of the band: [min, max].
    pub fn km_range(&self) -> (f64, f64) {
        let idx = match self {
            DistanceBand::ShortWalk => 0,
            DistanceBand::HalfDay => 1,
            DistanceBand::OneDay => 2,
            DistanceBand::TwoDays => 3,
            DistanceBand::LongJourney => 4,
        };
        let min = if idx == 0 {
            0.0
        } else {
            DistanceBand::KM_LIMITS[idx - 1]
        };
        let max = if idx == 4 {
            f64::INFINITY
        } else {
            DistanceBand::KM_LIMITS[idx]
        };
        (min, max)
    }

    pub fn phrase(self) -> &'static str {
        match self {
            DistanceBand::ShortWalk => "a short walk",
            DistanceBand::HalfDay => "half a day's march",
            DistanceBand::OneDay => "a day's march",
            DistanceBand::TwoDays => "two days' march",
            DistanceBand::LongJourney => "a long journey",
        }
    }
}

/// What a spatial hint points at (WS3 adds treasure sites and landmarks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintTarget {
    Tomb,
}

/// The semantic content behind a location clue (EXPANSION_PLAN §WS2): the
/// text a player reads is a faithful render of this struct. Search sectors
/// derive from it, so solvability operates on semantics only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialHint {
    /// Origin anchor: a settlement id (landmarks join in the regions
    /// milestone).
    pub origin: String,
    pub relation: CompassRelation,
    pub band: DistanceBand,
    /// How strongly the hint is phrased (0.75–0.95; WS3 search feedback).
    pub confidence: f64,
    pub target: HintTarget,
}

impl SpatialHint {
    /// The anchor tile the hint is measured from.
    pub fn origin_tile(&self, world: &World) -> Option<(u32, u32)> {
        let s = world.settlement(&self.origin)?;
        Some((s.x, s.y))
    }

    /// Whether `tile` lies in the claimed search sector: the octant wedge
    /// at the claimed distance band, measured from the origin anchor.
    pub fn sector_contains(&self, world: &World, tile: (u32, u32)) -> bool {
        let Some(origin) = self.origin_tile(world) else {
            return false;
        };
        if tile == origin {
            return false;
        }
        if relation_between(origin, tile) != self.relation {
            return false;
        }
        let km = tile_distance_km(origin, tile, KM_PER_CELL);
        let (min, max) = self.band.km_range();
        km >= min && km <= max
    }
}

/// A generated mystery: the clue nodes plus the win condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClueGraph {
    pub clues: BTreeMap<String, Clue>,
    pub expedition: Expedition,
    /// Structural ids of the two legs (see `Generator`).
    pub leg_a: Vec<String>,
    pub leg_b: Vec<String>,
    /// The start hint and the location clue.
    pub start_clue: String,
    pub location_clue: String,
    /// The semantic content the location clue names (None for synthetic
    /// test graphs that skip the generator).
    pub spatial_hint: Option<SpatialHint>,
    /// Settlement role order [start, mid1, mid2, end]: the rivals' circuit.
    pub settlement_order: Vec<String>,
    /// Display names of the graph settlements (for texts and rumors).
    pub settlement_display: BTreeMap<String, String>,
}

impl ClueGraph {
    /// Whether `obtained` satisfies the prerequisites of a clue (ALL of
    /// `prereq`, plus ANY one of `prereq_any` when it is non-empty).
    pub fn prereqs_met(&self, clue_id: &str, obtained: &BTreeSet<String>) -> bool {
        let Some(clue) = self.clues.get(clue_id) else {
            return false;
        };
        clue.prereq.iter().all(|p| obtained.contains(p))
            && (clue.prereq_any.is_empty() || clue.prereq_any.iter().any(|p| obtained.contains(p)))
    }

    /// A complete win path exists among the obtained clues (either leg).
    pub fn win_condition_met(&self, obtained: &BTreeSet<String>) -> bool {
        let leg_ok = |leg: &[String]| leg.iter().all(|id| obtained.contains(id));
        leg_ok(&self.leg_a) || leg_ok(&self.leg_b)
    }

    /// Solvability proof: with `consumed` clue ids unavailable (taken by
    /// rivals), is at least one complete path from the start clue to the
    /// location clue still reachable? Honoring prerequisites (rep gates are
    /// surmountable by the player) and key items: a clue that requires an
    /// item is only reachable if some reachable clue grants it (§4.5).
    pub fn is_solvable(&self, consumed: &BTreeSet<String>) -> bool {
        // Reachability fixpoint over the graph from the start clue, tracking
        // granted items so requires_item gates are proven, not assumed.
        let mut reachable: BTreeSet<String> = BTreeSet::new();
        let mut granted: BTreeSet<String> = BTreeSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for clue in self.clues.values() {
                if consumed.contains(&clue.id) || reachable.contains(&clue.id) {
                    continue;
                }
                let all_met = clue.prereq.iter().all(|p| reachable.contains(p));
                let any_met = clue.prereq_any.is_empty()
                    || clue.prereq_any.iter().any(|p| reachable.contains(p));
                let item_met = clue
                    .requires_item
                    .as_ref()
                    .is_none_or(|item| granted.contains(item));
                if all_met && any_met && item_met {
                    reachable.insert(clue.id.clone());
                    if let Some(item) = &clue.gives_item {
                        granted.insert(item.clone());
                    }
                    changed = true;
                }
            }
        }
        reachable.contains(&self.location_clue)
    }
}

/// Seeded generator parameters from `content/clues.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratorConfig {
    /// Settlements the graph is distributed across (in order of preference).
    pub settlements: Vec<String>,
    /// Item the key leg grants and the corroboration requires.
    pub key_item: String,
    /// Names of the rival explorers (DESIGN.md §4.8).
    pub rivals: Vec<String>,
    /// Number of red herrings to generate.
    pub herrings: u32,
    pub rep_gate_min: i32,
    pub rep_gate_max: i32,
}

/// Text templates for generated clues; `{settlement}`, `{npc}`, `{key}` are
/// substituted at generation time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClueTemplate {
    pub kind: ClueKind,
    /// Red-herring templates are marked and made falsifiable.
    #[serde(default)]
    pub red_herring: bool,
    pub text: String,
}

/// Everything the generator needs from content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratorInput {
    pub config: GeneratorConfig,
    pub templates: Vec<ClueTemplate>,
    pub tomb_candidates: Vec<(u32, u32)>,
    pub key_goods: Goods,
}

/// Deterministic seeded generator. The produced graph is a pure function of
/// `(seed, content)`, so replays and saves agree on it without serializing
/// the graph itself.
pub struct ClueGenerator<'a> {
    rng: ChaCha20Rng,
    seed: u64,
    input: &'a GeneratorInput,
    world: &'a World,
    npcs: &'a BTreeMap<String, Npc>,
}

impl<'a> ClueGenerator<'a> {
    pub fn new(
        seed: u64,
        input: &'a GeneratorInput,
        world: &'a World,
        npcs: &'a BTreeMap<String, Npc>,
    ) -> Self {
        let mut seed_bytes = [0u8; 32];
        seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
        ClueGenerator {
            rng: ChaCha20Rng::from_seed(seed_bytes),
            seed,
            input,
            world,
            npcs,
        }
    }

    /// Generate and prove a mystery for this seed. Panics if the generated
    /// graph fails the solvability proof (a content or generator bug).
    pub fn generate(mut self) -> ClueGraph {
        let settlements = self.role_assignment();
        let (start_id, mid1_id, mid2_id, end_id) = (
            settlements[0].clone(),
            settlements[1].clone(),
            settlements[2].clone(),
            settlements[3].clone(),
        );
        // NPC per graph settlement, resolved before the building closure so
        // the closure only borrows `self` mutably (for the RNG).
        let npc_of: BTreeMap<String, String> = settlements
            .iter()
            .map(|s| (s.clone(), self.npc_for(s).to_string()))
            .collect();
        let npc_name_of: BTreeMap<String, String> = npc_of
            .iter()
            .map(|(settlement, id)| {
                let name = self
                    .npcs
                    .get(id)
                    .map(|n| n.name.clone())
                    .unwrap_or_else(|| id.clone());
                (settlement.clone(), name)
            })
            .collect();
        let display_of: BTreeMap<String, String> = settlements
            .iter()
            .map(|s| {
                let name = self
                    .world
                    .settlement(s)
                    .map(|st| st.name.clone())
                    .unwrap_or_else(|| s.clone());
                (s.clone(), name)
            })
            .collect();

        let key_name = self.input.key_goods.name.clone();
        let key_item_id = self.input.config.key_item.clone();
        let herrings = self.input.config.herrings;
        // Gates and herring settlements drawn up front so the building
        // closure below borrows only locals, not `self`.
        let (c1_gate, f1_gate, f2_gate, c5_gate) =
            (self.gate(), self.gate(), self.gate(), self.gate());
        let herring_settlements: Vec<&String> = (0..herrings)
            .map(|_| &settlements[self.rng.random_range(0..settlements.len())])
            .collect();

        let clue = |id: &str,
                    kind: ClueKind,
                    settlement: &str,
                    text_at: &str,
                    prereq: Vec<String>,
                    prereq_any: Vec<String>,
                    rep_gate: i32,
                    gives: Option<&str>,
                    requires: Option<&str>,
                    herring: bool,
                    contradicts: Option<&str>| {
            let template = self.pick_template(kind, herring);
            Clue {
                id: id.to_string(),
                kind,
                settlement: settlement.to_string(),
                npc: npc_of[settlement].clone(),
                rep_gate,
                prereq,
                prereq_any,
                gives_item: gives.map(str::to_string),
                requires_item: requires.map(str::to_string),
                text: self.render(
                    template,
                    &display_of[text_at],
                    &npc_name_of[text_at],
                    &key_name,
                    "",
                    "",
                ),
                red_herring: herring,
                contradicts_herring: contradicts.map(str::to_string),
            }
        };

        let mut clues = BTreeMap::new();
        // Start hint at the start settlement.
        let start = clue(
            "c1",
            ClueKind::Hint,
            &start_id,
            &mid1_id,
            Vec::new(),
            Vec::new(),
            c1_gate,
            None,
            None,
            false,
            None,
        );
        let start_clue = start.id.clone();
        clues.insert(start.id.clone(), start);

        // Leg A: key (grants the key item) -> corroboration (requires it).
        let e1 = clue(
            "e1",
            ClueKind::Key,
            &mid1_id,
            &mid2_id,
            vec![start_clue.clone()],
            Vec::new(),
            0,
            Some(&key_item_id),
            None,
            false,
            None,
        );
        let e2 = clue(
            "e2",
            ClueKind::Corroboration,
            &mid2_id,
            &end_id,
            vec![e1.id.clone()],
            Vec::new(),
            0,
            None,
            Some(&key_item_id),
            false,
            None,
        );
        // Leg B: hint -> corroboration, sharing no intermediate node with A.
        let f1 = clue(
            "f1",
            ClueKind::Hint,
            &mid1_id,
            &mid2_id,
            vec![start_clue.clone()],
            Vec::new(),
            f1_gate,
            None,
            None,
            false,
            None,
        );
        let f2 = clue(
            "f2",
            ClueKind::Corroboration,
            &mid2_id,
            &end_id,
            vec![f1.id.clone()],
            Vec::new(),
            f2_gate,
            None,
            None,
            false,
            None,
        );

        // Location clue at the end settlement, fed by EITHER leg (OR-prereq).
        // Built after `pick_tomb` so its text can carry the spatial hint
        // computed from real geometry (EXPANSION_PLAN §WS2 milestone 1).
        let e2_id = e2.id.clone();
        let f2_id = f2.id.clone();
        clues.insert(e1.id.clone(), e1);
        clues.insert(e2.id.clone(), e2);
        clues.insert(f1.id.clone(), f1);
        clues.insert(f2.id.clone(), f2);

        // Red herrings: falsifiable by obtaining e2 (the leg-A corroboration).
        for (i, settlement) in herring_settlements.iter().enumerate() {
            let herring = clue(
                &format!("h{}", i + 1),
                ClueKind::Hint,
                settlement,
                settlement,
                Vec::new(),
                Vec::new(),
                0,
                None,
                None,
                true,
                Some("e2"),
            );
            clues.insert(herring.id.clone(), herring);
        }

        let tomb_tile = self.pick_tomb();
        let spatial_hint = self.hint_for_tomb(tomb_tile, &settlements);
        let c5 = Clue {
            id: "c5".to_string(),
            kind: ClueKind::Location,
            settlement: end_id.clone(),
            npc: npc_of[&end_id].clone(),
            rep_gate: c5_gate,
            prereq: Vec::new(),
            prereq_any: vec![e2_id, f2_id],
            gives_item: None,
            requires_item: None,
            text: {
                let template = self.pick_template(ClueKind::Location, false);
                self.render(
                    template,
                    &display_of[&spatial_hint.origin],
                    &npc_name_of[&end_id],
                    &key_name,
                    spatial_hint.relation.phrase(),
                    spatial_hint.band.phrase(),
                )
            },
            red_herring: false,
            contradicts_herring: None,
        };
        let location_clue = c5.id.clone();
        clues.insert(c5.id.clone(), c5);

        let graph = ClueGraph {
            clues,
            expedition: Expedition { tomb_tile },
            leg_a: vec![
                start_clue.clone(),
                "e1".into(),
                "e2".into(),
                location_clue.clone(),
            ],
            leg_b: vec![start_clue, "f1".into(), "f2".into(), location_clue.clone()],
            start_clue: "c1".into(),
            location_clue,
            spatial_hint: Some(spatial_hint),
            settlement_order: settlements.clone(),
            settlement_display: display_of,
        };

        // The solvability proof: two disjoint-enough paths exist, and both
        // survive any single-leg rival consumption.
        assert!(
            graph.is_solvable(&BTreeSet::new()),
            "seed produced an unsolvable graph"
        );
        for exclusive in ["e1", "e2", "f1", "f2"] {
            assert!(
                graph.is_solvable(&BTreeSet::from([exclusive.to_string()])),
                "rival consumption of {exclusive} breaks solvability"
            );
        }
        graph
    }

    /// Assign the four graph roles to settlements: the world's start keeps
    /// the start role; the rest are shuffled per seed.
    fn role_assignment(&mut self) -> Vec<String> {
        let mut order: Vec<String> = self
            .input
            .config
            .settlements
            .iter()
            .filter(|s| **s != self.world.start_settlement)
            .cloned()
            .collect();
        order.shuffle(&mut self.rng);
        let mut full = vec![self.world.start_settlement.clone()];
        full.extend(order.into_iter().take(3));
        full
    }

    fn npc_for(&self, settlement: &str) -> &str {
        self.npcs
            .values()
            .find(|npc| npc.settlement == settlement)
            .map(|npc| npc.id.as_str())
            .expect("every graph settlement has a named NPC (content lint)")
    }

    fn pick_template(&self, kind: ClueKind, herring: bool) -> &ClueTemplate {
        self.input
            .templates
            .iter()
            .find(|t| t.kind == kind && t.red_herring == herring)
            .unwrap_or_else(|| panic!("no template for kind {kind:?} herring={herring}"))
    }

    fn render(
        &self,
        template: &ClueTemplate,
        settlement: &str,
        npc: &str,
        key: &str,
        direction: &str,
        distance: &str,
    ) -> String {
        template
            .text
            .replace("{settlement}", settlement)
            .replace("{npc}", npc)
            .replace("{key}", key)
            .replace("{direction}", direction)
            .replace("{distance}", distance)
    }

    fn gate(&mut self) -> i32 {
        self.rng
            .random_range(self.input.config.rep_gate_min..=self.input.config.rep_gate_max)
    }

    /// Pick a passable tomb candidate; falls back to the first passable one.
    fn pick_tomb(&mut self) -> (u32, u32) {
        let mut candidates: Vec<(u32, u32)> = self
            .input
            .tomb_candidates
            .iter()
            .copied()
            .filter(|(x, y)| self.world.grid.passable_at(*x, *y))
            .collect();
        candidates.shuffle(&mut self.rng);
        candidates
            .first()
            .copied()
            .expect("at least one passable tomb candidate")
    }

    /// The spatial hint naming `tomb`: anchored on the nearest graph
    /// settlement (role order breaks ties; a settlement sitting on the tomb
    /// tile itself is a degenerate anchor and is skipped), relation and
    /// band measured from real geometry, confidence a pure function of the
    /// seed — never an RNG draw, so the solver's rng stream is untouched.
    fn hint_for_tomb(&self, tomb: (u32, u32), settlements: &[String]) -> SpatialHint {
        let anchor = settlements
            .iter()
            .filter_map(|s| self.world.settlement(s).map(|st| (s, (st.x, st.y))))
            .min_by_key(|(_, tile)| {
                let km = tile_distance_km(*tile, tomb, KM_PER_CELL);
                ((km == 0.0) as u8, (km * 1000.0) as u64)
            })
            .map(|(id, _)| id.clone())
            .expect("graph settlements exist on the world map");
        let anchor_tile = self
            .world
            .settlement(&anchor)
            .map(|s| (s.x, s.y))
            .expect("anchor settlement resolved above");
        let km = tile_distance_km(anchor_tile, tomb, KM_PER_CELL);
        let hash = self
            .seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .rotate_left(17);
        SpatialHint {
            origin: anchor,
            relation: relation_between(anchor_tile, tomb),
            band: DistanceBand::from_km(km),
            confidence: 0.75 + (hash % 20) as f64 / 100.0,
            target: HintTarget::Tomb,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    fn clue(
        id: &str,
        prereq_any: Vec<String>,
        gives: Option<&str>,
        requires: Option<&str>,
    ) -> Clue {
        Clue {
            id: id.to_string(),
            kind: ClueKind::Hint,
            settlement: "cairo".into(),
            npc: "npc".into(),
            rep_gate: 0,
            prereq: Vec::new(),
            prereq_any,
            gives_item: gives.map(str::to_string),
            requires_item: requires.map(str::to_string),
            text: id.to_string(),
            red_herring: false,
            contradicts_herring: None,
        }
    }

    fn graph_with(clues: BTreeMap<String, Clue>) -> ClueGraph {
        ClueGraph {
            clues,
            expedition: Expedition { tomb_tile: (0, 0) },
            leg_a: vec!["c1".into(), "c5".into()],
            leg_b: vec!["c1".into(), "c5".into()],
            start_clue: "c1".into(),
            location_clue: "c5".into(),
            spatial_hint: None,
            settlement_order: vec!["cairo".into()],
            settlement_display: BTreeMap::new(),
        }
    }

    #[test]
    fn solvability_respects_key_items() {
        // The location needs an item nobody grants: unsolvable.
        let unsolvable = graph_with(BTreeMap::from([
            ("c1".into(), clue("c1", Vec::new(), None, None)),
            (
                "c5".into(),
                clue("c5", vec!["e1".into()], None, Some("brass_key")),
            ),
            ("e1".into(), clue("e1", vec!["c1".into()], None, None)),
        ]));
        assert!(!unsolvable.is_solvable(&BTreeSet::new()));

        // Grant the key first: solvable.
        let solvable = graph_with(BTreeMap::from([
            ("c1".into(), clue("c1", Vec::new(), None, None)),
            (
                "e0".into(),
                clue("e0", vec!["c1".into()], Some("brass_key"), None),
            ),
            (
                "c5".into(),
                clue("c5", vec!["e1".into()], None, Some("brass_key")),
            ),
            ("e1".into(), clue("e1", vec!["e0".into()], None, None)),
        ]));
        assert!(solvable.is_solvable(&BTreeSet::new()));

        // Consuming the key-granting clue breaks it.
        assert!(!solvable.is_solvable(&BTreeSet::from(["e0".to_string()])));
    }

    #[test]
    fn relation_octants_are_compass_accurate() {
        // Origin at (5, 5); each octant's dominant direction is stable.
        let cases = [
            ((5u32, 5u32), (5u32, 0u32), CompassRelation::North),
            ((5, 5), (8, 2), CompassRelation::NorthEast),
            ((5, 5), (11, 6), CompassRelation::East),
            ((5, 5), (9, 9), CompassRelation::SouthEast),
            ((5, 5), (5, 10), CompassRelation::South),
            ((5, 5), (1, 9), CompassRelation::SouthWest),
            ((5, 5), (0, 5), CompassRelation::West),
            ((5, 5), (2, 2), CompassRelation::NorthWest),
        ];
        for (from, to, expected) in cases {
            assert_eq!(relation_between(from, to), expected, "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn distance_band_boundaries_match_sector_ranges() {
        // Distance partitions: [0,100) [100,250) [250,500) [500,1000] (1000,∞).
        for (km, band) in [
            (0.0, DistanceBand::ShortWalk),
            (99.9, DistanceBand::ShortWalk),
            (100.0, DistanceBand::HalfDay),
            (249.9, DistanceBand::HalfDay),
            (250.0, DistanceBand::OneDay),
            (499.9, DistanceBand::OneDay),
            (500.0, DistanceBand::TwoDays),
            (1000.0, DistanceBand::TwoDays),
            (1000.1, DistanceBand::LongJourney),
        ] {
            assert_eq!(DistanceBand::from_km(km), band, "km {km}");
        }
        // km_range agrees at the inclusive ends of each band.
        for band in [
            DistanceBand::ShortWalk,
            DistanceBand::HalfDay,
            DistanceBand::OneDay,
            DistanceBand::TwoDays,
        ] {
            let (min, max) = band.km_range();
            assert!(min < max);
            assert_eq!(DistanceBand::from_km(min), band, "{band:?} min {min}");
            // Upper ends are exclusive except TwoDays' (the 1000 km cap).
            if band == DistanceBand::TwoDays {
                assert_eq!(DistanceBand::from_km(max), band, "{band:?} max {max}");
            } else {
                assert_ne!(DistanceBand::from_km(max), band, "{band:?} max {max}");
            }
        }
        // LongJourney is (1000, ∞): its lower end is open.
        let (min, max) = DistanceBand::LongJourney.km_range();
        assert!(!max.is_finite());
        assert_eq!(DistanceBand::from_km(min + 1e-6), DistanceBand::LongJourney);
    }

    #[test]
    fn sector_excludes_anchor_and_other_bands() {
        let world = crate::content::load::load_world(Path::new("content")).unwrap();
        // Anchor on a real settlement; the tomb 20 tiles south-east.
        let anchor = world.settlement("asyut").unwrap();
        let (ax, ay) = (anchor.x, anchor.y);
        let hint = SpatialHint {
            origin: "asyut".into(),
            relation: relation_between((ax, ay), (ax + 10, ay + 10)),
            band: DistanceBand::from_km(tile_distance_km(
                (ax, ay),
                (ax + 10, ay + 10),
                KM_PER_CELL,
            )),
            confidence: 0.9,
            target: HintTarget::Tomb,
        };
        assert!(hint.sector_contains(&world, (ax + 10, ay + 10)));
        assert!(!hint.sector_contains(&world, (ax, ay)), "anchor itself");
        assert!(!hint.sector_contains(&world, (ax - 10, ay + 10)), "west");
        assert!(!hint.sector_contains(&world, (ax + 30, ay + 30)), "too far");
        // An exaggerated band must not swallow the whole map.
        let far = hint.clone();
        let mut far_hint = far;
        far_hint.band = DistanceBand::ShortWalk;
        assert!(!far_hint.sector_contains(&world, (ax + 10, ay + 10)));
    }
}
