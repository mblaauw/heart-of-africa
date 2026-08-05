//! The player-view sim bot: completes runs using ONLY what a human player
//! sees — diary prose (clue texts name the next settlement), visible
//! dialogue options, residents, the market, and the revealed tomb marker.
//!
//! It never reads `game.graph`, `game.clues`, `false_clues`,
//! `rivals_consumed`, or any hidden flag. This is the §10.3
//! in-game-information acceptance: the oracle bot (bot.rs) proves the graph
//! is traversable; this bot proves a player can follow it.
//!
//! State lives here (target settlement, worked residents, explored places),
//! because "what have I tried" is player memory, not game data.

use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::player::Direction;
use heart_of_africa::world::World;
use heart_of_africa::world::pathfind::find_path;

/// Input budget: a bot exceeding it is a softlock.
pub const INPUT_BUDGET: usize = 6000;

/// Player memory of the run.
#[derive(Default)]
pub struct PlayerBot {
    /// Settlement we are traveling toward (None = work where we are).
    target: Option<String>,
    /// Residents fully worked at the current settlement.
    residents_done: Vec<String>,
    /// Settlements explored without learning anything new.
    explored: Vec<String>,
    /// Diary length at the last look; growth means a clue was recorded.
    seen_diary: usize,
    /// Gift attempts at the current resident (gates need at most ~3).
    gifts: usize,
}

impl PlayerBot {
    pub fn new() -> Self {
        PlayerBot::default()
    }

    /// Play one run, recording every input in the replay log.
    pub fn play_recording(&mut self, game: &mut Game, world: &World) -> Vec<Input> {
        let mut log = Vec::new();
        while game.status == GameStatus::Playing && log.len() < INPUT_BUDGET {
            let Some(input) = self.next_input(game, world) else {
                break; // stuck
            };
            log.push(input.clone());
            game.apply(&input, world);
        }
        log
    }

    /// One input for the current state (public for the trace harness).
    pub fn next_input_pub(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        self.next_input(game, world)
    }

    fn next_input(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        // Keep body and soul together, exactly like a player watching the bars.
        if game.survival.hydration < 55.0 {
            return self.supply(game, world, "water");
        }
        if game.survival.nutrition < 55.0 {
            return self.supply(game, world, "food");
        }
        if game.survival.rest < 40.0 {
            return Some(Input::Rest);
        }
        if game.settlement_at(world).is_some() {
            let market = game
                .content
                .markets
                .get(&game.settlement_at(world)?.id)
                .cloned();
            if market.as_ref().is_some_and(|m| m.trades("water"))
                && game.survival.hydration < 80.0
                && game.player.item_count("water") < 2
            {
                return Some(Input::Buy("water".to_string()));
            }
            if market.as_ref().is_some_and(|m| m.trades("food"))
                && game.survival.nutrition < 80.0
                && game.player.item_count("food") < 2
            {
                return Some(Input::Buy("food".to_string()));
            }
        }

        // The revealed tomb marker is the endgame goal.
        if let Some(tomb) = game.tomb_revealed {
            return self.travel_or_walk(game, world, tomb);
        }

        // New diary entries carry the next destination in prose.
        self.absorb_diary(game, world);

        let here = game.settlement_at(world).map(|s| s.id.clone());
        let target = self.target.clone();
        match target {
            Some(target) if here.as_deref() == Some(target.as_str()) => {
                self.work_residents(game, world)
            }
            Some(target) => {
                self.leaving();
                self.travel_to_settlement(game, world, &target)
            }
            None => match here {
                Some(_) => self.work_residents(game, world), // startup: work where we are
                None => {
                    // No destination and in open country: head home.
                    let start = world.start()?;
                    self.travel_or_walk(game, world, (start.x, start.y))
                }
            },
        }
    }

    /// Use a carried supply, buy it here, or march to the nearest market
    /// that sells it (never loop on a no-op Buy in open country).
    fn supply(&mut self, game: &mut Game, world: &World, id: &str) -> Option<Input> {
        if game.player.item_count(id) > 0 {
            return Some(Input::Use(id.to_string()));
        }
        let trades_here = game
            .settlement_at(world)
            .and_then(|s| game.content.markets.get(&s.id))
            .is_some_and(|m| m.trades(id));
        if trades_here {
            return Some(Input::Buy(id.to_string()));
        }
        let (x, y) = game.player.pos();
        let target = world
            .settlements
            .iter()
            .filter(|s| {
                game.content
                    .markets
                    .get(&s.id)
                    .is_some_and(|m| m.trades(id))
            })
            .min_by_key(|s| s.x.abs_diff(x) + s.y.abs_diff(y))?
            .id
            .clone();
        self.travel_to_settlement(game, world, &target)
    }

    /// Read new diary entries: a fresh clue text names the next settlement.
    fn absorb_diary(&mut self, game: &Game, world: &World) {
        let new: Vec<String> = game
            .diary
            .iter()
            .skip(self.seen_diary)
            .map(|e| e.text.clone())
            .collect();
        self.seen_diary = game.diary.len();
        if new.is_empty() {
            return;
        }
        let here = game.settlement_at(world).map(|s| s.id.clone());
        for text in &new {
            if !text.starts_with("Clue recorded") {
                continue;
            }
            for settlement in &world.settlements {
                if Some(&settlement.id) != here.as_ref() && text.contains(&settlement.name) {
                    // Fresh information: re-explore with new knowledge.
                    self.target = Some(settlement.id.clone());
                    self.explored.clear();
                    self.residents_done.clear();
                    return;
                }
            }
        }
    }

