//! Phase 5 integration tests: the LLM garnish end-to-end through the sim —
//! context whitelisting, validated actions on the scripted pipeline, and
//! byte-identical replay with `Input::LlmAction` in the log (§9).

mod common;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::dialogue::npc::Effect;
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::world::World;

fn fresh_game(seed: u64, world: &World) -> Game {
    let content = load_game_content(common::content_dir()).expect("game content loads");
    Game::new(seed, content, world)
}

/// Open a conversation and choose the option whose text contains `fragment`.
fn talk(game: &mut Game, world: &World, npc_id: &str, fragment: &str) {
    if game
        .conversation
        .as_ref()
        .is_none_or(|c| c.npc_id != npc_id)
    {
        game.apply(&Input::Talk(npc_id.to_string(), 0), world);
    }
    let talk = game.talk_screen().expect("conversation open");
    let index = talk
        .options
        .iter()
        .position(|o| o.contains(fragment))
        .unwrap_or_else(|| panic!("option containing {fragment:?} visible"));
    game.apply(&Input::Talk(talk.npc_id.clone(), index), world);
}

fn gift(game: &mut Game, world: &World, npc_id: &str) {
    talk(game, world, npc_id, "gift");
}

/// The context whitelists exactly the pending clue: never the start clue
/// before the rep gate is met, always the start clue after.
#[test]
fn llm_context_whitelists_pending_clue() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(7, &world);

    game.apply(&Input::Talk("cairo_hakim".to_string(), 0), &world);
    let ctx = game.llm_context().expect("conversation context");
    let before = ctx.pending_clue.as_ref().map(|(id, _)| id.clone());
    assert_ne!(
        before.as_deref(),
        Some(game.graph.start_clue.as_str()),
        "start clue hidden before the rep gate"
    );

    // Earn Hakim's trust and the start clue enters the whitelist.
    game.apply(&Input::Buy("coffee".to_string()), &world);
    game.apply(&Input::Buy("coffee".to_string()), &world);
    gift(&mut game, &world, "cairo_hakim");
    gift(&mut game, &world, "cairo_hakim");
    let ctx = game.llm_context().expect("conversation context");
    let (id, text) = ctx.pending_clue.as_ref().expect("start clue whitelisted");
    assert_eq!(id, &game.graph.start_clue);
    assert!(!text.is_empty());

    // The gates carry every clue text for the leak scan.
    let gates = game.llm_gates();
    assert_eq!(gates.all_clue_texts.len(), game.graph.clues.len());
    assert_eq!(
        gates.pending_clue.as_ref().map(|(id, _)| id),
        Some(&game.graph.start_clue)
    );
}

/// A validated LLM action flows through the scripted pipeline: revealing
/// the pending clue works exactly like the scripted tomb question.
#[test]
fn llm_action_reveals_pending_clue_like_scripted() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(7, &world);
    game.apply(&Input::Buy("coffee".to_string()), &world);
    game.apply(&Input::Buy("coffee".to_string()), &world);
    game.apply(&Input::Talk("cairo_hakim".to_string(), 0), &world);
    gift(&mut game, &world, "cairo_hakim");
    gift(&mut game, &world, "cairo_hakim");

    let start_clue = game.graph.start_clue.clone();
    let effect = Effect {
        reveal_clue: Some(start_clue.clone()),
        ..Default::default()
    };
    game.apply(
        &Input::LlmAction {
            npc_id: "cairo_hakim".to_string(),
            effect,
        },
        &world,
    );

    assert!(
        game.clues.contains(&start_clue),
        "LLM action revealed the clue"
    );
    assert_eq!(game.status, GameStatus::Playing);
}

/// A non-pending (or empty) LLM action is a no-op: the garnish cannot invent
/// outcomes.
#[test]
fn llm_action_is_revalidated_and_rejected_when_not_pending() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut game = fresh_game(7, &world);
    game.apply(&Input::Buy("coffee".to_string()), &world);
    game.apply(&Input::Buy("coffee".to_string()), &world);
    game.apply(&Input::Talk("cairo_hakim".to_string(), 0), &world);
    gift(&mut game, &world, "cairo_hakim");
    gift(&mut game, &world, "cairo_hakim");

    // Reveal a clue that is NOT pending at this NPC (e.g. the location clue).
    let location = game.graph.location_clue.clone();
    let effect = Effect {
        reveal_clue: Some(location.clone()),
        ..Default::default()
    };
    let events = game.apply(
        &Input::LlmAction {
            npc_id: "cairo_hakim".to_string(),
            effect,
        },
        &world,
    );
    assert!(events.is_empty(), "disallowed action has no effect");
    assert!(!game.clues.contains(&location));

    // An empty effect is a no-op too.
    let events = game.apply(
        &Input::LlmAction {
            npc_id: "cairo_hakim".to_string(),
            effect: Effect::default(),
        },
        &world,
    );
    assert!(events.is_empty());
}

/// The §9 guarantee: a run whose log contains an `LlmAction` replays
/// byte-identically — the validated action is part of the input history.
#[test]
fn llm_action_replays_byte_identically() {
    let world = load_world(common::content_dir()).expect("content loads");
    let mut log = Vec::new();
    let mut game = fresh_game(7, &world);

    let step = |game: &mut Game, input: Input, log: &mut Vec<Input>| {
        log.push(input.clone());
        game.apply(&input, &world);
    };
    // Gift through the log too, so the replay sees identical inputs.
    let gift_step = |game: &mut Game, log: &mut Vec<Input>| {
        let talk = game.talk_screen().expect("conversation open");
        let index = talk
            .options
            .iter()
            .position(|o| o.contains("gift"))
            .expect("gift option visible");
        let input = Input::Talk(talk.npc_id.clone(), index);
        log.push(input.clone());
        game.apply(&input, &world);
    };

    step(&mut game, Input::Buy("coffee".to_string()), &mut log);
    step(&mut game, Input::Buy("coffee".to_string()), &mut log);
    step(
        &mut game,
        Input::Talk("cairo_hakim".to_string(), 0),
        &mut log,
    );
    gift_step(&mut game, &mut log);
    gift_step(&mut game, &mut log);

    let start_clue = game.graph.start_clue.clone();
    let llm_input = Input::LlmAction {
        npc_id: "cairo_hakim".to_string(),
        effect: Effect {
            reveal_clue: Some(start_clue.clone()),
            ..Default::default()
        },
    };
    step(&mut game, llm_input.clone(), &mut log);
    assert!(game.clues.contains(&start_clue));

    let first = game.snapshot(log.clone());
    let first_hash = first.state_hash();

    let mut replay = fresh_game(7, &world);
    for input in &log {
        replay.apply(input, &world);
    }
    let second = replay.snapshot(log);
    assert_eq!(
        second.state_hash(),
        first_hash,
        "LlmAction replays byte-identically"
    );
    assert!(replay.clues.contains(&start_clue));
}
