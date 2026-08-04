//! Seed-replay determinism (DESIGN.md §9) and the Phase 2 acceptance test
//! (§10.2): a tester can finish a mini-run start→tomb using only in-game
//! information, and the run replays byte-identically from its input log.

mod common;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::save::{Save, from_json, to_json};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::player::Direction;
use heart_of_africa::world::World;
use heart_of_africa::world::pathfind::find_path;

/// Apply one input, recording it in the replay log.
fn step(game: &mut Game, world: &World, input: Input, log: &mut Vec<Input>) -> Vec<String> {
    log.push(input.clone());
    game.apply(&input, world)
}

/// Buy `count` units of a good at the current settlement.
fn buy(game: &mut Game, world: &World, id: &str, count: u32, log: &mut Vec<Input>) {
    for _ in 0..count {
        step(game, world, Input::Buy(id.to_string()), log);
    }
}

/// Walk to a tile using A* steps, one move per input.
fn walk_to(game: &mut Game, world: &World, target: (u32, u32), log: &mut Vec<Input>) {
    let (x, y) = game.player.pos();
    let path = find_path(&world.grid, (x, y), target).expect("walkable target");
    for pair in path.windows(2) {
        let dir = Direction::between(pair[0], pair[1]).expect("adjacent path tiles");
        step(game, world, Input::Move(dir), log);
        if game.status != GameStatus::Playing {
            return;
        }
    }
    assert_eq!(game.player.pos(), target, "walk ended at the target");
}

/// Speak the option whose text contains `fragment`; opens the conversation
/// first if needed (both inputs are recorded in the log). Returns false when
/// no visible option matches.
fn talk_option(
    game: &mut Game,
    world: &World,
    npc_id: &str,
    fragment: &str,
    log: &mut Vec<Input>,
) -> bool {
    if game
        .conversation
        .as_ref()
        .is_none_or(|c| c.npc_id != npc_id)
    {
        step(game, world, Input::Talk(npc_id.to_string(), 0), log);
    }
    let visible = game.visible_talk_options();
    let (_, options) = visible
        .into_iter()
        .find(|(id, _)| id == npc_id)
        .expect("npc is in conversation");
    let Some(index) = options.iter().position(|o| o.text.contains(fragment)) else {
        return false;
    };
    step(game, world, Input::Talk(npc_id.to_string(), index), log);
    true
}

