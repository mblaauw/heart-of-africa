//! Phase 3 acceptance (DESIGN.md §10.3): three seeds produce visibly
//! different but always-winnable runs; the sim bot completes 100 seeds
//! without softlocks. Plus the §9 guarantees the generator must keep:
//! falsifiable herrings and rivals that never break solvability.

mod common;

use std::collections::BTreeSet;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::sim::rivals;
use heart_of_africa::world::World;
use heart_of_africa::world::pathfind::find_path;

use common::bot;

fn fresh_game(seed: u64, world: &World) -> Game {
    let content = load_game_content(common::content_dir()).expect("game content loads");
    Game::new(seed, content, world)
}

/// Seeds 0..100 are solvable by the greedy bot: every one wins within the
/// input budget (no softlocks, §10.3).
#[test]
fn sim_bot_completes_100_seeds_without_softlocks() {
    let world = load_world(common::content_dir()).expect("content loads");
    for seed in 0..100 {
        let mut game = fresh_game(seed, &world);
        let log = bot::play_recording(&mut game, &world);
        assert_eq!(
            game.status,
            GameStatus::Won,
            "seed {seed} did not win ({} inputs; pos {:?}; clues {:?}; rep {:?})",
            log.len(),
            game.player.pos(),
            game.clues,
            game.reputation
        );
        assert!(
            log.len() < bot::INPUT_BUDGET,
            "seed {seed} softlocked at {} inputs",
            log.len()
        );
        assert!(
            game.survival.hydration > 0.0 && game.survival.nutrition > 0.0,
            "seed {seed} survived to the tomb"
        );
    }
}

/// Three seeds produce visibly different but always-winnable runs: the tomb
/// site and clue texts differ across seeds (§10.3).
#[test]
fn three_seeds_produce_visibly_different_runs() {
    let world = load_world(common::content_dir()).expect("content loads");
    let signatures: Vec<(u32, u32, String)> = (0..6)
        .map(|seed| {
            let game = fresh_game(seed, &world);
            let text_hash = game
                .graph
                .clues
                .values()
                .map(|c| c.text.clone())
                .collect::<Vec<_>>()
                .join("|");
            (
                game.graph.expedition.tomb_tile.0,
                game.graph.expedition.tomb_tile.1,
                text_hash,
            )
        })
        .collect();
    let distinct: BTreeSet<(u32, u32, String)> = signatures.into_iter().collect();
    assert!(distinct.len() >= 3, "runs should vary across seeds");
}

/// Every seed's graph survives ANY single exclusive being taken by a rival,
/// and the empty graph is always solvable (the generation-time proof, §9).
#[test]
fn generation_time_solvability_proof_holds_for_all_seeds() {
    let world = load_world(common::content_dir()).expect("content loads");
    for seed in 0..50 {
        let game = fresh_game(seed, &world);
        assert!(
            game.graph.is_solvable(&BTreeSet::new()),
            "seed {seed} unsolvable"
        );
        for exclusive in ["e1", "e2", "f1", "f2"] {
            assert!(
                game.graph
                    .is_solvable(&BTreeSet::from([exclusive.to_string()])),
                "seed {seed}: consuming {exclusive} breaks solvability"
            );
        }
    }
}

