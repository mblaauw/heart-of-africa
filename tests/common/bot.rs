//! The headless sim bot (DESIGN.md §9): a greedy agent that plays a run
//! start to finish using only in-game information (option texts, clue text
//! structure, map). Used by the Phase 3 acceptance tests: 100 seeds, no
//! softlocks.

use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::player::Direction;
use heart_of_africa::world::World;
use heart_of_africa::world::pathfind::find_path;

/// Input budget per run: a bot exceeding it is a softlock.
pub const INPUT_BUDGET: usize = 4000;

/// Play a greedy run. Returns the number of inputs used (INPUT_BUDGET means
/// the bot got stuck — a softlock).
pub fn play(game: &mut Game, world: &World) -> usize {
    play_recording(game, world).len()
}

/// Play a greedy run, recording every input in the replay log.
pub fn play_recording(game: &mut Game, world: &World) -> Vec<Input> {
    let mut log = Vec::new();
    while game.status == GameStatus::Playing && log.len() < INPUT_BUDGET {
        let Some(input) = next_input(game, world) else {
            break; // stuck
        };
        log.push(input.clone());
        game.apply(&input, world);
    }
    log
}

/// One input for the current state: keep body and soul together first, then
/// follow the clue graph, then the tomb. Public for the trace harness.
pub fn next_input_pub(game: &mut Game, world: &World) -> Option<Input> {
    next_input(game, world)
}

fn next_input(game: &mut Game, world: &World) -> Option<Input> {
    // Critical supplies: use them, buy them in town, or march to a town
    // that sells them (never loop on a no-op Buy in open country).
    if game.survival.hydration < 55.0 {
        return supply_action(game, world, "water");
    }
    if game.survival.nutrition < 55.0 {
        return supply_action(game, world, "food");
    }
    // Rest when exhausted.
    if game.survival.rest < 40.0 {
        return Some(Input::Rest);
    }
    // Stock up while in town, before heading out.
    if game.settlement_at(world).is_some() {
        if game.survival.hydration < 80.0 && game.player.item_count("water") < 2 {
            return Some(Input::Buy("water".to_string()));
        }
        if game.survival.nutrition < 80.0 && game.player.item_count("food") < 2 {
            return Some(Input::Buy("food".to_string()));
        }
    }

    if let Some(tomb) = game.tomb_revealed {
        return step_toward(game, world, tomb);
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

/// Use a carried supply, buy it at the current market, or march toward the
/// nearest settlement that sells it.
fn supply_action(game: &mut Game, world: &World, id: &str) -> Option<Input> {
    if game.player.item_count(id) > 0 {
        return Some(Input::Use(id.to_string()));
    }
    if game.settlement_at(world).is_some() {
        return Some(Input::Buy(id.to_string()));
    }
    let target = nearest_market_settlement(game, world, id)?;
    travel_or_walk(game, world, &target)
}

/// The nearest settlement (Manhattan) whose market trades the item.
fn nearest_market_settlement(game: &Game, world: &World, id: &str) -> Option<String> {
    let (x, y) = game.player.pos();
    world
        .settlements
        .iter()
        .filter(|s| {
            game.content
                .markets
                .get(&s.id)
                .is_some_and(|m| m.trades(id))
        })
        .min_by_key(|s| s.x.abs_diff(x) + s.y.abs_diff(y))
        .map(|s| s.id.clone())
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
fn travel_or_walk(game: &mut Game, world: &World, target: &str) -> Option<Input> {
    let target_settlement = world.settlement(target)?;
    if let Some(settlement) = game.settlement_at(world)
        && let Some(route) = world.routes.iter().find(|r| {
            (r.from == settlement.id || r.to == settlement.id)
                && (r.from == target || r.to == target)
                && game.routes_here(world).iter().any(|h| h.id == r.id)
        })
    {
        return Some(Input::Travel(route.id.clone()));
    }
    step_toward(game, world, (target_settlement.x, target_settlement.y))
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
    let daylight = game.tick.is_daylight();
    let thirsty = game.survival.hydration < 60.0;
    if daylight && (biome == "savanna" || (biome == "desert" && thirsty)) {
        return Some(Input::Rest); // wait out the sun
    }
    Some(Input::Move(dir))
}