/// A scripted expedition: Cairo -> Asyut -> Luxor -> Aswan -> the tomb,
/// driven only by what the game tells the player (option texts, clue texts).
/// Records every input for replay.
fn run_mini_run(game: &mut Game, world: &World, log: &mut Vec<Input>) {
    // Outfit in Cairo: gifts to buy trust, supplies for the road (§4.1).
    buy(game, world, "coffee", 2, log);
    buy(game, world, "beads", 2, log);
    buy(game, world, "water", 2, log);
    buy(game, world, "food", 1, log);

    // Hakim's knowledge is reputation-gated: two coffees earn his trust.
    assert!(talk_option(
        game,
        world,
        "cairo_hakim",
        "Give coffee as a gift.",
        log
    ));
    assert!(talk_option(
        game,
        world,
        "cairo_hakim",
        "Give coffee as a gift.",
        log
    ));
    assert!(talk_option(
        game,
        world,
        "cairo_hakim",
        "I seek the tomb of Pharaoh",
        log
    ));
    talk_option(game, world, "cairo_hakim", "Farewell", log);
    assert!(game.clues.contains("nile.c1"), "first clue obtained");
    assert_eq!(game.status, GameStatus::Playing);

    // The clue text itself names the next destination — in-game information.
    let clue_text = game
        .diary
        .iter()
        .find(|e| e.text.contains("nile.c1"))
        .expect("diary records the clue")
        .text
        .clone();
    assert!(
        clue_text.contains("Asyut"),
        "clue must point the way: {clue_text}"
    );

    walk_to(game, world, settle_pos(world, "asyut"), log);
    // Exercise a cache: bury and recover supplies at Asyut (§4.10).
    step(game, world, Input::Bury("water".to_string()), log);
    step(game, world, Input::Retrieve("water".to_string()), log);
    assert!(talk_option(game, world, "asyut_sheikh", "brass key", log));
    talk_option(game, world, "asyut_sheikh", "Farewell", log);
    assert!(game.clues.contains("nile.c2"));
    assert_eq!(game.player.item_count("brass_key"), 1, "the key is carried");

    walk_to(game, world, settle_pos(world, "luxor"), log);
    assert!(talk_option(game, world, "luxor_priest", "key", log));
    talk_option(game, world, "luxor_priest", "Farewell", log);
    assert!(game.clues.contains("nile.c3"));

    walk_to(game, world, settle_pos(world, "aswan"), log);
    talk_option(game, world, "aswan_guide", "Farewell", log); // greeting
    assert!(talk_option(
        game,
        world,
        "aswan_guide",
        "Give trade beads",
        log
    ));
    assert!(talk_option(
        game,
        world,
        "aswan_guide",
        "Give trade beads",
        log
    ));
    assert!(
        talk_option(game, world, "aswan_guide", "eastern hills", log),
        "the guide speaks once he trusts you"
    );
    talk_option(game, world, "aswan_guide", "Farewell", log);
    assert!(game.clues.contains("nile.c4"), "tomb location named");

    // Use the supplies bought in Cairo on the road.
    step(game, world, Input::Use("water".to_string()), log);
    step(game, world, Input::Use("food".to_string()), log);

    let tomb = game.tomb_revealed.expect("tomb tile is now known");
    walk_to(game, world, tomb, log);
    assert_eq!(game.status, GameStatus::Won, "the tomb is found");
    assert!(game.survival.hydration > 0.0, "survived the expedition");
}

fn settle_pos(world: &World, id: &str) -> (u32, u32) {
    let s = world
        .settlement(id)
        .unwrap_or_else(|| panic!("settlement {id} exists"));
    (s.x, s.y)
}

fn fresh_game(seed: u64, world: &World) -> Game {
    let content = load_game_content(common::content_dir()).expect("game content loads");
    Game::new(seed, content, world)
}

#[test]
fn seeded_walk_across_the_nile_region_replays_deterministically() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(42, &world);
    walk_to(&mut game, &world, settle_pos(&world, "aswan"), &mut log);
    assert!(game.player.pos().1 >= 27, "walk crossed the region");
    let first = game.snapshot(log.clone());

    // Replay the same log from scratch: identical state and hash.
    let mut replay = fresh_game(42, &world);
    for input in &log {
        replay.apply(input, &world);
    }
    let second = replay.snapshot(log);
    assert_eq!(second.state_hash(), first.state_hash());
    assert_eq!(second, first);
}

#[test]
fn mini_run_start_to_tomb_wins_using_in_game_information() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(7, &world);
    run_mini_run(&mut game, &world, &mut log);

    let win = game.snapshot(log.clone());
    assert_eq!(win.status, GameStatus::Won);
    assert_eq!(win.clues.len(), 4, "all four clues collected");
}

#[test]
fn mini_run_replays_byte_identically_from_its_input_log() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(7, &world);
    run_mini_run(&mut game, &world, &mut log);

    let first = game.snapshot(log.clone());
    let first_hash = first.state_hash();

    let mut replay = fresh_game(7, &world);
    for input in &log {
        replay.apply(input, &world);
    }
    let second = replay.snapshot(log);
    assert_eq!(second.state_hash(), first_hash);
    assert_eq!(second, first);
    assert_eq!(second.status, GameStatus::Won);
}

#[test]
fn save_round_trip_via_json_keeps_state_hash() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(1, &world);
    walk_to(&mut game, &world, settle_pos(&world, "luxor"), &mut log);
    let save: Save = game.snapshot(log);
    let back = from_json(&to_json(&save).unwrap()).unwrap();
    assert_eq!(back.state_hash(), save.state_hash());
}
