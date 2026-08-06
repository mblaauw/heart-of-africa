//! The headless sim bot (DESIGN.md §9): a greedy agent that plays a run
//! start to finish using only in-game information (option texts, clue text
//! structure, map). Used by the Phase 3 acceptance tests: 100 seeds, no
//! softlocks.
//!
//! Endgame (EXPANSION_PLAN §WS2): the location clue names a *region*, not
//! a tile. The bot reads the semantic hint and sweeps the claimed sector
//! (octant ∩ distance band from the anchor) in deterministic order until
//! standing on the tomb triggers discovery. Scan memory is bot-side state
//! ("what have I searched"), never game data — the save hash and replay
//! log stay untouched.

use heart_of_africa::sim::Input;
use heart_of_africa::sim::clues::SpatialHint;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::player::Direction;
use heart_of_africa::world::KM_PER_CELL;
use heart_of_africa::world::Route;
use heart_of_africa::world::World;
use heart_of_africa::world::geo::tile_distance_km;
use heart_of_africa::world::pathfind::find_path;

/// Input budget per run: a bot exceeding it is a softlock.
pub const INPUT_BUDGET: usize = 4000;

/// A greedy oracle run: full access to clue structure, never to the tomb
/// tile before discovery — the WS2 scan replaces the old direct walk.
#[derive(Default)]
pub struct Bot {
    /// The sector being swept, keyed by the hint it was built from (a
    /// different seed's hint rebuilds it).
    scan: Option<(SpatialHint, Vec<(u32, u32)>)>,
    /// Cursor into `scan`: tiles behind it have been walked already.
    scan_next: usize,
}

impl Bot {
    pub fn new() -> Self {
        Bot::default()
    }

    /// Drop per-run memory (scan cursor) so a fresh game starts clean.
    fn reset(&mut self) {
        self.scan = None;
        self.scan_next = 0;
    }

    /// Play a greedy run. Returns the number of inputs used (INPUT_BUDGET
    /// means the bot got stuck — a softlock).
    pub fn play(&mut self, game: &mut Game, world: &World) -> usize {
        self.play_recording(game, world).len()
    }

    /// Play a greedy run, recording every input in the replay log.
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

    /// One input for the current state. Public for the trace harness.
    pub fn next_input_pub(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        self.next_input(game, world)
    }

    fn next_input(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        // Once the sweep is committed the whole sector must fit in one
        // tank: heading back to market at the first thirst would burn the
        // band's approach and return on every restock (DESIGN.md §9's
        // budget, EXPANSION_PLAN §WS2). So the endgame trigger is much
        // lower than the normal cruise line.
        let (water_hard, food_hard) = if game.tomb_hint().is_some() {
            (30.0, 30.0)
        } else {
            (55.0, 55.0)
        };
        if game.survival.hydration < water_hard {
            return supply_action(game, world, "water");
        }
        if game.survival.nutrition < food_hard {
            return supply_action(game, world, "food");
        }
        // Rest when exhausted.
        if game.survival.rest < 40.0 {
            return Some(Input::Rest);
        }
        // Stock up while in town, before heading out (skip what we cannot
        // afford — the crisis path below handles being broke). Once the
        // location clue is in hand the endgame sweep starts: stock a full
        // tank so the sweep is self-funded (return trips to market are
        // what drains both time and the purse).
        if game.settlement_at(world).is_some() {
            let here = game.settlement_at(world).map(|s| s.id.clone()).unwrap();
            let (water_cap, food_cap) = if game.tomb_hint().is_some() {
                (12, 8)
            } else {
                (2, 2)
            };
            // In the endgame the hint settlement may be the last town the
            // bot sees, so fill the tank now regardless of current thirst —
            // nothing else the money can buy is worth the return trek. The
            // Buy loop self-paces: one unit per call, capped and
            // affordability-gated below.
            let want_water = game.tomb_hint().is_some()
                || (game.survival.hydration < 80.0 && game.player.item_count("water") < 4);
            if game.player.item_count("water") < water_cap
                && want_water
                && game
                    .buy_price(&here, "water")
                    .is_some_and(|p| p <= game.player.money())
            {
                return Some(Input::Buy("water".to_string()));
            }
            let want_food = game.tomb_hint().is_some()
                || (game.survival.nutrition < 80.0 && game.player.item_count("food") < 4);
            if game.player.item_count("food") < food_cap
                && want_food
                && game
                    .buy_price(&here, "food")
                    .is_some_and(|p| p <= game.player.money())
            {
                return Some(Input::Buy("food".to_string()));
            }
        }

        // Endgame: the marker is only set by discovery, so prefer the
        // direct walk once it appears, then sweep the hinted sector.
        if let Some(tomb) = game.tomb_revealed {
            return step_toward(game, world, tomb);
        }
        if game.tomb_hint().is_some() {
            return self.scan_step(game, world);
        }

        let clue_id = next_clue_id(game)?;
        let (clue_settlement, clue_rep_gate, clue_npc) = {
            let clue = game.graph.clues.get(&clue_id)?;
            (clue.settlement.clone(), clue.rep_gate, clue.npc.clone())
        };
        if game.settlement_at(world).map(|s| s.id.as_str()) == Some(clue_settlement.as_str()) {
            let rep = game.reputation(&clue_settlement);
            if rep < clue_rep_gate {
                return gift_input(game, &clue_npc);
            }
            return tomb_question_input(game, &clue_npc);
        }
        travel_or_walk(game, world, &clue_settlement)
    }

