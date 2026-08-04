//! egui UI panels: diary, inventory, trade, dialogue, map overlay (§8.2).

use egui::Context;

use crate::sim::clock::Tick;

/// Minimal debug overlay: frame rate, sim clock, player position, and the
/// biome under the player. Phase 1 scaffold; the full HUD arrives later.
pub fn debug_overlay(ctx: &Context, fps: i32, tick: Tick, pos: (u32, u32), biome_id: &str) {
    egui::Window::new("Debug")
        .default_pos([8.0, 8.0])
        .show(ctx, |ui| {
            ui.label(format!("fps: {fps}"));
            ui.label(format!("day {} {:02}:00", tick.day(), tick.hour_of_day()));
            ui.label(format!("pos: {}, {}", pos.0, pos.1));
            ui.label(format!("biome: {biome_id}"));
        });
}
