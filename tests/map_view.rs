//! Explore-map view (WS1 renderer split): camera inputs are
//! presentation-only — they must never change the sim, the replay log, or
//! the save hash (DESIGN.md §8.3, EXPANSION_PLAN WS1 gate).

mod common;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::render::map::camera::{KnownBounds, MapView, ViewInput, apply_view_input};
use heart_of_africa::sim::game::Game;
use heart_of_africa::world::World;

use common::bot;

fn fresh_game(seed: u64, world: &World) -> Game {
    let content = load_game_content(common::content_dir()).expect("game content loads");
    Game::new(seed, content, world)
}

/// A frame of camera input that exercises every explore action.
fn busy_input(screen: (f32, f32)) -> ViewInput {
    ViewInput {
        tab: false,
        esc: false,
        f: true,
        home: true,
        zoom_in: true,
        zoom_out: false,
        wheel: 1.0,
        drag: Some((37.0, -19.0)),
        mouse: (300.0, 250.0),
        screen,
    }
}

#[test]
fn camera_inputs_never_change_the_save_hash() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(11, &world);
    let log = bot::play_recording(&mut game, &world);
    assert_eq!(game.status, heart_of_africa::sim::game::GameStatus::Won);
    let before = game.snapshot(log.clone());
    let before_hash = before.state_hash();

    // Hammer every explore camera action: tab into explore, zoom, pan,
    // center on player, center on known world, tab back out.
    let mut view = MapView::new((world.width, world.height));
    let screen = (1280.0, 800.0);
    let mut input = busy_input(screen);
    input.tab = true;
    apply_view_input(
        &mut view,
        input,
        game.player.pos(),
        KnownBounds::from_visited(game.player.visited()),
    );
    assert_eq!(
        view.mode,
        heart_of_africa::render::map::camera::ViewMode::Explore
    );
    for _ in 0..16 {
        apply_view_input(
            &mut view,
            busy_input(screen),
            game.player.pos(),
            KnownBounds::from_visited(game.player.visited()),
        );
    }
    input.tab = true;
    apply_view_input(
        &mut view,
        input,
        game.player.pos(),
        KnownBounds::from_visited(game.player.visited()),
    );

    let after = game.snapshot(log);
    assert_eq!(
        after.state_hash(),
        before_hash,
        "camera inputs are presentation-only: the save hash must not move"
    );
    assert_eq!(after, before);
}

#[test]
fn camera_inputs_produce_no_replay_entries() {
    // The camera path returns nothing and appends nothing to the input log:
    // the recorded log before and after camera input must be identical.
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(3, &world);
    let log = bot::play_recording(&mut game, &world);
    let snap = game.snapshot(log.clone());
    let logged = snap.input_log.len();

    let mut view = MapView::new((world.width, world.height));
    apply_view_input(
        &mut view,
        busy_input((1280.0, 800.0)),
        game.player.pos(),
        KnownBounds::from_visited(game.player.visited()),
    );

    let snap2 = game.snapshot(log);
    assert_eq!(
        snap2.input_log.len(),
        logged,
        "camera input must not append to the replay log"
    );
}

#[test]
fn home_centers_on_known_world_bounds() {
    let world = load_world(common::content_dir()).expect("content loads");
    let game = fresh_game(5, &world);
    let visited = game.player.visited().to_vec();
    let mut view = MapView::new((world.width, world.height));
    view.mode = heart_of_africa::render::map::camera::ViewMode::Explore;
    let known = KnownBounds::from_visited(&visited);
    let expect = known.center((0.0, 0.0));
    apply_view_input(
        &mut view,
        ViewInput {
            home: true,
            screen: (1280.0, 800.0),
            ..Default::default()
        },
        game.player.pos(),
        known,
    );
    let c = view.camera.center_tile();
    assert!(
        (c.0 - expect.0).abs() < 1e-3 && (c.1 - expect.1).abs() < 1e-3,
        "Home centers on the visited bounds"
    );
}
