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
    if game.survival.hydration < 55.0 {
        return Some(use_or_buy(game, world, "water"));
    }
    if game.survival.nutrition < 55.0 {
        return Some(use_or_buy(game, world, "food"));
    }
    if game.survival.rest < 40.0 {
        return Some(Input::Rest);
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

/// Use a supply item if carried, else buy it at the current market.
fn use_or_buy(game: &mut Game, _world: &World, id: &str) -> Input {
    if game.player.item_count(id) > 0 {
        Input::Use(id.to_string())
    } else {
        Input::Buy(id.to_string())
    }
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
/// use the visible gift option, or buy the gift the NPC's options mention.
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
    // No visible gift option: find what this NPC accepts, buy it.
    let npc = game.content.npcs.get(npc_id)?;
    let mentions = |needle: &str| {
        npc.nodes
            .values()
            .flat_map(|n| &n.options)
            .any(|o| o.text.contains(needle))
    };
    let item = if mentions("coffee") {
        "coffee"
    } else if mentions("beads") {
        "beads"
    } else {
        return None;
    };
    if game.player.item_count(item) == 0 {
        return Some(Input::Buy(item.to_string()));
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

/// One walk step toward a tile (A* next hop).
fn step_toward(game: &mut Game, world: &World, target: (u32, u32)) -> Option<Input> {
    let (x, y) = game.player.pos();
    let path = find_path(&world.grid, (x, y), target)?;
    let next = *path.get(1)?;
    let dir = Direction::between((x, y), next)?;
    Some(Input::Move(dir))
}