    /// One step of the endgame sweep: walk the sector (deterministic
    /// order, nearest tile of the band first) until discovery fires.
    fn scan_step(&mut self, game: &mut Game, world: &World) -> Option<Input> {
        let hint = game.tomb_hint()?;
        if self.scan.as_ref().map(|(h, _)| h) != Some(&hint) {
            self.scan = Some((hint.clone(), build_sector(world, &hint)));
            self.scan_next = 0;
        }
        let sector = &self.scan.as_ref()?.1;
        let (x, y) = game.player.pos();
        // The cursor advances past tiles we are standing on; every tile is
        // walked at most once, so discovery (standing on the tomb) always
        // fires before the sweep ends.
        while self.scan_next < sector.len() && sector[self.scan_next] == (x, y) {
            self.scan_next += 1;
        }
        let target = *sector.get(self.scan_next)?;
        let step = step_toward(game, world, target);
        if step.is_none() {
            // Unreachable tile (rare: an impassable barrier): skip it.
            self.scan_next += 1;
            return self.scan_step(game, world);
        }
        step
    }
}

/// The sector a hint claims: the octant wedge at the claimed band from the
/// anchor, passable tiles only, nearest first. Falls back to the full band
/// annulus (any direction) and then to every passable tile when the wedge
/// is degenerate (off-map or empty) — the tomb is always inside the
/// primary wedge, so the fallbacks are never needed for a true hint.
///
/// The order is a concentric-ring arc sweep: tiles sorted by distance
/// ring, then by angle around the anchor. Consecutive targets are then
/// spatially adjacent (the bot arcs the wedge ring by ring instead of
/// zig-zagging across the 45° wedge between same-radius targets, which is
/// what a plain (x, y) tiebreak produces — the sweep must fit in one
/// supply tank, DESIGN.md §9's budget, EXPANSION_PLAN §WS2).
pub fn build_sector(world: &World, hint: &SpatialHint) -> Vec<(u32, u32)> {
    let Some((ox, oy)) = hint.origin_tile(world) else {
        return Vec::new();
    };
    let (min, max) = hint.band.km_range();
    let mut strict = Vec::new();
    let mut relaxed = Vec::new();
    for y in 0..world.grid.height() {
        for x in 0..world.grid.width() {
            let tile = (x, y);
            if tile == (ox, oy) || !world.grid.passable_at(x, y) {
                continue;
            }
            let km = tile_distance_km((ox, oy), tile, KM_PER_CELL);
            if km < min || km > max {
                continue;
            }
            if hint.sector_contains(world, tile) {
                strict.push(tile);
            }
            relaxed.push(tile);
        }
    }
    let sector = if strict.is_empty() { relaxed } else { strict };
    let mut sector = sector;
    let (fx, fy) = (ox as f64, oy as f64);
    sector.sort_by_key(|&(x, y)| {
        let km = tile_distance_km((ox, oy), (x, y), KM_PER_CELL);
        let ring = (km / KM_PER_CELL).round() as u64;
        let mut angle = ((y as f64) - fy).atan2((x as f64) - fx);
        if angle < 0.0 {
            angle += std::f64::consts::TAU;
        }
        let angle_bucket = (angle / std::f64::consts::TAU * 100_000.0) as u64;
        (ring, angle_bucket, x, y)
    });
    sector
}

