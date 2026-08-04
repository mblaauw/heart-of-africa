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
use crate::world::World;

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
    /// location clue still reachable? Honoring prerequisites only (rep gates
    /// are surmountable by the player).
    pub fn is_solvable(&self, consumed: &BTreeSet<String>) -> bool {
        // Reachability fixpoint over the graph from the start clue.
        let mut reachable: BTreeSet<String> = BTreeSet::new();
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
                if all_met && any_met {
                    reachable.insert(clue.id.clone());
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
                    &display_of[settlement],
                    &npc_of[settlement],
                    &key_name,
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
            vec![f1.id.clone()],
            Vec::new(),
            f2_gate,
            None,
            None,
            false,
            None,
        );

        // Location clue at the end settlement, fed by EITHER leg (OR-prereq).
        let c5 = clue(
            "c5",
            ClueKind::Location,
            &end_id,
            Vec::new(),
            vec![e2.id.clone(), f2.id.clone()],
            c5_gate,
            None,
            None,
            false,
            None,
        );
        let location_clue = c5.id.clone();
        clues.insert(e1.id.clone(), e1);
        clues.insert(e2.id.clone(), e2);
        clues.insert(f1.id.clone(), f1);
        clues.insert(f2.id.clone(), f2);
        clues.insert(c5.id.clone(), c5);

        // Red herrings: falsifiable by obtaining e2 (the leg-A corroboration).
        for (i, settlement) in herring_settlements.iter().enumerate() {
            let herring = clue(
                &format!("h{}", i + 1),
                ClueKind::Hint,
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

    fn render(&self, template: &ClueTemplate, settlement: &str, npc: &str, key: &str) -> String {
        template
            .text
            .replace("{settlement}", settlement)
            .replace("{npc}", npc)
            .replace("{key}", key)
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
}
