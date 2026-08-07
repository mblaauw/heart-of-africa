//! egui UI panels (DESIGN.md §8.2): HUD, market, dialogue, inventory,
//! caches, diary. UI code may import egui/macroquad freely; it only ever
//! changes the game through `Input`s, which it returns for the caller to
//! apply (the input log stays complete for replay).

use egui::Context;

use crate::sim::Input;
use crate::sim::game::{Game, GameStatus};
use crate::world::World;

/// Apply the desert/parchment visual theme (inspired by the 1890s expedition
/// design prototype: warm earth tones, gold accents, dark wood panels).
pub fn apply_desert_theme(ctx: &egui::Context) {
    use egui::{Color32, CornerRadius, Stroke};
    let mut style = (*ctx.style()).clone();

    // Dark wood backgrounds
    style.visuals.window_fill = Color32::from_rgb(0x24, 0x1A, 0x10);
    style.visuals.panel_fill = Color32::from_rgb(0x1C, 0x14, 0x0C);
    style.visuals.faint_bg_color = Color32::from_rgb(0x1C, 0x14, 0x0C);
    style.visuals.extreme_bg_color = Color32::from_rgb(0x16, 0x0F, 0x09);

    // Gold/cream text on dark backgrounds
    style.visuals.override_text_color = Some(Color32::from_rgb(0xF3, 0xE4, 0xBD));

    // Widget backgrounds — parchment tone
    style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(0x3A, 0x2C, 0x18);
    style.visuals.widgets.noninteractive.fg_stroke =
        Stroke::new(1.0, Color32::from_rgb(0xCB, 0xAA, 0x5C));
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x5C, 0x43, 0x26);
    style.visuals.widgets.inactive.fg_stroke =
        Stroke::new(1.0, Color32::from_rgb(0xCB, 0xAA, 0x5C));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x8A, 0x6A, 0x3A);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xF6, 0xE9, 0xC9));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(0x9C, 0x45, 0x35); // warm red
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xF6, 0xE9, 0xC9));

    // Selection color
    style.visuals.selection.bg_fill = Color32::from_rgb(0x8A, 0x6A, 0x3A);
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0xE0, 0xC0, 0x7A));

    // Rounded corners (matches prototype's border-radius:6px/8px patterns)
    style.visuals.window_corner_radius = CornerRadius::same(6);
    style.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);
    style.visuals.widgets.inactive.corner_radius = CornerRadius::same(4);
    style.visuals.widgets.hovered.corner_radius = CornerRadius::same(4);
    style.visuals.widgets.active.corner_radius = CornerRadius::same(4);

    // Hyperlinks
    style.visuals.hyperlink_color = Color32::from_rgb(0x8A, 0x32, 0x26);

    // Warning/error colors
    style.visuals.warn_fg_color = Color32::from_rgb(0xC8, 0xAA, 0x28);
    style.visuals.error_fg_color = Color32::from_rgb(0xC8, 0x3C, 0x28);

    ctx.set_style(style);
}