// --- Free-function API: the legacy call sites get a fresh bot per run,
// but a persistent one per thread for the step-wise trace harness. ---

thread_local! {
#[allow(clippy::missing_const_for_thread_local)]
    static BOT: std::cell::RefCell<Bot> =
        std::cell::RefCell::new(Bot { scan: None, scan_next: 0 });
}

/// Play a greedy run. Returns the number of inputs used.
pub fn play(game: &mut Game, world: &World) -> usize {
    BOT.with(|bot| {
        let mut bot = bot.borrow_mut();
        bot.reset();
        bot.play(game, world)
    })
}

/// Play a greedy run, recording every input in the replay log.
pub fn play_recording(game: &mut Game, world: &World) -> Vec<Input> {
    BOT.with(|bot| {
        let mut bot = bot.borrow_mut();
        bot.reset();
        bot.play_recording(game, world)
    })
}

/// One input for the current state. Public for the trace harness. The scan
/// cache is keyed by the hint, so step-wise calls stay consistent across
/// games without a reset.
pub fn next_input_pub(game: &mut Game, world: &World) -> Option<Input> {
    BOT.with(|bot| bot.borrow_mut().next_input_pub(game, world))
}

/// Use a carried supply, buy it at the current market, or march toward the
/// best settlement that sells it. Never loops on a no-op Buy: if the market
/// here cannot trade it (sold out or unaffordable), the bot sells cargo to
/// raise the price, moves to a better market, or works the spread.
fn supply_action(game: &mut Game, world: &World, id: &str) -> Option<Input> {
    if game.player.item_count(id) > 0 {
        return Some(Input::Use(id.to_string()));
    }
    let money = game.player.money();
    let Some(settlement) = game.settlement_at(world) else {
        // In the field: pick the cheapest market we can actually buy at;
        // if we cannot afford any, park at the nearest market to trade.
        let target = best_market_for(game, world, id, Some(money))
            .or_else(|| best_market_for(game, world, id, None))?;
        return travel_or_walk(game, world, &target);
    };
    // Buy while we can afford it here.
    if game
        .buy_price(&settlement.id, id)
        .is_some_and(|p| p <= money)
    {
        return Some(Input::Buy(id.to_string()));
    }
    // Not affordable here: sell cargo to raise cash. The most valuable
    // item the market buys funds the supply; anything sold can be
    // re-bought when the clue needs it.
    if let Some((item, _)) = game
        .player
        .items()
        .iter()
        .filter(|(item, _)| *item != id && game.content.goods(item).is_some())
        .filter_map(|(item, _)| {
            game.sell_price(&settlement.id, item)
                .map(|p| (item.clone(), p))
        })
        .max_by_key(|(_, p)| *p)
    {
        return Some(Input::Sell(item));
    }
    // Affordable elsewhere: march there (travel and the gated price agree,
    // so this never picks the settlement we just failed to buy at). When
    // nothing is affordable, merchant_step trades in town instead of
    // walking away in circles.
    if let Some(target) = best_market_for(game, world, id, Some(money)) {
        return travel_or_walk(game, world, &target);
    }
    merchant_step(game, world)
}

