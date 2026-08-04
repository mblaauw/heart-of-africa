//! Game binary: init, main loop, mode switching (DESIGN.md §8.2).
//!
//! Phase 1 scope: load the world, walk the map with arrow keys, see the
//! biome map with placeholder markers, watch the sim clock tick.

use std::path::Path;

use macroquad::prelude::*;

use heart_of_africa::content::load::{load_assets, load_world};
use heart_of_africa::render::map::{MapRenderer, TILE_SIZE};
use heart_of_africa::sim::clock::Tick;
use heart_of_africa::sim::player::{Direction, Player};

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
    // The art manifest is optional by design (§5): missing art falls back to
    // placeholder shapes, so the game always runs.
    let assets = load_assets(Path::new("assets")).ok();

    let start = world
        .start()
        .expect("validated content has a start settlement");
    let mut player = Player::new(start.x, start.y);
    let mut tick = Tick::START;
    let renderer = MapRenderer::new(&world, assets.as_ref());

    loop {
        // Movement input: one successful move advances the sim clock by one
        // tick (1 tick = 1 in-game hour), keeping sim and render in step.
        let keys = [
            (KeyCode::Up, Direction::North),
            (KeyCode::Down, Direction::South),
            (KeyCode::Left, Direction::West),
            (KeyCode::Right, Direction::East),
        ];
        for (key, dir) in keys {
            if is_key_pressed(key) && player.try_move(&world.grid, dir).is_some() {
                tick.advance();
            }
        }

        clear_background(BLACK);
        renderer.draw(
            &world,
            player.pos(),
            (screen_width(), screen_height()),
            TILE_SIZE,
        );

        let biome = world.grid.biome_id_at(player.pos().0, player.pos().1);
        egui_macroquad::ui(|ctx| {
            heart_of_africa::ui::debug_overlay(
                ctx,
                macroquad::time::get_fps(),
                tick,
                player.pos(),
                biome,
            );
        });
        egui_macroquad::draw();

        next_frame().await
    }
}
