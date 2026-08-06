//! Game orchestration (DESIGN.md §4.1): the expedition loop, headless.
//!
//! `Game` owns every subsystem of a run and advances them one `Input` at a
//! time in `apply`. World state at tick T is a pure function of
//! (seed, ordered inputs) — `apply` never consults wall-clock time; all
//! background activity (weather, market drift, rivals) is re-derived from
//! stateless seeded streams, so the whole game runs in `cargo test` and
//! replays stay byte-identical.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dialogue::Conversation;
use crate::dialogue::npc::{Effect, Npc};
use crate::save::Save;
use crate::sim::Input;
use crate::sim::clock::Tick;
use crate::sim::clues::{ClueGenerator, ClueGraph, GeneratorInput, SpatialHint};
use crate::sim::economy::{Goods, Market, MarketEntry, season_factor};
use crate::sim::events;
use crate::sim::player::{Direction, Player};
use crate::sim::rivals;
use crate::sim::survival::Survival;
use crate::world::{Settlement, World};

/// Start money of the expedition, in piastres.
pub const START_MONEY: u32 = 300;

/// All authored game content the simulation reads (DESIGN.md §7): goods,
/// markets, NPCs, and the clue-graph generator inputs. Loaded once by
/// `content::load`; validated by content lint (§9).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameContent {
    pub goods: BTreeMap<String, Goods>,
    pub markets: BTreeMap<String, Market>,
    pub npcs: BTreeMap<String, Npc>,
    /// Everything the seeded clue generator needs.
    pub generator: Option<GeneratorInput>,
}

impl GameContent {
    pub fn goods(&self, id: &str) -> Option<&Goods> {
        self.goods.get(id)
    }
}

/// One diary line: the deterministic auto-log of a run (DESIGN.md §4.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub tick: Tick,
    pub text: String,
}

/// Run outcome. `Dead` carries the in-fiction reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameStatus {
    Playing,
    Won,
    Dead(String),
}

/// A buried cache (DESIGN.md §4.10): items hidden at one tile, recovered at
/// the same tile. Plain struct; no location spells needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    pub x: u32,
    pub y: u32,
    pub items: BTreeMap<String, u32>,
}

/// What the UI needs to render one conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkScreen {
    pub npc_id: String,
    pub npc_name: String,
    /// The current node's line.
    pub text: String,
    /// Visible authored options (texts).
    pub options: Vec<String>,
    /// Whether "Ask about the tomb" is available as the last option.
    pub tomb_question: bool,
}

/// The whole state of a run. Everything except `content`/`graph` is
/// deterministic and serializable (a `Save` wraps it with the replay log);
/// the graph is a pure function of `seed` and is regenerated on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub seed: u64,
    pub tick: Tick,
    pub player: Player,
    pub survival: Survival,
    /// Settlement ID -> reputation.
    pub reputation: BTreeMap<String, i32>,
    /// Obtained clue IDs.
    pub clues: BTreeSet<String>,
    /// Herrings proven false (DESIGN.md §4.5).
    pub false_clues: BTreeSet<String>,
    /// Exclusive clues taken by rivals.
    pub rivals_consumed: BTreeSet<String>,
    pub diary: Vec<DiaryEntry>,
    pub caches: Vec<Cache>,
    pub status: GameStatus,
    /// Tomb tile once the player stands on it holding the location clue
    /// (None until then; WS2 discovery, EXPANSION_PLAN §WS2).
    pub tomb_revealed: Option<(u32, u32)>,
    /// Explored sim cells as sector masks over the map tiles
    /// (WS1 fog of war).
    pub explored_sectors: BTreeSet<(u32, u32)>,
    /// Labels learned from landmarks and regions; persists across
    /// saves and are re-interpreted by the diary (WS2).
    pub known_labels: BTreeSet<String>,
    /// Active conversation, if any.
    pub conversation: Option<Conversation>,
    /// The generated mystery of this run.
    pub graph: ClueGraph,
    pub content: GameContent,
}

