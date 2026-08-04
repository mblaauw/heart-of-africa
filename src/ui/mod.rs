//! egui UI panels (DESIGN.md §8.2): HUD, market, dialogue, inventory,
//! caches, diary. UI code may import egui/macroquad freely; it only ever
//! changes the game through `Input`s, which it returns for the caller to
//! apply (the input log stays complete for replay).

use egui::Context;

use crate::sim::Input;
use crate::sim::economy::season_factor;
use crate::sim::game::{Game, GameStatus};
use crate::world::World;

/// Draw the game UI and return the inputs the player chose this frame.
pub fn game_ui(ctx: &Context, game: &mut Game, world: &World, fps: i32) -> Vec<Input> {
    let mut inputs = Vec::new();
    hud(ctx, game, world, fps, &mut inputs);
    settlement_panel(ctx, game, world, &mut inputs);
    conversation_panel(ctx, game, &mut inputs);
    pack_panel(ctx, game, &mut inputs);
    diary_panel(ctx, game);
    inputs
}

/// Top-left HUD: clock, purse, stats, ailments, standing, clues, outcome.
fn hud(ctx: &Context, game: &Game, world: &World, fps: i32, inputs: &mut Vec<Input>) {
    let settlement_name = game
        .settlement_at(world)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "open country".to_string());

    egui::Window::new("Expedition")
        .default_pos([8.0, 8.0])
        .show(ctx, |ui| {
            ui.label(format!(
                "Day {} {:02}:00 — {}",
                game.tick.day(),
                game.tick.hour_of_day(),
                settlement_name
            ));
            ui.label(format!(
                "Purse: {} piastres   |   f{:.0}",
                game.player.money(),
                fps
            ));
            bar(ui, "Hydration", game.survival.hydration);
            bar(ui, "Nutrition", game.survival.nutrition);
            bar(ui, "Rest", game.survival.rest);
            for (ailment, stage) in game.survival.visible_ailments() {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 60, 40),
                    format!("{} (stage {stage})", ailment.label()),
                );
            }
            if let Some(settlement) = game.settlement_at(world) {
                let rep = game.reputation(&settlement.id);
                ui.label(format!("Standing in {}: {}", settlement.name, rep));
            }
            ui.label(format!("Clues: {}", game.clues.len()));
            match &game.status {
                GameStatus::Playing => {}
                GameStatus::Won => {
                    ui.colored_label(egui::Color32::from_rgb(200, 170, 40), "THE TOMB IS FOUND");
                }
                GameStatus::Dead(reason) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 40),
                        format!("The expedition {reason}."),
                    );
                }
            }
            if ui.button("Rest (R)").clicked() {
                inputs.push(Input::Rest);
            }
            ui.label("Arrows: walk   R: rest   S: save   L: load");
        });
}

/// Market and residents of the current settlement.
fn settlement_panel(ctx: &Context, game: &mut Game, world: &World, inputs: &mut Vec<Input>) {
    let Some(settlement) = game.settlement_at(world) else {
        return;
    };
    egui::Window::new("Settlement")
        .default_pos([8.0, 320.0])
        .show(ctx, |ui| {
            ui.heading(&settlement.name);

            ui.label("Residents:");
            for npc in game.npcs_here(world) {
                if ui.button(format!("Talk to {}", npc.name)).clicked() {
                    inputs.push(Input::Talk(npc.id.clone(), 0));
                }
            }

            ui.separator();
            ui.label("Market:");
            let Some(market) = game.content.markets.get(&settlement.id) else {
                ui.label("No market here.");
                return;
            };
            let season = season_factor(game.tick.month());
            let rep = game.reputation(&settlement.id);
            let goods_ids: Vec<String> = market.items.keys().cloned().collect();
            for id in goods_ids {
                let Some(goods) = game.content.goods(&id) else {
                    continue;
                };
                let buy = market.buy_price(goods, season, rep);
                let sell = market.sell_price(goods, season, rep);
                let have = game.player.item_count(&id);
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({}p / {}p)", goods.name, buy, sell));
                    if ui.button("Buy").clicked() {
                        inputs.push(Input::Buy(id.clone()));
                    }
                    if have > 0 && ui.button("Sell").clicked() {
                        inputs.push(Input::Sell(id.clone()));
                    }
                });
            }
        });
}

/// The active conversation, if any.
fn conversation_panel(ctx: &Context, game: &mut Game, inputs: &mut Vec<Input>) {
    let Some((name, text)) = game.current_talk() else {
        return;
    };
    egui::Window::new(name)
        .default_pos([420.0, 60.0])
        .show(ctx, |ui| {
            ui.label(text);
            ui.separator();
            let visible = game.visible_talk_options();
            if let Some((npc_id, options)) = visible.first() {
                for (index, option) in options.iter().enumerate() {
                    if ui.button(&option.text).clicked() {
                        inputs.push(Input::Talk(npc_id.clone(), index));
                    }
                }
            }
        });
}

/// Inventory: use supplies, bury items.
fn pack_panel(ctx: &Context, game: &mut Game, inputs: &mut Vec<Input>) {
    egui::Window::new("Pack")
        .default_pos([420.0, 420.0])
        .show(ctx, |ui| {
            if game.player.items().is_empty() {
                ui.label("Your pack is empty.");
            }
            let items: Vec<String> = game.player.items().keys().cloned().collect();
            for id in items {
                let count = game.player.item_count(&id);
                let Some(goods) = game.content.goods(&id) else {
                    continue;
                };
                let usable = goods.restores_hydration > 0.0
                    || goods.restores_nutrition > 0.0
                    || goods.cures.is_some();
                ui.horizontal(|ui| {
                    ui.label(format!("{} x{count}", goods.name));
                    if usable && ui.button("Use").clicked() {
                        inputs.push(Input::Use(id.clone()));
                    }
                    if ui.button("Bury").clicked() {
                        inputs.push(Input::Bury(id.clone()));
                    }
                });
            }

            // Caches at the current tile.
            let (x, y) = game.player.pos();
            let cache_items: Vec<String> = game
                .caches
                .iter()
                .filter(|c| c.x == x && c.y == y)
                .flat_map(|c| c.items.keys().cloned().collect::<Vec<String>>())
                .collect();
            if !cache_items.is_empty() {
                ui.separator();
                ui.label("Cache here:");
                for id in cache_items {
                    if ui.button(format!("Retrieve {id}")).clicked() {
                        inputs.push(Input::Retrieve(id));
                    }
                }
            }
        });
}

/// Diary tail: the deterministic auto-log (DESIGN.md §4.9).
fn diary_panel(ctx: &Context, game: &Game) {
    egui::Window::new("Diary")
        .default_pos([420.0, 620.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(format!("{} entries", game.diary.len()));
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for entry in game.diary.iter().rev().take(24) {
                        ui.label(format!("Day {}: {}", entry.tick.day(), entry.text));
                    }
                });
        });
}

/// One stat bar in the HUD.
fn bar(ui: &mut egui::Ui, label: &str, value: f32) {
    ui.add(
        egui::ProgressBar::new((value / 100.0).clamp(0.0, 1.0))
            .text(format!("{label}: {value:.0}")),
    );
}
