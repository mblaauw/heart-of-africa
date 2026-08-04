//! Game binary: init, main loop, mode switching (DESIGN.md §8.2).

use macroquad::prelude::*;

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
    // Phase 1 scaffold: window + debug overlay only.
    // Unit 7 adds biome-map rendering and the player marker.
    loop {
        clear_background(BLACK);

        egui_macroquad::ui(|ctx| {
            heart_of_africa::ui::debug_overlay(ctx, macroquad::time::get_fps());
        });
        egui_macroquad::draw();

        next_frame().await
    }
}