    /// Talk and trade with the residents of the current settlement until one
    /// of them yields something, or the place is exhausted.
    fn work_residents(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        let residents: Vec<String> = game.npcs_here(world).iter().map(|n| n.id.clone()).collect();
        let npc_id = residents
            .iter()
            .find(|r| !self.residents_done.contains(r))
            .cloned();

        let Some(npc_id) = npc_id else {
            // The whole settlement yielded nothing new: mark it explored and
            // move to the nearest unexplored place.
            let here = game.settlement_at(world)?.id.clone();
            if !self.explored.contains(&here) {
                self.explored.push(here.clone());
            }
            self.residents_done.clear();
            self.gifts = 0;
            let (x, y) = game.player.pos();
            let next = world
                .settlements
                .iter()
                .filter(|s| s.id != here)
                .filter(|s| !self.explored.contains(&s.id))
                .min_by_key(|s| s.x.abs_diff(x) + s.y.abs_diff(y))?
                .id
                .clone();
            self.target = Some(next.clone());
            self.leaving();
            return self.travel_to_settlement(game, world, &next);
        };

        // One action per resident visit. Check the question FIRST: a gift
        // may have just crossed the gate (two gifts cover every gate).
        if game
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            return Some(Input::Talk(npc_id.clone(), 0));
        }
        let talk = game.talk_screen()?;
        if talk.tomb_question {
            // Ask. Whether it reveals the real chain or a herring, the diary
            // will tell us next iteration — so keep the resident open.
            self.gifts = 0;
            return Some(Input::Talk(npc_id.clone(), talk.options.len()));
        }
        if self.gifts >= 2 {
            // Two gifts and still no question: nothing more to unlock here.
            self.residents_done.push(npc_id.clone());
            self.gifts = 0;
        }
        if let Some(index) = talk.options.iter().position(|o| o.contains("gift")) {
            self.gifts += 1;
            return Some(Input::Talk(npc_id.clone(), index));
        }
        // No gift option visible: buy a gift — and if we already carry one
        // this resident didn't take, try the OTHER kind (a human trying
        // gifts would too). Keep reserves for supplies.
        if game.player.money() >= 60 {
            let settlement = game.settlement_at(world)?;
            let market = game.content.markets.get(&settlement.id)?;
            let carrying_coffee = game.player.item_count("coffee") > 0;
            let carrying_beads = game.player.item_count("beads") > 0;
            let item = if !carrying_coffee && market.trades("coffee") {
                Some("coffee")
            } else if !carrying_beads && market.trades("beads") {
                Some("beads")
            } else {
                None
            };
            if let Some(item) = item {
                return Some(Input::Buy(item.to_string()));
            }
        }
        // Nothing more to offer this resident.
        self.residents_done.push(npc_id.clone());
        self.gifts = 0;
        let last = talk.options.len().saturating_sub(1);
        Some(Input::Talk(npc_id.clone(), last))
    }

    /// Fast-travel along a scouted route to a settlement, else walk.
    fn travel_to_settlement(
        &mut self,
        game: &mut Game,
        world: &World,
        target: &str,
    ) -> Option<Input> {
        let to = world.settlement(target)?;
        self.travel_or_walk(game, world, (to.x, to.y))
    }

    /// One step toward a tile: a scouted caravan if it helps, else walk.
    fn travel_or_walk(
        &mut self,
        game: &mut Game,
        world: &World,
        target: (u32, u32),
    ) -> Option<Input> {
        if let Some(settlement) = game.settlement_at(world) {
            let routes: Vec<String> = game
                .routes_here(world)
                .iter()
                .map(|r| r.id.clone())
                .collect();
            for route_id in routes {
                let route = world.routes.iter().find(|r| r.id == route_id)?;
                let (a, b) = (world.settlement(&route.from)?, world.settlement(&route.to)?);
                let end = if a.id == settlement.id {
                    (b.x, b.y)
                } else {
                    (a.x, a.y)
                };
                if end == target {
                    return Some(Input::Travel(route_id));
                }
            }
        }
        let (x, y) = game.player.pos();
        let path = find_path(&world.grid, (x, y), target)?;
        let next = *path.get(1)?;
        let dir = Direction::between((x, y), next)?;
        // Night travel through tsetse country and when thirsty in open
        // desert — daylight exposure is how the ailments find you.
        let biome = world.grid.biome_id_at(next.0, next.1);
        if game.tick.is_daylight()
            && (biome == "savanna" || (biome == "desert" && game.survival.hydration < 60.0))
        {
            return Some(Input::Rest);
        }
        Some(Input::Move(dir))
    }

    /// We are moving on: resident work and gifts reset.
    fn leaving(&mut self) {
        self.residents_done.clear();
        self.gifts = 0;
    }
}
