//! Seed-replay determinism (DESIGN.md §9): world state at tick T is a pure
//! function of (seed, ordered player inputs). Bot runs replay byte-identically
//! from their input logs.

mod common;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::save::{Save, from_json, to_json};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::Game;
use heart_of_africa::sim::player::Direction;
use heart_of_africa::world::World;
use heart_of_africa::world::pathfind::find_path;

use common::bot;

fn fresh_game(seed: u64, world: &World) -> Game {
    let content = load_game_content(common::content_dir()).expect("game content loads");
    Game::new(seed, content, world)
}

#[test]
fn seeded_walk_across_the_nile_region_replays_deterministically() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(42, &world);
    let aswan = world.settlement("aswan").unwrap();
    let (x, y) = game.player.pos();
    let path = find_path(&world.grid, (x, y), (aswan.x, aswan.y)).unwrap();
    for pair in path.windows(2) {
        let dir = Direction::between(pair[0], pair[1]).unwrap();
        log.push(Input::Move(dir));
        game.apply(&Input::Move(dir), &world);
    }
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
fn bot_run_replays_byte_identically_from_its_input_log() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(7, &world);
    let log = bot::play_recording(&mut game, &world);
    assert!(log.len() < bot::INPUT_BUDGET, "bot got stuck (softlock)");
    assert_eq!(game.status, heart_of_africa::sim::game::GameStatus::Won);

    let first = game.snapshot(log.clone());
    let first_hash = first.state_hash();

    let mut replay = fresh_game(7, &world);
    for input in &log {
        replay.apply(input, &world);
    }
    let second = replay.snapshot(log);
    assert_eq!(second.state_hash(), first_hash);
    assert_eq!(second, first);
}

#[test]
fn save_round_trip_via_json_keeps_state_hash() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(1, &world);
    bot::play(&mut game, &world);
    let save: Save = game.snapshot(Vec::new());
    let back = from_json(&to_json(&save).unwrap()).unwrap();
    assert_eq!(back.state_hash(), save.state_hash());
}

/// Using a non-consumable (a clue item) is a no-op: the pack can't be
/// wasted through the sim path.
#[test]
fn using_a_clue_item_does_not_consume_it() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(7, &world);
    game.player.add_item("brass_key");
    assert_eq!(game.player.item_count("brass_key"), 1);

    let events = game.apply(&Input::Use("brass_key".to_string()), &world);
    assert!(events.is_empty(), "non-consumable use must be a no-op");
    assert_eq!(game.player.item_count("brass_key"), 1, "the key survives");

    // Consumables still work.
    game.player.add_item("water");
    let events = game.apply(&Input::Use("water".to_string()), &world);
    assert_eq!(events.len(), 1);
    assert_eq!(game.player.item_count("water"), 0);
}