/// The market for a supply: cheapest buy price wins; distance breaks ties.
/// `budget` (when `Some`) filters to markets the bot can actually pay at.
fn best_market_for(game: &Game, world: &World, id: &str, budget: Option<u32>) -> Option<String> {
    let (x, y) = game.player.pos();
    world
        .settlements
        .iter()
        .filter(|s| budget.is_none_or(|b| game.buy_price(&s.id, id).is_some_and(|p| p <= b)))
        .filter(|s| game.buy_price(&s.id, id).is_some())
        .min_by_key(|s| {
            let price = game.buy_price(&s.id, id).unwrap_or(u32::MAX);
            let dist = s.x.abs_diff(x) + s.y.abs_diff(y);
            (price, dist)
        })
        .map(|s| s.id.clone())
}

/// One step of the market arbitrage loop (DESIGN.md §4.7): first deliver
/// a carried good to its best market (that's why it was bought), then
/// liquidate any residual cargo, then buy the best positive cross-market
/// spread.
fn merchant_step(game: &mut Game, world: &World) -> Option<Input> {
    let settlement = game.settlement_at(world)?;
    let here = settlement.id.clone();
    // Deliver goods that sell for more somewhere else before liquidating:
    // selling them back here (at a loss by construction) would drain the
    // bot's money to zero in a buy→sell loop.
    let deliverable = game
        .player
        .items()
        .iter()
        .filter(|(item, _)| *item != "water" && *item != "food")
        .filter_map(|(item, _)| {
            let local = game.sell_price(&here, item)?;
            let best = world
                .settlements
                .iter()
                .filter(|s| s.id != here)
                .filter_map(|s| game.sell_price(&s.id, item).map(|p| (s.id.clone(), p)))
                .max_by_key(|(_, p)| *p)
                .filter(|(_, p)| *p > local)?;
            Some((best.1, item.clone(), best.0))
        })
        .max_by_key(|(price, _, _)| *price);
    if let Some((_price, _gid, sell_at)) = deliverable {
        return travel_or_walk(game, world, &sell_at);
    }
    if let Some((item, _)) = game
        .player
        .items()
        .iter()
        .filter(|(item, _)| *item != "water" && *item != "food")
        .filter_map(|(item, _)| game.sell_price(&here, item).map(|p| (item.clone(), p)))
        .max_by_key(|(_, p)| *p)
    {
        return Some(Input::Sell(item));
    }
    let mut best: Option<(String, String, u32)> = None;
    for gid in game.content.goods.keys() {
        let Some(buy) = game.buy_price(&here, gid) else {
            continue;
        };
        if buy > game.player.money() {
            continue;
        }
        for s in &world.settlements {
            if s.id == here {
                continue;
            }
            if let Some(sell) = game.sell_price(&s.id, gid)
                && sell > buy
                && best.as_ref().is_none_or(|(_, _, p)| sell - buy > *p)
            {
                best = Some((gid.clone(), s.id.clone(), sell - buy));
            }
        }
    }
    let (gid, sell_at, _) = best?;
    if game.player.item_count(&gid) > 0 {
        return travel_or_walk(game, world, &sell_at);
    }
    Some(Input::Buy(gid.clone()))
}

/// The next clue the bot should chase: not obtained, not taken by a rival,
/// prerequisites met, and not a red herring (flavor only).
fn next_clue_id(game: &Game) -> Option<String> {
    game.graph
        .clues
        .values()
        .filter(|c| !c.red_herring)
        .filter(|c| !game.clues.contains(&c.id))
        .filter(|c| !game.rivals_consumed.contains(&c.id))
        .find(|c| game.graph.prereqs_met(&c.id, &game.clues))
        .map(|c| c.id.clone())
}

/// Open (or continue) a conversation and gift the NPC its accepted gift:
/// use the visible gift option, or buy the gift from the NPC's `gift` field.
fn gift_input(game: &mut Game, npc_id: &str) -> Option<Input> {
    if game
        .conversation
        .as_ref()
        .is_none_or(|c| c.npc_id != npc_id)
    {
        return Some(Input::Talk(npc_id.to_string(), 0));
    }
    let talk = game.talk_screen()?;
    if let Some(index) = talk.options.iter().position(|o| o.contains("gift")) {
        return Some(Input::Talk(talk.npc_id.clone(), index));
    }
    // No visible gift option: buy the gift this NPC accepts.
    let item = game.content.npcs.get(npc_id)?.gift.as_ref()?.item.clone();
    if game.player.item_count(&item) == 0 {
        return Some(Input::Buy(item));
    }
    // Carrying the gift but the option is hidden — leave the talk.
    Some(Input::Talk(
        talk.npc_id.clone(),
        talk.options.len().saturating_sub(1),
    ))
}

