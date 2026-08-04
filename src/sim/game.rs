//! Game orchestration (DESIGN.md §4.1): the expedition loop, headless.
//!
//! `Game` owns every subsystem of a run and advances them one `Input` at a
//! time in `apply`. World state at tick T is a pure function of
//! (seed, ordered inputs) — `apply` never consults a clock or RNG, so the
//! whole game runs in `cargo test`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dialogue::npc::{DialogueOption, Effect, Npc};
use crate::dialogue::{Conversation, GameSnapshot};
use crate::save::Save;
use crate::sim::Input;
use crate::sim::clock::Tick;
use crate::sim::clues::{ClueKind, Expedition};
use crate::sim::economy::{Goods, Market, season_factor};
use crate::sim::player::{Direction, Player};
use crate::sim::survival::Survival;
use crate::world::{Settlement, World};

/// Start money of the expedition, in piastres.
pub const START_MONEY: u32 = 300;

/// All authored game content the simulation reads (DESIGN.md §7): goods,
/// markets, clues, NPCs, and the expedition win condition. Loaded once by
/// `content::load`; validated by content lint (§9).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameContent {
    pub goods: BTreeMap<String, Goods>,
    pub markets: BTreeMap<String, Market>,
    pub clues: BTreeMap<String, crate::sim::clues::Clue>,
    pub npcs: BTreeMap<String, Npc>,
    pub expedition: Option<Expedition>,
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

/// The whole state of a run. Everything here is deterministic and
/// serializable (a `Save` wraps it with the replay log).
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
    pub diary: Vec<DiaryEntry>,
    pub caches: Vec<Cache>,
    pub status: GameStatus,
    /// Tomb tile once the location clue names it (None until then).
    pub tomb_revealed: Option<(u32, u32)>,
    /// Active conversation, if any.
    pub conversation: Option<Conversation>,
    pub content: GameContent,
}

impl Game {
    /// Start a fresh run: the expedition begins at the world's start
    /// settlement with full stats and the starting purse.
    pub fn new(seed: u64, content: GameContent, world: &World) -> Game {
        let start = world
            .start()
            .expect("validated content has a start settlement");
        let mut player = Player::new(start.x, start.y);
        player.add_money(START_MONEY);
        Game {
            seed,
            tick: Tick::START,
            player,
            survival: Survival::new(),
            reputation: BTreeMap::new(),
            clues: BTreeSet::new(),
            diary: Vec::new(),
            caches: Vec::new(),
            status: GameStatus::Playing,
            tomb_revealed: None,
            conversation: None,
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
        let events = match input {
            Input::Move(dir) => self.apply_move(*dir, world),
            Input::Rest => self.apply_rest(world),
            Input::Buy(id) => self.apply_buy(id, world),
            Input::Sell(id) => self.apply_sell(id, world),
            Input::Use(id) => self.apply_use(id, world),
            Input::Talk(npc_id, option) => self.apply_talk(npc_id, *option, world),
            Input::Bury(id) => self.apply_bury(id),
            Input::Retrieve(id) => self.apply_retrieve(id),
        };
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

    fn apply_move(&mut self, dir: Direction, world: &World) -> Vec<String> {
        let events = Vec::new();
        if self.player.try_move(&world.grid, dir).is_none() {
            return events;
        }
        self.tick.advance();
        let (x, y) = self.player.pos();
        let biome = world.grid.biome_id_at(x, y);
        self.survival
            .on_travel(biome, self.tick.hour_of_day(), self.tick.month());
        if let Some(reason) = self.survival.cause_of_death() {
            self.die(reason);
        } else {
            self.check_win();
        }
        events
    }

    fn apply_rest(&mut self, world: &World) -> Vec<String> {
        self.tick.advance();
        let at_settlement = self.settlement_at(world).is_some();
        let has_tent = self.player.item_count("tent") > 0;
        self.survival
            .on_rest(self.tick.hour_of_day(), at_settlement, has_tent);
        if let Some(reason) = self.survival.cause_of_death() {
            self.die(reason);
        }
        Vec::new()
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
        let price = market.buy_price(
            goods,
            season_factor(self.tick.month()),
            self.reputation(&settlement.id),
        );
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
        let price = market.sell_price(
            goods,
            season_factor(self.tick.month()),
            self.reputation(&settlement.id),
        );
        self.player.add_money(price);
        vec![format!(
            "Sold 1 {} for {} piastres in {}.",
            goods.name, price, settlement.name
        )]
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
        if let Some(reason) = self.survival.cause_of_death() {
            self.die(reason);
        }
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

        let snapshot = GameSnapshot {
            reputation: self.reputation.clone(),
            items: self.player.items().clone(),
            clues: self.clues.clone(),
        };
        let (effect, ended) = {
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

    /// Apply one scripted effect — the closed action set of §6.4.
    fn apply_effect(&mut self, effect: &Effect, npc: &Npc) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(clue_id) = &effect.reveal_clue
            && let Some(clue) = self.content.clues.get(clue_id)
            && self.clues.insert(clue_id.clone())
        {
            if let Some(item) = &clue.gives_item {
                self.player.add_item(item);
            }
            if clue.kind == ClueKind::Location
                && let Some(exp) = &self.content.expedition
            {
                self.tomb_revealed = Some(exp.tomb_tile);
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

    /// All dialogue options the player can currently see, for the UI.
    pub fn visible_talk_options(&self) -> Vec<(String, Vec<DialogueOption>)> {
        let Some(conv) = &self.conversation else {
            return Vec::new();
        };
        let Some(npc) = self.content.npcs.get(&conv.npc_id) else {
            return Vec::new();
        };
        let snapshot = GameSnapshot {
            reputation: self.reputation.clone(),
            items: self.player.items().clone(),
            clues: self.clues.clone(),
        };
        let options = conv
            .visible_options(npc, &snapshot)
            .iter()
            .map(|o| (*o).clone())
            .collect();
        vec![(conv.npc_id.clone(), options)]
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
        let Some(exp) = &self.content.expedition else {
            return;
        };
        // Win requires the tomb location clue (tomb_revealed) plus the full
        // corroboration set — searching without them is not enough (§4.5).
        if self.tomb_revealed == Some(exp.tomb_tile)
            && self.clues.len() >= exp.win_clues
            && self.player.pos() == exp.tomb_tile
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