impl Game {
    /// Start a fresh run: generate the mystery from the seed, begin at the
    /// world's start settlement with full stats and the starting purse.
    pub fn new(seed: u64, content: GameContent, world: &World) -> Game {
        let start = world
            .start()
            .expect("validated content has a start settlement");
        let mut player = Player::new(start.x, start.y);
        player.add_money(START_MONEY);
        let graph = generate_graph(seed, &content, world);
        Game {
            seed,
            tick: Tick::START,
            player,
            survival: Survival::new(),
            reputation: BTreeMap::new(),
            clues: BTreeSet::new(),
            false_clues: BTreeSet::new(),
            rivals_consumed: BTreeSet::new(),
            diary: Vec::new(),
            caches: Vec::new(),
            status: GameStatus::Playing,
            tomb_revealed: None,
            explored_sectors: BTreeSet::new(),
            known_labels: BTreeSet::new(),
            conversation: None,
            graph,
            content,
        }
    }

    /// Rebuild a game from a save (DESIGN.md §4.10), including the diary
    /// (the quest log). The clue graph is regenerated from the save's seed.
    pub fn from_save(save: Save, content: GameContent, world: &World) -> Game {
        let graph = generate_graph(save.seed, &content, world);
        Game {
            seed: save.seed,
            tick: save.tick,
            player: save.player,
            survival: save.survival,
            reputation: save.reputation,
            clues: save.clues,
            false_clues: save.false_clues,
            rivals_consumed: save.rivals_consumed,
            diary: save.diary,
            caches: save.caches,
            status: save.status,
            tomb_revealed: save.tomb_revealed,
            explored_sectors: save.explored_sectors,
            known_labels: save.known_labels,
            conversation: None,
            graph,
            content,
        }
    }

    /// Advance the game by one input. Returns human-readable events of this
    /// step (also appended to the diary); empty when the input did nothing.
    ///
    /// No-op while the run has ended (won or dead). The daily director runs
    /// once per in-game day CROSSED by the input (walk, rest, fast travel) —
    /// never repeatedly at midnight, never skipped by multi-day travel.
    pub fn apply(&mut self, input: &Input, world: &World) -> Vec<String> {
        if self.status != GameStatus::Playing {
            return Vec::new();
        }
        let day_before = self.tick.day();
        let mut events = match input {
            Input::Move(dir) => self.apply_move(*dir, world),
            Input::Rest => self.apply_rest(world),
            Input::Buy(id) => self.apply_buy(id, world),
            Input::Sell(id) => self.apply_sell(id, world),
            Input::Use(id) => self.apply_use(id, world),
            Input::Talk(npc_id, option) => self.apply_talk(npc_id, *option, world),
            Input::Bury(id) => self.apply_bury(id),
            Input::Retrieve(id) => self.apply_retrieve(id),
            Input::Travel(route_id) => self.apply_travel(route_id, world),
            Input::LlmAction { npc_id, effect } => self.apply_llm_action(npc_id, effect),
        };
        // The daily director: rivals move and consume exclusives — exactly
        // once per crossed day (DESIGN.md §4.8). Weather and market drift
        // stay pure functions of (seed, day), read on demand.
        if self.status == GameStatus::Playing {
            for day in (day_before + 1)..=self.tick.day() {
                events.extend(self.process_day(day));
            }
        }
        for text in &events {
            self.diary.push(DiaryEntry {
                tick: self.tick,
                text: text.clone(),
            });
        }
        events
    }

    /// A full serializable snapshot plus the replay log (DESIGN.md §4.10).
    pub fn snapshot(&self, input_log: Vec<Input>) -> Save {
        Save {
            version: crate::save::SAVE_VERSION,
            seed: self.seed,
            tick: self.tick,
            player: self.player.clone(),
            survival: self.survival.clone(),
            reputation: self.reputation.clone(),
            clues: self.clues.clone(),
            false_clues: self.false_clues.clone(),
            rivals_consumed: self.rivals_consumed.clone(),
            diary: self.diary.clone(),
            caches: self.caches.clone(),
            tomb_revealed: self.tomb_revealed,
            explored_sectors: self.explored_sectors.clone(),
            known_labels: self.known_labels.clone(),
            status: self.status.clone(),
            input_log,
        }
    }