/// Open (or continue) a conversation and ask the tomb question.
fn tomb_question_input(game: &mut Game, npc_id: &str) -> Option<Input> {
    if game
        .conversation
        .as_ref()
        .is_none_or(|c| c.npc_id != npc_id)
    {
        return Some(Input::Talk(npc_id.to_string(), 0));
    }
    let talk = game.talk_screen()?;
    if !talk.tomb_question {
        // Nothing askable right now; end the talk and retry next step.
        return Some(Input::Talk(
            talk.npc_id.clone(),
            talk.options.len().saturating_sub(1),
        ));
    }
    Some(Input::Talk(talk.npc_id.clone(), talk.options.len()))
}

/// Fast-travel along a scouted route to the target settlement, else walk.
/// Refuses to board when the ride would cross daylight over tsetse
/// country (savanna) or thirsty desert — exposure is how the ailments
/// find you, and dying on the road wastes the whole run (§4.3).
fn travel_or_walk(game: &mut Game, world: &World, target: &str) -> Option<Input> {
    let target_settlement = world.settlement(target)?;
    if let Some(settlement) = game.settlement_at(world)
        && let Some(route) = world.routes.iter().find(|r| {
            (r.from == settlement.id || r.to == settlement.id)
                && (r.from == target || r.to == target)
                && game.routes_here(world).iter().any(|h| h.id == r.id)
        })
        && !ride_would_expose(world, game, route, &settlement.id)
    {
        return Some(Input::Travel(route.id.clone()));
    }
    step_toward(game, world, (target_settlement.x, target_settlement.y))
}

/// Would boarding this route now cross any daylight hour over savanna or
/// (when thirsty) desert? Samples the committed polyline hour-by-hour,
/// exactly like the ride sim itself.
fn ride_would_expose(world: &World, game: &Game, route: &Route, here: &str) -> bool {
    let tiles: Vec<(u32, u32)> = if route.from == here {
        route.tiles.to_vec()
    } else {
        route.tiles.iter().rev().copied().collect()
    };
    let hour0 = game.tick.hour_of_day();
    let thirsty = game.survival.hydration < 60.0;
    tiles.iter().enumerate().any(|(i, &(tx, ty))| {
        let hour = (hour0 + (i as u64) / 2) % 24;
        if !(6..18).contains(&hour) {
            return false;
        }
        let biome = world.grid.biome_id_at(tx, ty);
        biome == "savanna" || (biome == "desert" && thirsty)
    })
}

/// One walk step toward a tile (A* next hop). Travels at night through
/// tsetse country (savanna) and when thirsty in open desert — daylight
/// exposure is how the ailments find you (DESIGN.md §4.3).
fn step_toward(game: &mut Game, world: &World, target: (u32, u32)) -> Option<Input> {
    let (x, y) = game.player.pos();
    let path = find_path(&world.grid, (x, y), target)?;
    let next = *path.get(1)?;
    let dir = Direction::between((x, y), next)?;
    let (nx, ny) = (next.0, next.1);
    let biome = world.grid.biome_id_at(nx, ny);
    let hour = game.tick.hour_of_day();
    let thirsty = game.survival.hydration < 60.0;
    // A move spans `cost` hours (matching the sim's `as u64`); starting far
    // enough before dawn would roll the last hour into daylight exposure
    // (§4.3). Rest from hour 6−cost, so the 18:00→05:00 window stays free.
    let exposed = biome == "savanna" || (biome == "desert" && thirsty);
    if exposed
        && hour >= 6u64.saturating_sub(world.grid.movement_cost_at(nx, ny) as u64)
        && hour < 18
    {
        return Some(Input::Rest); // wait out the sun
    }
    Some(Input::Move(dir))
}