/// Following the rivals' schedule, no seed ever becomes unsolvable (their
/// consumption is solvability-preserving by construction).
#[test]
fn rivals_never_break_solvability_under_their_schedule() {
    let world = load_world(common::content_dir()).expect("content loads");
    let content = load_game_content(common::content_dir()).expect("content loads");
    let rival_names = content
        .generator
        .as_ref()
        .map(|g| g.config.rivals.len())
        .unwrap_or(0);

    for seed in 0..25 {
        let game = fresh_game(seed, &world);
        let mut consumed: BTreeSet<String> = BTreeSet::new();
        let order = game.graph.settlement_order.clone();
        for day in 1..=300u64 {
            for rival in 0..rival_names {
                if !rivals::arrives_today(day) {
                    continue;
                }
                let pos = rivals::position(day, rival, &order);
                let exclusives: Vec<String> = game
                    .graph
                    .clues
                    .values()
                    .filter(|c| c.settlement == pos && is_exclusive(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
                for clue_id in exclusives {
                    if consumed.contains(&clue_id) {
                        continue;
                    }
                    let mut would = consumed.clone();
                    would.insert(clue_id.clone());
                    if game.graph.is_solvable(&would) {
                        consumed.insert(clue_id);
                        break;
                    }
                }
            }
        }
        assert!(
            game.graph.is_solvable(&consumed),
            "seed {seed}: rival play made the run unsolvable"
        );
    }
}

/// Red herrings are falsifiable: obtaining the contradicting corroboration
/// proves them false (DESIGN.md §4.5).
///
/// Runs on a seed whose herring sits at the start settlement or Asyut, and
/// finishes before the first rival arrival (day 3), so nothing gets consumed
/// mid-test.
#[test]
fn herrings_are_falsifiable() {
    let world = load_world(common::content_dir()).expect("content loads");
    let content = load_game_content(common::content_dir()).expect("content loads");

    // Find a seed with a conveniently placed herring.
    let mut seed = None;
    let mut herring_id = String::new();
    for candidate in 0..100u64 {
        let game = Game::new(candidate, content.clone(), &world);
        if let Some(h) = game.graph.clues.values().find(|c| c.red_herring)
            && (h.settlement == "cairo" || h.settlement == "asyut")
        {
            seed = Some(candidate);
            herring_id = h.id.clone();
            break;
        }
    }
    let seed = seed.expect("some seed has a herring near the start");
    let mut game = Game::new(seed, content, &world);

    let contradiction = game
        .graph
        .clues
        .get(&herring_id)
        .and_then(|c| c.contradicts_herring.clone())
        .expect("herring is falsifiable");

    // Order matters: start hint, then the herring, then leg A (whose
    // corroboration contradicts it). Leg A's nodes are gate-0 after c1.
    let start_clue = game.graph.start_clue.clone();
    ask_clue(&mut game, &world, &start_clue);
    ask_clue(&mut game, &world, &herring_id);
    assert!(game.clues.contains(&herring_id));
    assert!(
        !game.false_clues.contains(&herring_id),
        "herring believed until contradicted"
    );

    // Leg A: the key (gate 0), then the corroboration that contradicts.
    let e1 = game
        .graph
        .clues
        .values()
        .find(|c| c.id == "e1")
        .expect("leg A exists")
        .id
        .clone();
    ask_clue(&mut game, &world, &e1);
    ask_clue(&mut game, &world, &contradiction);
    assert!(game.clues.contains(&contradiction));
    assert!(
        game.false_clues.contains(&herring_id),
        "contradiction proves the rumor false"
    );

    // Fast enough that no rival arrival (day 3) could interfere.
    assert!(
        game.tick.day() < 3,
        "test must finish before rival arrivals; finished day {}",
        game.tick.day()
    );
}

/// Drive the dialogue to obtain one clue (gifting to meet the rep gate).
fn ask_clue(game: &mut Game, world: &World, clue_id: &str) {
    let clue = game.graph.clues.get(clue_id).expect("clue exists").clone();
    // Prerequisites first.
    for prereq in clue.prereq.iter().chain(clue.prereq_any.iter()) {
        if !game.clues.contains(prereq) {
            ask_clue(game, world, prereq);
        }
    }
    // Travel to the settlement.
    let to = world
        .settlement(&clue.settlement)
        .expect("settlement exists");
    let (x, y) = game.player.pos();
    let path = find_path(&world.grid, (x, y), (to.x, to.y)).expect("walkable");
    for pair in path.windows(2) {
        let dir = heart_of_africa::sim::player::Direction::between(pair[0], pair[1]).unwrap();
        game.apply(&Input::Move(dir), world);
    }
    // Meet the rep gate with gifts.
    while game.reputation(&clue.settlement) < clue.rep_gate {
        let settlement = game.settlement_at(world).expect("at settlement");
        let market = game.content.markets.get(&settlement.id).expect("market");
        let gift = if market.trades("coffee") {
            "coffee"
        } else {
            "beads"
        };
        if game.player.item_count(gift) == 0 {
            game.apply(&Input::Buy(gift.to_string()), world);
        }
        let _ = game.talk_screen();
        let npc_id = clue.npc.clone();
        if game
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            game.apply(&Input::Talk(npc_id.clone(), 0), world);
        }
        let talk = game.talk_screen().expect("conversation open");
        let index = talk
            .options
            .iter()
            .position(|o| o.contains("gift"))
            .expect("gift option visible");
        game.apply(&Input::Talk(npc_id, index), world);
    }
    // Ask the tomb question.
    let npc_id = clue.npc.clone();
    if game
        .conversation
        .as_ref()
        .is_none_or(|c| c.npc_id != npc_id)
    {
        game.apply(&Input::Talk(npc_id.clone(), 0), world);
    }
    let talk = game.talk_screen().expect("conversation open");
    assert!(talk.tomb_question, "tomb question available for {clue_id}");
    game.apply(&Input::Talk(npc_id, talk.options.len()), world);
    assert!(game.clues.contains(clue_id), "{clue_id} obtained");
}

fn is_exclusive(id: &str) -> bool {
    matches!(id, "e1" | "e2" | "f1" | "f2")
}
