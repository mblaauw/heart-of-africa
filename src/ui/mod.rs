//! egui UI panels: diary, inventory, trade, dialogue, map overlay (§8.2).

use egui::Context;

/// Minimal debug overlay: frame rate. Phase 1 scaffold; replaced by the
/// full HUD and debug tools in later phases.
pub fn debug_overlay(ctx: &Context, fps: i32) {
    egui::Window::new("Debug")
        .default_pos([8.0, 8.0])
        .show(ctx, |ui| {
            ui.label(format!("fps: {fps}"));
        });
}
