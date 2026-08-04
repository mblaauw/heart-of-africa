//! Game binary: init, main loop, mode switching (DESIGN.md §8.2).
//!
//! All inputs flow through `Game::apply` and the replay log, exactly like
//! the headless tests. Rendering, audio, and UI are skins over the sim.

use std::path::Path;

use macroquad::prelude::*;

use heart_of_africa::audio::Ambient;
use heart_of_africa::content::load::{load_assets, load_game_content, load_world};
use heart_of_africa::dialogue::llm::{self, LlmCache, LlmConfig, LlmContext, ValidatedReply};
use heart_of_africa::render::map::{MapRenderer, TILE_SIZE};
use heart_of_africa::save::{read_save, write_save};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::events;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::player::Direction;

const SAVE_FILE: &str = "saves/autosave.json";

/// The LLM garnish session (DESIGN.md §6): one request in flight per
/// conversation turn, polled by the UI; validated replies feed the scripted
/// pipeline through `Input::LlmAction`. Purely presentational — the sim
/// never waits on it, and a failure is just the scripted line.
struct LlmSession {
    config: LlmConfig,
    cache: LlmCache,
    pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    /// The validated reply for the current turn, if any.
    line: Option<String>,
    /// The context key the current `line` belongs to.
    line_key: Option<String>,
}

impl LlmSession {
    fn new(config: LlmConfig) -> LlmSession {
        LlmSession {
            config,
            cache: LlmCache::new(),
            pending: None,
            line: None,
            line_key: None,
        }
    }

    /// Advance the garnish one frame. Emits `Input::LlmAction`s for
    /// validated actions (recorded in the replay log, §9).
    fn tick(&mut self, game: &Game, inputs: &mut Vec<Input>) {
        if !self.config.enabled {
            return;
        }
        let Some(ctx) = game.llm_context() else {
            self.pending = None;
            self.line = None;
            self.line_key = None;
            return;
        };
        let key = ctx.key();

        // A new turn: drop the previous line and any stale request.
        if self.line_key.as_deref() != Some(key.as_str()) {
            self.line = None;
            self.line_key = Some(key.clone());
        }

        // The line for this turn is already in hand.
        if self.line.is_some() {
            return;
        }

        // A request is in flight: poll it.
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(raw)) => {
                    let reply = llm::parse_response(&raw)
                        .and_then(|content| llm::validate(&content, &game.llm_gates()));
                    self.cache.store(key.clone(), reply.clone());
                    if let Some(reply) = reply {
                        self.apply_reply(reply, &ctx, inputs);
                    }
                    self.pending = None;
                }
                Ok(Err(_)) => self.pending = None, // fail closed
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
            return;
        }

        // Serve from the session cache, or dispatch a request.
        if self.config.cache
            && let Some(reply) = self.cache.lookup(&key).flatten()
        {
            self.apply_reply(reply, &ctx, inputs);
            return;
        }
        let body = llm::build_request(&self.config, &ctx);
        let rx = llm::spawn_request(self.config.clone(), key.clone(), body);
        self.pending = Some(rx);
    }

    /// Show a validated reply: rephrased line + its actions as inputs.
    fn apply_reply(&mut self, reply: ValidatedReply, ctx: &LlmContext, inputs: &mut Vec<Input>) {
        self.line = Some(reply.say);
        for effect in reply.actions {
            inputs.push(Input::LlmAction {
                npc_id: ctx.npc_id.clone(),
                effect,
            });
        }
    }

    /// Whether the UI should show the considering indicator.
    fn waiting(&self) -> bool {
        self.pending.is_some() && self.line.is_none()
    }
}

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
    let mut ambient = Ambient::load(assets.as_ref()).await;

    // The LLM garnish is off unless config.toml enables it (DESIGN.md §6.2);
    // without the `llm` cargo feature it is structurally absent.
    let mut llm = LlmSession::new(llm::LlmConfig::load());

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
        // End of run: a new expedition with a fresh seed, or quit.
        if game.status != GameStatus::Playing {
            if is_key_pressed(KeyCode::N) {
                game = Game::new(game.seed.wrapping_add(1), content.clone(), &world);
                input_log.clear();
            }
            if is_key_pressed(KeyCode::Escape) {
                std::process::exit(0);
            }
        }

        // The LLM garnish: poll the in-flight reply, apply validated actions.
        llm.tick(&game, &mut inputs);

        // UI buttons produce inputs too; all inputs are applied in order and
        // recorded in the replay log.
        egui_macroquad::ui(|ctx| {
            inputs.extend(heart_of_africa::ui::game_ui(
                ctx,
                &mut game,
                &world,
                macroquad::time::get_fps(),
                llm.line.as_deref(),
                llm.waiting(),
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

        // Ambient wind, weather-aware (render-side only).
        ambient.tick(events::weather(game.seed, game.tick.day()));

        egui_macroquad::draw();

        next_frame().await
    }
}