    /// The settlement the player is standing in, if any.
    pub fn settlement_at<'a>(&self, world: &'a World) -> Option<&'a Settlement> {
        let (x, y) = self.player.pos();
        world.settlements.iter().find(|s| s.x == x && s.y == y)
    }

    /// Reputation in a settlement (0 when unknown).
    pub fn reputation(&self, settlement: &str) -> i32 {
        self.reputation.get(settlement).copied().unwrap_or(0)
    }

    /// Routes that can be ridden from the current settlement (scouted and
    /// reachable), for the UI.
    pub fn routes_here<'a>(&'a self, world: &'a World) -> Vec<&'a crate::world::Route> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        world
            .routes
            .iter()
            .filter(|r| {
                (r.from == settlement.id || r.to == settlement.id) && self.route_scouted(r, world)
            })
            .collect()
    }

    fn apply_move(&mut self, dir: Direction, world: &World) -> Vec<String> {
        if self.player.try_move(&world.grid, dir).is_none() {
            return Vec::new();
        }
        // One tile costs its biome's movement cost in in-game hours (the
        // data's costs are real: floodplain 2, mountain 3, open ground 1).
        let (x, y) = self.player.pos();
        let cost = world.grid.movement_cost_at(x, y) as u64;
        for _ in 0..cost {
            self.advance_travel(world);
        }
        self.after_movement();
        Vec::new()
    }

    /// One in-game hour of travel — the single per-hour rule shared by
    /// walking and caravan travel: clock, survival drain, and the day's
    /// weather (DESIGN.md §3.3).
    pub(crate) fn advance_travel(&mut self, world: &World) {
        self.tick.advance();
        let (x, y) = self.player.pos();
        let biome = world.grid.biome_id_at(x, y);
        self.survival
            .on_travel(biome, self.tick.hour_of_day(), self.tick.month());
        match events::weather(self.seed, self.tick.day()) {
            events::Weather::Heatwave if biome == "desert" || biome == "savanna" => {
                self.survival.hydration = (self.survival.hydration - 0.05).max(0.0);
            }
            events::Weather::Duststorm => {
                self.survival.rest = (self.survival.rest - 0.05).max(0.0);
            }
            _ => {}
        }
    }

    fn apply_rest(&mut self, world: &World) -> Vec<String> {
        self.tick.advance();
        let at_settlement = self.settlement_at(world).is_some();
        let has_tent = self.player.item_count("tent") > 0;
        self.survival
            .on_rest(self.tick.hour_of_day(), at_settlement, has_tent);
        self.after_movement();
        Vec::new()
    }

    /// Shared post-movement bookkeeping: death, tomb discovery, win checks.
    pub(crate) fn after_movement(&mut self) {
        if let Some(reason) = self.survival.cause_of_death() {
            self.die(reason);
        } else {
            // WS2: the location clue names a region, not a tile. The tomb
            // is discovered by standing on it — the spatial hint (diary
            // text) is what leads a player here (EXPANSION_PLAN §WS2).
            if self.tomb_revealed.is_none()
                && self.clues.contains(&self.graph.location_clue)
                && self.player.pos() == self.graph.expedition.tomb_tile
            {
                self.tomb_revealed = Some(self.graph.expedition.tomb_tile);
                self.diary.push(DiaryEntry {
                    tick: self.tick,
                    text: "The cleft in the red rock opens before you — the hills split apart, and the way down is clear."
                        .to_string(),
                });
            }
            self.check_win();
        }
    }

    /// The semantic content of the location clue, once it is obtained
    /// (None before). A pure function of the seed — never serialized.
    pub fn tomb_hint(&self) -> Option<SpatialHint> {
        if self.clues.contains(&self.graph.location_clue) {
            self.graph.spatial_hint.clone()
        } else {
            None
        }
    }

    /// The authoritative buy price at a settlement: base × season × drifted
    /// demand × reputation (DESIGN.md §4.4). The UI displays this and
    /// `apply_buy` charges it — one price, one source of truth.
    pub fn buy_price(&self, settlement: &str, id: &str) -> Option<u32> {
        let goods = self.content.goods(id)?;
        let market = self.content.markets.get(settlement)?;
        if !market.trades(id) {
            return None;
        }
        let entry = self.drifted_entry(market, id);
        Some(Market::buy_price_with(
            goods,
            season_factor(self.tick.month()),
            self.reputation(settlement),
            entry.demand,
        ))
    }

    /// The authoritative sell price at a settlement (see `buy_price`).
    pub fn sell_price(&self, settlement: &str, id: &str) -> Option<u32> {
        let goods = self.content.goods(id)?;
        let market = self.content.markets.get(settlement)?;
        if !market.trades(id) {
            return None;
        }
        let entry = self.drifted_entry(market, id);
        Some(Market::sell_price_with(
            goods,
            season_factor(self.tick.month()),
            self.reputation(settlement),
            entry.supply,
        ))
    }

    fn apply_buy(&mut self, id: &str, world: &World) -> Vec<String> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        let Some(goods) = self.content.goods(id) else {
            return Vec::new();
        };
        let Some(price) = self.buy_price(&settlement.id, id) else {
            return Vec::new();
        };
        if !self.player.spend(price) {
            return Vec::new();
        }
        self.player.add_item(id);
        vec![format!(
            "Bought 1 {} for {} piastres in {}.",
            goods.name, price, settlement.name
        )]
    }

    fn apply_sell(&mut self, id: &str, world: &World) -> Vec<String> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        let Some(goods) = self.content.goods(id) else {
            return Vec::new();
        };
        let Some(price) = self.sell_price(&settlement.id, id) else {
            return Vec::new();
        };
        if !self.player.remove_item(id) {
            return Vec::new();
        }
        self.player.add_money(price);
        vec![format!(
            "Sold 1 {} for {} piastres in {}.",
            goods.name, price, settlement.name
        )]
    }

    /// The supply/demand entry of an item, with the day's market drift.
    fn drifted_entry(&self, market: &Market, id: &str) -> MarketEntry {
        let base = market.items.get(id).copied().unwrap_or(MarketEntry {
            demand: 1.0,
            supply: 1.0,
        });
        MarketEntry {
            demand: events::drifted(self.seed, self.tick.day(), id, base.demand),
            supply: events::drifted(self.seed, self.tick.day(), id, base.supply),
        }
    }

    fn apply_use(&mut self, id: &str, world: &World) -> Vec<String> {
        // Cloned: goods data is small, and the item effect may trigger a
        // mutable borrow of self afterwards.
        let Some(goods) = self.content.goods(id).cloned() else {
            return Vec::new();
        };
        // Only consumables are consumed: using a clue item (or any good
        // without an effect) is a no-op, so the pack can't be wasted
        // through the sim path (the UI already hides the button).
        if goods.restores_hydration <= 0.0
            && goods.restores_nutrition <= 0.0
            && goods.cures.is_none()
        {
            return Vec::new();
        }
        if !self.player.remove_item(id) {
            return Vec::new();
        }
        let (x, y) = self.player.pos();
        let biome = world.grid.biome_id_at(x, y);
        if goods.restores_hydration > 0.0 {
            if id == "water" || id == "boiled_water" {
                // Unboiled water in the floodplain risks dysentery; boiled
                // water relieves it (DESIGN.md §4.3).
                self.survival
                    .drink(id == "boiled_water", biome == "floodplain");
            } else {
                self.survival.hydration =
                    (self.survival.hydration + goods.restores_hydration).min(100.0);
            }
        }
        if goods.restores_nutrition > 0.0 {
            self.survival.eat(goods.restores_nutrition);
        }
        if let Some(ailment) = goods.cures {
            self.survival.cure(ailment);
        }
        self.after_movement();
        vec![format!("Used 1 {}.", goods.name)]
    }

    /// Apply a validated LLM action (DESIGN.md §6.4). Re-validated on apply
    /// so replaying the input log is byte-identical: the only permitted
    /// effect is revealing the NPC's pending clue, gated exactly like the
    /// scripted tomb question.
    fn apply_llm_action(&mut self, npc_id: &str, effect: &Effect) -> Vec<String> {
        let Some(clue_id) = &effect.reveal_clue else {
            return Vec::new();
        };
        let Some(npc) = self.content.npcs.get(npc_id).cloned() else {
            return Vec::new();
        };
        let Some(pending) = self.pending_clue_of_npc_inner(npc_id).map(|c| c.id.clone()) else {
            return Vec::new();
        };
        if *clue_id != pending {
            return Vec::new();
        }
        self.apply_effect(effect, &npc)
    }

    /// Apply one scripted effect — the closed action set of §6.4.
    pub(crate) fn apply_effect(&mut self, effect: &Effect, npc: &Npc) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(clue_id) = &effect.reveal_clue
            && let Some(clue) = self.graph.clues.get(clue_id)
            && self.clues.insert(clue_id.clone())
        {
            if let Some(item) = &clue.gives_item {
                self.player.add_item(item);
            }
            // Falsification: obtaining the contradiction proves herrings false
            // (DESIGN.md §4.5: discoverable as false through corroboration).
            for herring in self.graph.clues.values() {
                if herring.red_herring
                    && herring.contradicts_herring.as_deref() == Some(clue_id.as_str())
                    && self.clues.contains(&herring.id)
                    && self.false_clues.insert(herring.id.clone())
                {
                    events.push(format!(
                        "The rumor proved false: \"{}\" was a lie.",
                        herring.text
                    ));
                }
            }
            events.push(format!("Clue recorded ({}): {}", clue.id, clue.text));
        }
        if let Some(item) = &effect.take_item {
            self.player.remove_item(item);
        }
        if let Some(item) = &effect.give_item {
            self.player.add_item(item);
        }
        if let Some(delta) = effect.rep_delta {
            let rep = self.reputation.entry(npc.settlement.clone()).or_insert(0);
            *rep += delta;
            events.push(format!("Your standing with {} has changed.", npc.name));
        }
        events
    }

    fn apply_bury(&mut self, id: &str) -> Vec<String> {
        if !self.player.remove_item(id) {
            return Vec::new();
        }
        let (x, y) = self.player.pos();
        if let Some(cache) = self.caches.iter_mut().find(|c| c.x == x && c.y == y) {
            *cache.items.entry(id.to_string()).or_insert(0) += 1;
        } else {
            self.caches.push(Cache {
                x,
                y,
                items: BTreeMap::from([(id.to_string(), 1)]),
            });
        }
        let name = self
            .content
            .goods(id)
            .map_or(id.to_string(), |g| g.name.clone());
        vec![format!("Buried 1 {name} here.")]
    }

    fn apply_retrieve(&mut self, id: &str) -> Vec<String> {
        let (x, y) = self.player.pos();
        let Some(cache) = self.caches.iter_mut().find(|c| c.x == x && c.y == y) else {
            return Vec::new();
        };
        let Some(count) = cache.items.get_mut(id) else {
            return Vec::new();
        };
        if *count == 0 {
            return Vec::new();
        }
        *count -= 1;
        if *count == 0 {
            cache.items.remove(id);
        }
        self.player.add_item(id);
        let name = self
            .content
            .goods(id)
            .map_or(id.to_string(), |g| g.name.clone());
        vec![format!("Recovered 1 {name} from the cache.")]
    }

    /// The daily director for one specific in-game day: rivals move and
    /// consume exclusives (DESIGN.md §4.8). Called exactly once per crossed
    /// day from `apply`.
    fn process_day(&mut self, day: u64) -> Vec<String> {
        let mut events = Vec::new();
        let order = &self.graph.settlement_order;
        let rivals_names: Vec<String> = self
            .content
            .generator
            .as_ref()
            .map(|g| g.config.rivals.clone())
            .unwrap_or_default();

        for (i, name) in rivals_names.iter().enumerate() {
            let pos = rivals::position(day, i, order);
            let display = self
                .graph
                .settlement_display
                .get(pos)
                .cloned()
                .unwrap_or_else(|| pos.to_string());
            if rivals::arrives_today(day) {
                events.push(format!("Rumor: {name} was seen in {display}."));
                // Rival consumption: an exclusive clue here, if taking it
                // keeps at least one complete path open (the solvability
                // proof, DESIGN.md §9). Consumption is gated behind a grace
                // window so the player can race the rival on foot first
                // (DESIGN.md §4.8); before then the rival only spread the
                // rumor above.
                if day < rivals::CONSUME_GRACE_DAY {
                    continue;
                }
                let exclusives: Vec<String> = self
                    .graph
                    .clues
                    .values()
                    .filter(|c| c.settlement == pos && is_exclusive(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
                for clue_id in exclusives {
                    if self.rivals_consumed.contains(&clue_id) || self.clues.contains(&clue_id) {
                        continue;
                    }
                    let mut would = self.rivals_consumed.clone();
                    would.insert(clue_id.clone());
                    if !self.graph.is_solvable(&would) {
                        continue;
                    }
                    self.rivals_consumed.insert(clue_id.clone());
                    let clue = &self.graph.clues[&clue_id];
                    events.push(format!(
                        "Rumor: {name} has taken what {npc_name} held at {display}.",
                        npc_name = self
                            .npc_display(&clue.npc)
                            .unwrap_or_else(|| clue.npc.clone())
                    ));
                    break;
                }
            }
        }
        events
    }

    fn npc_display(&self, npc_id: &str) -> Option<String> {
        self.content.npcs.get(npc_id).map(|n| n.name.clone())
    }

    fn die(&mut self, reason: String) {
        self.status = GameStatus::Dead(reason.clone());
        self.diary.push(DiaryEntry {
            tick: self.tick,
            text: format!("The expedition {reason}."),
        });
    }

    fn check_win(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }
        // Win requires the tomb location clue plus a complete leg of
        // corroborating clues (§4.5) — and being at the tomb.
        if self.tomb_revealed == Some(self.graph.expedition.tomb_tile)
            && self.graph.win_condition_met(&self.clues)
            && self.player.pos() == self.graph.expedition.tomb_tile
        {
            self.status = GameStatus::Won;
            self.diary.push(DiaryEntry {
                tick: self.tick,
                text: "The cleft in the red rock opens before you — the tomb of Ahnk-Ahnk. You have found it."
                    .to_string(),
            });
        }
    }
}

impl crate::dialogue::DialogueView for Game {
    fn reputation(&self, settlement: &str) -> i32 {
        self.reputation(settlement)
    }
    fn has_item(&self, item_id: &str) -> bool {
        self.player.item_count(item_id) > 0
    }
    fn has_clue(&self, clue_id: &str) -> bool {
        self.clues.contains(clue_id)
    }
}

/// The exclusive (leg) clue ids the rivals may consume.
fn is_exclusive(id: &str) -> bool {
    matches!(id, "e1" | "e2" | "f1" | "f2")
}

/// Generate the mystery for a seed; panics only on generator/content bugs
/// (the solvability proof is asserted inside the generator).
fn generate_graph(seed: u64, content: &GameContent, world: &World) -> ClueGraph {
    let input = content
        .generator
        .as_ref()
        .expect("validated content has a clue generator");
    ClueGenerator::new(seed, input, world, &content.npcs).generate()
}
