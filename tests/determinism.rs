//! Seed-replay determinism (DESIGN.md §9).
//!
//! The acceptance test of Phase 1 (§10.1): a seeded walk across the Nile
//! region replays deterministically in a headless test. World state is a
//! pure function of (seed, ordered player inputs); equal state hashes must
//! imply identical runs.

mod common;

use heart_of_africa::content::load::load_world;
use heart_of_africa::save::{Save, from_json, to_json};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::clock::Tick;
use heart_of_africa::sim::player::{Direction, Player};
use heart_of_africa::world::pathfind::find_path;

/// A scripted expedition: from the start settlement to Aswan, one
/// A*-computed step per input. Returns the ordered input log and the final
/// player state.
fn scripted_walk() -> (Vec<Input>, Player) {
    let world = load_world(common::content_dir()).expect("content loads");
    let start = world.start().expect("start settlement resolves");
    let goal = world.settlement("aswan").expect("aswan resolves");
    let path = find_path(&world.grid, (start.x, start.y), (goal.x, goal.y))
        .expect("a walk from the start settlement to Aswan exists");
    assert_eq!(path.first(), Some(&(start.x, start.y)));
    assert_eq!(path.last(), Some(&(goal.x, goal.y)));

    let mut player = Player::new(start.x, start.y);
    let mut inputs = Vec::new();
    for pair in path.windows(2) {
        let dir = Direction::between(pair[0], pair[1]).expect("path tiles are adjacent");
        player.try_move(&world.grid, dir).expect("path is passable");
        inputs.push(Input::Move(dir));
    }
    (inputs, player)
}

fn save_with(seed: u64, tick_hours: u64, player: Player, input_log: Vec<Input>) -> Save {
    Save {
        seed,
        tick: Tick::from_hours(tick_hours),
        player,
        input_log,
    }
}

#[test]
fn seeded_walk_across_the_nile_region_replays_deterministically() {
    let (inputs, player) = scripted_walk();
    // The route must actually cross the region (Cairo y=8 -> Aswan y=28).
    assert!(
        player.pos().1 >= 27,
        "walk should reach Aswan latitude, ended at {:?}",
        player.pos()
    );

    let first = save_with(42, inputs.len() as u64, player, inputs.clone());
    let first_hash = first.state_hash();

    // Replay the same log from scratch: identical state, identical hash.
    let world = load_world(common::content_dir()).expect("content loads");
    let start = world.start().expect("start settlement resolves");
    let mut replay_player = Player::new(start.x, start.y);
    for input in &inputs {
        let Input::Move(dir) = input;
        replay_player
            .try_move(&world.grid, *dir)
            .expect("replayed input is passable");
    }
    let replay = save_with(42, inputs.len() as u64, replay_player, inputs);
    assert_eq!(replay.state_hash(), first_hash);
    assert_eq!(replay, first);
}

#[test]
fn save_json_is_byte_identical_across_runs() {
    let (inputs, player) = scripted_walk();
    let first = to_json(&save_with(7, 3, player.clone(), inputs.clone())).unwrap();
    let second = to_json(&save_with(7, 3, player, inputs)).unwrap();
    assert_eq!(first, second);
}

#[test]
fn save_round_trip_via_json_keeps_state_hash() {
    let (inputs, player) = scripted_walk();
    let save = save_with(1, 5, player, inputs);
    let back = from_json(&to_json(&save).unwrap()).unwrap();
    assert_eq!(back.state_hash(), save.state_hash());
}
