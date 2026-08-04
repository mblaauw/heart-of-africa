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

use crate::dialogue::npc::{Effect, Npc};
use crate::dialogue::{Conversation, GameSnapshot};
use crate::save::Save;
use crate::sim::Input;
use crate::sim::clock::Tick;
use crate::sim::clues::{Clue, ClueGenerator, ClueGraph, ClueKind, GeneratorInput};
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
    /// Tomb tile once the location clue names it (None until then).
    pub tomb_revealed: Option<(u32, u32)>,
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
            conversation: None,
            graph,
            content,
        }
    }

    /// Rebuild a game from a save (DESIGN.md §4.10). The diary is not part
    /// of the save (it is derivable from the input log); it starts empty.
    /// The clue graph is regenerated from the save's seed.
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
            diary: Vec::new(),
            caches: save.caches,
            status: save.status,
            tomb_revealed: save.tomb_revealed,
            conversation: None,
            graph,
            content,
        }
    }

    /// Advance the game by one input. Returns human-readable events of this
    /// step (also appended to the diary); empty when the input did nothing.
    ///
    /// No-op while the run has ended (won or dead).
    pub fn apply(&mut self, input: &Input, world: &World) -> Vec<String> {
        if self.status != GameStatus::Playing {
            return Vec::new();
        }
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
        };
        // The daily director: weather, market drift, rivals (DESIGN.md §4.8).
        if self.tick.hour_of_day() == 0 && self.status == GameStatus::Playing {
            events.extend(self.daily());
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
            seed: self.seed,
            tick: self.tick,
            player: self.player.clone(),
            survival: self.survival.clone(),
            reputation: self.reputation.clone(),
            clues: self.clues.clone(),
            false_clues: self.false_clues.clone(),
            rivals_consumed: self.rivals_consumed.clone(),
            caches: self.caches.clone(),
            tomb_revealed: self.tomb_revealed,
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

    /// The current conversation turn, or None.
    pub fn talk_screen(&self) -> Option<TalkScreen> {
        let conv = self.conversation.as_ref()?;
        let npc = self.content.npcs.get(&conv.npc_id)?;
        let snapshot = self.snapshot_view();
        let options = conv
            .visible_options(npc, &snapshot)
            .iter()
            .map(|o| o.text.clone())
            .collect();
        Some(TalkScreen {
            npc_id: conv.npc_id.clone(),
            npc_name: npc.name.clone(),
            text: conv.node(npc).text.clone(),
            options,
            tomb_question: self.tomb_question_available(npc),
        })
    }

    /// The NPCs present at the current settlement, for the UI.
    pub fn npcs_here<'a>(&'a self, world: &'a World) -> Vec<&'a Npc> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        self.content
            .npcs
            .values()
            .filter(|npc| npc.settlement == settlement.id)
            .collect()
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
        self.tick.advance();
        let (x, y) = self.player.pos();
        let biome = world.grid.biome_id_at(x, y);
        self.survival
            .on_travel(biome, self.tick.hour_of_day(), self.tick.month());
        // Weather affects travel (DESIGN.md §3.3): telegraphed, deterministic.
        match events::weather(self.seed, self.tick.day()) {
            events::Weather::Heatwave if biome == "desert" || biome == "savanna" => {
                self.survival.hydration = (self.survival.hydration - 0.05).max(0.0);
            }
            events::Weather::Duststorm => {
                self.survival.rest = (self.survival.rest - 0.05).max(0.0);
            }
            _ => {}
        }
        self.after_movement();
        Vec::new()
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

    /// Shared post-movement bookkeeping: death and win checks.
    fn after_movement(&mut self) {
        if let Some(reason) = self.survival.cause_of_death() {
            self.die(reason);
        } else {
            self.check_win();
        }
    }

    fn apply_buy(&mut self, id: &str, world: &World) -> Vec<String> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        let Some(goods) = self.content.goods(id) else {
            return Vec::new();
        };
        let Some(market) = self.content.markets.get(&settlement.id) else {
            return Vec::new();
        };
        if !market.trades(id) {
            return Vec::new();
        }
        let entry = self.drifted_entry(market, id);
        let season = season_factor(self.tick.month());
        let rep = self.reputation(&settlement.id);
        let price = Market::buy_price_with(goods, season, rep, entry.demand);
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
        let Some(market) = self.content.markets.get(&settlement.id) else {
            return Vec::new();
        };
        if !market.trades(id) || !self.player.remove_item(id) {
            return Vec::new();
        }
        let entry = self.drifted_entry(market, id);
        let season = season_factor(self.tick.month());
        let rep = self.reputation(&settlement.id);
        let price = Market::sell_price_with(goods, season, rep, entry.supply);
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
        if !self.player.remove_item(id) {
            return Vec::new();
        }
        // Cloned: goods data is small, and the item effect may trigger a
        // mutable borrow of self afterwards.
        let Some(goods) = self.content.goods(id).cloned() else {
            return Vec::new();
        };
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

    fn apply_talk(&mut self, npc_id: &str, option_index: usize, world: &World) -> Vec<String> {
        let Some(npc) = self.content.npcs.get(npc_id).cloned() else {
            return Vec::new();
        };
        // Conversations happen in the NPC's settlement.
        if self.settlement_at(world).map(|s| s.id.as_str()) != Some(npc.settlement.as_str()) {
            return Vec::new();
        }
        let mut events = Vec::new();

        // First word (or switching NPCs) opens the conversation.
        if self
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            self.conversation = Some(Conversation::start(npc_id.to_string()));
            let conv = self.conversation.as_ref().expect("just started");
            events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
            return events;
        }

        let authored = self
            .conversation
            .as_ref()
            .expect("conversation exists")
            .visible_options(&npc, &self.snapshot_view())
            .len();

        if option_index == authored {
            // The runtime tomb question (last option).
            if !self.tomb_question_available(&npc) {
                return Vec::new();
            }
            events.extend(self.ask_tomb(&npc));
            if !npc.tomb_answer.is_empty() {
                self.conversation
                    .as_mut()
                    .expect("conversation exists")
                    .jump_to(npc.tomb_answer.clone());
                let conv = self.conversation.as_ref().expect("just moved");
                events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
            } else {
                self.conversation = None;
            }
            return events;
        }

        // An authored option.
        let (effect, ended) = {
            let snapshot = self.snapshot_view();
            let conv = self.conversation.as_mut().expect("conversation exists");
            match conv.choose(&npc, &snapshot, option_index) {
                Some(option) => (option.effect, conv.ended()),
                None => return Vec::new(),
            }
        };
        events.extend(self.apply_effect(&effect, &npc));
        if ended {
            self.conversation = None;
        } else {
            let conv = self.conversation.as_ref().expect("not ended");
            events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
        }
        events
    }

    /// Whether the runtime "ask about the tomb" question is available from
    /// this NPC: they hold a graph clue that is not yet obtained, not taken
    /// by a rival, with prerequisites and reputation met.
    fn tomb_question_available(&self, npc: &Npc) -> bool {
        self.pending_clue_of_npc_inner(&npc.id).is_some()
    }

    /// The clue this NPC can currently reveal. An NPC may hold several clues
    /// (e.g. a herring next to a real one); the first askable one in id
    /// order wins: not obtained, not consumed by a rival, prerequisites and
    /// rep gate met.
    fn pending_clue_of_npc_inner(&self, npc_id: &str) -> Option<&Clue> {
        self.graph
            .clues
            .values()
            .filter(|c| c.npc == npc_id)
            .find(|c| {
                !self.clues.contains(&c.id)
                    && !self.rivals_consumed.contains(&c.id)
                    && self.graph.prereqs_met(&c.id, &self.clues)
                    && self.reputation(&c.settlement) >= c.rep_gate
            })
    }

    /// Ask about the tomb: reveal the NPC's pending clue (the one runtime
    /// action the seeded graph controls).
    fn ask_tomb(&mut self, npc: &Npc) -> Vec<String> {
        let Some(clue_id) = self
            .pending_clue_of_npc_inner(&npc.id)
            .map(|c| c.id.clone())
        else {
            return Vec::new();
        };
        let effect = Effect {
            reveal_clue: Some(clue_id),
            ..Default::default()
        };
        self.apply_effect(&effect, npc)
    }

    /// Apply one scripted effect — the closed action set of §6.4.
    fn apply_effect(&mut self, effect: &Effect, npc: &Npc) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(clue_id) = &effect.reveal_clue
            && let Some(clue) = self.graph.clues.get(clue_id)
            && self.clues.insert(clue_id.clone())
        {
            if let Some(item) = &clue.gives_item {
                self.player.add_item(item);
            }
            if clue.kind == ClueKind::Location {
                self.tomb_revealed = Some(self.graph.expedition.tomb_tile);
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

    /// Caravan fast-travel (DESIGN.md §4.2): time + supply cost; encounter
    /// roll only on routes flagged risky (telegraphed before departure).
    fn apply_travel(&mut self, route_id: &str, world: &World) -> Vec<String> {
        let Some(route) = world.routes.iter().find(|r| r.id == route_id) else {
            return Vec::new();
        };
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        let (from_id, to_id) = if route.from == settlement.id {
            (route.from.as_str(), route.to.as_str())
        } else if route.to == settlement.id {
            (route.to.as_str(), route.from.as_str())
        } else {
            return Vec::new();
        };
        if !self.route_scouted(route, world) {
            return Vec::new();
        }
        let to = world.settlement(to_id).expect("validated route endpoint");
        let from = world.settlement(from_id).expect("validated route endpoint");
        let hours = (from.x.abs_diff(to.x) + from.y.abs_diff(to.y)) * 2;

        self.tick.advance_by(hours as u64);
        self.survival.hydration = (self.survival.hydration - hours as f32 * 0.1).max(0.0);
        self.survival.nutrition = (self.survival.nutrition - hours as f32 * 0.1).max(0.0);
        self.player.mark_visited(to.x, to.y);
        self.player.teleport(to.x, to.y);

        let mut events = vec![format!(
            "Caravan to {} — {} hours on the road.",
            to.name, hours
        )];
        if route.risky && events::encounter(self.seed, self.tick.as_hours()) {
            events.push(self.ambush());
        }
        self.after_movement();
        events
    }

    /// A telegraphed risky-route encounter: lose some supplies or money.
    fn ambush(&mut self) -> String {
        // Lose the first carried item, else some money.
        let victim = self.player.items().keys().next().cloned();
        if let Some(id) = victim {
            self.player.remove_item(&id);
            let name = self
                .content
                .goods(&id)
                .map_or(id.clone(), |g| g.name.clone());
            format!("Waylaid on the road — the bandits took your {name}.")
        } else {
            let loss = 10;
            self.player.spend(loss);
            format!("Waylaid on the road — the bandits took {loss} piastres.")
        }
    }

    /// A route is scouted when both endpoint settlements have been visited.
    fn route_scouted(&self, route: &crate::world::Route, world: &World) -> bool {
        let Some(from) = world.settlement(&route.from) else {
            return false;
        };
        let Some(to) = world.settlement(&route.to) else {
            return false;
        };
        self.player.has_visited(from.x, from.y) && self.player.has_visited(to.x, to.y)
    }

    /// The daily director: rivals move and consume exclusives (DESIGN.md
    /// §4.8). Weather and market drift are pure functions of (seed, day).
    fn daily(&mut self) -> Vec<String> {
        let day = self.tick.day();
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
                // proof, DESIGN.md §9).
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

    /// An owned view of the game state for the dialogue runner.
    fn snapshot_view(&self) -> GameSnapshot {
        GameSnapshot {
            reputation: self.reputation.clone(),
            items: self.player.items().clone(),
            clues: self.clues.clone(),
        }
    }

    /// The clue currently obtainable at an NPC, for the bot/UI.
    pub fn pending_clue_of_npc(&self, npc_id: &str) -> Option<&Clue> {
        self.pending_clue_of_npc_inner(npc_id)
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