/// Draw the game UI and return the inputs the player chose this frame.
/// `llm_line` is the garnish's rephrased line for the current conversation
/// (None = the scripted line); `llm_waiting` shows the considering state.
pub fn game_ui(
    ctx: &Context,
    game: &mut Game,
    world: &World,
    fps: i32,
    llm_line: Option<&str>,
    llm_waiting: bool,
) -> Vec<Input> {
    let mut inputs = Vec::new();
    hud(ctx, game, world, fps, &mut inputs);
    settlement_panel(ctx, game, world, &mut inputs);
    conversation_panel(ctx, game, llm_line, llm_waiting, &mut inputs);
    pack_panel(ctx, game, &mut inputs);
    diary_panel(ctx, game);
    dev_panel(ctx, game, world);
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
                "Weather: {}   |   Purse: {} piastres   |   f{:.0}",
                crate::sim::events::weather(game.seed, game.tick.day()).label(),
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
            if ui.button("Rest (R)").clicked() {
                inputs.push(Input::Rest);
            }
            if game.status != GameStatus::Playing {
                ui.add_space(6.0);
                match &game.status {
                    GameStatus::Won => {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 170, 40),
                            "THE TOMB IS FOUND — press N for a new expedition, Esc to quit",
                        );
                    }
                    GameStatus::Dead(reason) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 60, 40),
                            format!("The expedition {reason}. Press N for a new expedition, Esc to quit."),
                        );
                    }
                    GameStatus::Playing => {}
                }
            }
            if let Some(last) = game.diary.last() {
                ui.add_space(6.0);
                ui.label(format!("Last: {}", last.text));
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
            ui.label("Roads:");
            let routes = game.routes_here(world);
            if routes.is_empty() {
                ui.label("No scouted routes leave here yet.");
            }
            for route in routes {
                let (from, to) = (world.settlement(&route.from), world.settlement(&route.to));
                let dest = if from.is_some_and(|s| s.id == settlement.id) {
                    to
                } else {
                    from
                };
                let dest_name = dest.map_or("?", |s| s.name.as_str());
                let label = if route.risky {
                    format!("Caravan to {dest_name} (risky)")
                } else {
                    format!("Caravan to {dest_name}")
                };
                if ui.button(label).clicked() {
                    inputs.push(Input::Travel(route.id.clone()));
                }
            }

            ui.separator();
            ui.label("Market:");
            let Some(market) = game.content.markets.get(&settlement.id) else {
                ui.label("No market here.");
                return;
            };
            let goods_ids: Vec<String> = market.items.keys().cloned().collect();
            for id in goods_ids {
                let Some(goods) = game.content.goods(&id) else {
                    continue;
                };
                // The authoritative prices (what the transaction will charge):
                // base × season × drifted demand/supply × reputation.
                let buy = game.buy_price(&settlement.id, &id).unwrap_or(0);
                let sell = game.sell_price(&settlement.id, &id).unwrap_or(0);
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

/// The active conversation, if any. The tomb question is the runtime slot
/// of the seeded clue graph (DESIGN.md §4.5); with the LLM garnish on, the
/// NPC's line is the rephrased `llm_line` (falling back to the scripted
/// text), and "…considering" shows while the model is answering.
fn conversation_panel(
    ctx: &Context,
    game: &mut Game,
    llm_line: Option<&str>,
    llm_waiting: bool,
    inputs: &mut Vec<Input>,
) {
    let Some(talk) = game.talk_screen() else {
        return;
    };
    egui::Window::new(talk.npc_name)
        .default_pos([420.0, 60.0])
        .show(ctx, |ui| {
            let text = llm_line.unwrap_or(talk.text.as_str());
            ui.label(text);
            if llm_waiting && llm_line.is_none() {
                ui.label("…considering");
            }
            ui.separator();
            for (index, option) in talk.options.iter().enumerate() {
                if ui.button(option).clicked() {
                    inputs.push(Input::Talk(talk.npc_id.clone(), index));
                }
            }
            if talk.tomb_question && ui.button("Ask about the tomb.").clicked() {
                inputs.push(Input::Talk(talk.npc_id.clone(), talk.options.len()));
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
            // The clue board (DESIGN.md §4.9): everything the expedition
            // knows, false rumors marked, the tomb line once revealed.
            ui.label("Clues:");
            if game.clues.is_empty() {
                ui.label("(none yet — ask about the tomb)");
            }
            let clue_ids: Vec<String> = game.clues.iter().cloned().collect();
            for id in clue_ids {
                let Some(clue) = game.graph.clues.get(&id) else {
                    continue;
                };
                if game.false_clues.contains(&id) {
                    ui.colored_label(
                        egui::Color32::from_rgb(140, 140, 140),
                        format!("[proved false] {}", clue.text),
                    );
                } else {
                    ui.label(&clue.text);
                }
            }
            if let Some((tx, ty)) = game.tomb_revealed {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 170, 40),
                    format!("The tomb: the cleft in the hills at ({tx}, {ty})."),
                );
            }
            ui.separator();
            ui.label(format!("Log — {} entries", game.diary.len()));
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for entry in game.diary.iter().rev().take(24) {
                        ui.label(format!("Day {}: {}", entry.tick.day(), entry.text));
                    }
                });
        });
}

/// One stat bar in the HUD; color warns when the stat is low.
fn bar(ui: &mut egui::Ui, label: &str, value: f32) {
    let color = if value < 25.0 {
        egui::Color32::from_rgb(200, 60, 40)
    } else if value < 50.0 {
        egui::Color32::from_rgb(200, 170, 40)
    } else {
        egui::Color32::from_rgb(90, 170, 90)
    };
    ui.add(
        egui::ProgressBar::new((value / 100.0).clamp(0.0, 1.0))
            .fill(color)
            .text(format!("{label}: {value:.0}")),
    );
}

/// Development panel (F3 toggle): live read-only debug inspect.
/// Emits no `Input`s — presentation-only, hash-neutral.
pub fn dev_panel(ctx: &Context, game: &Game, world: &World) {
    egui::Window::new("Dev")
        .default_pos([8.0, 620.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(format!("seed: {}", game.seed));
            ui.label(format!("tick: {:?}", game.tick));
            let (px, py) = game.player.pos();
            let biome = world.grid.biome_at(px, py);
            ui.label(format!(
                "player: ({}, {}) biome: {:?}",
                px,
                py,
                biome.map(|b| b.id.as_str())
            ));
            if let Some(settlement) = game.settlement_at(world) {
                ui.label(format!("settlement: {}", settlement.name));
            }
            let weather = crate::sim::events::weather(game.seed, game.tick.day());
            ui.label(format!("weather: {}", weather.label()));
            ui.label(format!("money: {} piastres", game.player.money()));
            ui.label(format!("hydration: {:.0}", game.survival.hydration));
            ui.label(format!("nutrition: {:.0}", game.survival.nutrition));
            ui.label(format!("rest: {:.0}", game.survival.rest));
            ui.label(format!("clues: {}", game.clues.len()));
            ui.label(format!("reputation entries: {}", game.reputation.len()));
            ui.label(format!(
                "save hash: {:016x}",
                game.snapshot(vec![]).state_hash()
            ));
        });
}
