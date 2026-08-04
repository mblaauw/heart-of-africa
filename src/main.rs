//! Game binary: init, main loop, mode switching (DESIGN.md §8.2).
//!
//! Phase 2 scope: walk the map, talk, trade, survive, and solve the clue
//! chain of the vertical slice. All inputs flow through `Game::apply` and
//! the replay log, exactly like the headless tests.

use std::path::Path;

use macroquad::prelude::*;

use heart_of_africa::content::load::{load_assets, load_game_content, load_world};
use heart_of_africa::render::map::{MapRenderer, TILE_SIZE};
use heart_of_africa::save::{read_save, write_save};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::Game;
use heart_of_africa::sim::player::Direction;

const SAVE_FILE: &str = "saves/autosave.json";

fn window_conf() -> Conf {
    Conf {
        window_title: "Heart of Africa".to_owned(),
        window_width: 1280,
        window_height: 800,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // Content is validated by `cargo test`; a hard failure here means the
    // game was launched from the wrong directory or the repo is broken.
    let world = match load_world(Path::new("content")) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to load content: {e:#}");
            std::process::exit(1);
        }
    };
    let content = match load_game_content(Path::new("content")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to load game content: {e:#}");
            std::process::exit(1);
        }
    };
    // The art manifest is optional by design (§5): missing art falls back to
    // placeholder shapes, so the game always runs.
    let assets = load_assets(Path::new("assets")).ok();

    let mut game = Game::new(1, content.clone(), &world);
    let mut input_log: Vec<Input> = Vec::new();
    let renderer = MapRenderer::new(&world, assets.as_ref());

    let _ = std::fs::create_dir_all("saves");

    loop {
        // Movement and rest keys: every successful move is one sim tick.
        let keys = [
            (KeyCode::Up, Direction::North),
            (KeyCode::Down, Direction::South),
            (KeyCode::Left, Direction::West),
            (KeyCode::Right, Direction::East),
        ];
        let mut inputs: Vec<Input> = Vec::new();
        for (key, dir) in keys {
            if is_key_pressed(key) {
                inputs.push(Input::Move(dir));
            }
        }
        if is_key_pressed(KeyCode::R) {
            inputs.push(Input::Rest);
        }
        if is_key_pressed(KeyCode::S) {
            let save = game.snapshot(input_log.clone());
            match write_save(Path::new(SAVE_FILE), &save) {
                Ok(()) => eprintln!("saved to {SAVE_FILE}"),
                Err(e) => eprintln!("save failed: {e}"),
            }
        }
        if is_key_pressed(KeyCode::L) {
            match read_save(Path::new(SAVE_FILE)) {
                Ok(save) => {
                    input_log = save.input_log.clone();
                    game = Game::from_save(save, content.clone(), &world);
                    eprintln!("loaded {SAVE_FILE}");
                }
                Err(e) => eprintln!("load failed: {e}"),
            }
        }

        // UI buttons produce inputs too; all inputs are applied in order and
        // recorded in the replay log.
        egui_macroquad::ui(|ctx| {
            inputs.extend(heart_of_africa::ui::game_ui(
                ctx,
                &mut game,
                &world,
                macroquad::time::get_fps(),
            ));
        });
        for input in &inputs {
            input_log.push(input.clone());
            game.apply(input, &world);
        }

        clear_background(BLACK);
        renderer.draw(
            &world,
            game.player.pos(),
            (screen_width(), screen_height()),
            TILE_SIZE,
            game.tomb_revealed,
        );

        egui_macroquad::draw();

        next_frame().await
    }
}
