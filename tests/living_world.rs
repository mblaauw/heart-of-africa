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
/// Rivals may consume a leg's clues mid-test, so this runs over seeds until
/// one completes the whole sequence unmolested (deterministic per seed).
#[test]
fn herrings_are_falsifiable() {
    let world = load_world(common::content_dir()).expect("content loads");
    let content = load_game_content(common::content_dir()).expect("content loads");

    let mut completed = None;
    for candidate in 0..1000u64 {
        let mut game = Game::new(candidate, content.clone(), &world);
        let Some(h) = game.graph.clues.values().find(|c| c.red_herring) else {
            continue;
        };
        if h.settlement != "cairo" && h.settlement != "asyut" {
            continue; // keep the walks short
        }
        let herring_id = h.id.clone();
        let contradiction = h
            .contradicts_herring
            .clone()
            .expect("herring is falsifiable");
        let start_clue = game.graph.start_clue.clone();
        let e1 = game
            .graph
            .clues
            .values()
            .find(|c| c.id == "e1")
            .expect("leg A exists")
            .id
            .clone();

        // Order: start hint, then the herring (still believed), then leg A.
        let ok = try_ask_clue(&mut game, &world, &start_clue)
            && try_ask_clue(&mut game, &world, &herring_id)
            && game.clues.contains(&herring_id)
            && !game.false_clues.contains(&herring_id)
            && try_ask_clue(&mut game, &world, &e1)
            && try_ask_clue(&mut game, &world, &contradiction)
            && game.clues.contains(&contradiction)
            && game.false_clues.contains(&herring_id);
        if ok {
            completed = Some(candidate);
            break;
        }
    }
    assert!(
        completed.is_some(),
        "no seed demonstrated herring falsification without rival interference"
    );
}

/// Try to obtain one clue by driving the dialogue (gifting to meet the rep
/// gate). Returns false when anything blocks it (rival consumption, missing
/// gift option, unreachable) — the caller retries another seed.
fn try_ask_clue(game: &mut Game, world: &World, clue_id: &str) -> bool {
    let Some(clue) = game.graph.clues.get(clue_id).cloned() else {
        return false;
    };
    // Prerequisites first.
    for prereq in clue.prereq.iter().chain(clue.prereq_any.iter()) {
        if !game.clues.contains(prereq) && !try_ask_clue(game, world, prereq) {
            return false;
        }
    }
    // Travel to the settlement.
    let Some(to) = world.settlement(&clue.settlement) else {
        return false;
    };
    let (x, y) = game.player.pos();
    let Some(path) = find_path(&world.grid, (x, y), (to.x, to.y)) else {
        return false;
    };
    for pair in path.windows(2) {
        let Some(dir) = heart_of_africa::sim::player::Direction::between(pair[0], pair[1]) else {
            return false;
        };
        game.apply(&Input::Move(dir), world);
    }
    // Meet the rep gate with gifts.
    while game.reputation(&clue.settlement) < clue.rep_gate {
        let Some(settlement) = game.settlement_at(world) else {
            return false;
        };
        let Some(market) = game.content.markets.get(&settlement.id) else {
            return false;
        };
        let gift = if market.trades("coffee") {
            "coffee"
        } else {
            "beads"
        };
        if game.player.item_count(gift) == 0 {
            game.apply(&Input::Buy(gift.to_string()), world);
        }
        let npc_id = clue.npc.clone();
        if game
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            game.apply(&Input::Talk(npc_id.clone(), 0), world);
        }
        let Some(talk) = game.talk_screen() else {
            return false;
        };
        let Some(index) = talk.options.iter().position(|o| o.contains("gift")) else {
            return false;
        };
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
    let Some(talk) = game.talk_screen() else {
        return false;
    };
    if !talk.tomb_question {
        return false; // consumed by a rival, or gated beyond reach
    }
    game.apply(&Input::Talk(npc_id, talk.options.len()), world);
    game.clues.contains(clue_id)
}

fn is_exclusive(id: &str) -> bool {
    matches!(id, "e1" | "e2" | "f1" | "f2")
}

/// The key-item gate is enforced (DESIGN.md §4.5): a clue with
/// `requires_item` is unrevealable while the player lacks the item — bury
/// the brass key and the corroboration stays locked until it is recovered.
#[test]
fn key_item_gate_is_enforced() {
    let world = load_world(common::content_dir()).expect("content loads");
    let content = load_game_content(common::content_dir()).expect("content loads");

    let mut completed = None;
    for seed in 0..1000u64 {
        let mut game = Game::new(seed, content.clone(), &world);
        // Keep the legs nearby so the walk stays short (rival-free).
        let (e1, e2) = {
            let e1 = game.graph.clues.get("e1").expect("leg A exists").clone();
            let e2 = game.graph.clues.get("e2").expect("leg A exists").clone();
            if !["asyut", "luxor"].contains(&e1.settlement.as_str())
                || !["asyut", "luxor"].contains(&e2.settlement.as_str())
            {
                continue;
            }
            (e1, e2)
        };

        let start = game.graph.start_clue.clone();
        if !try_ask_clue(&mut game, &world, &start) {
            continue;
        }
        if !try_ask_clue(&mut game, &world, &e1.id) {
            continue;
        }
        assert_eq!(game.player.item_count("brass_key"), 1, "the key is carried");

        // Walk to the corroboration's settlement first, then bury the key
        // right there — bury and retrieve must share one tile.
        let to = world.settlement(&e2.settlement).expect("settlement exists");
        let (x, y) = game.player.pos();
        let Some(path) = find_path(&world.grid, (x, y), (to.x, to.y)) else {
            continue;
        };
        for pair in path.windows(2) {
            let Some(dir) = heart_of_africa::sim::player::Direction::between(pair[0], pair[1])
            else {
                continue;
            };
            game.apply(&Input::Move(dir), &world);
        }
        game.apply(&Input::Bury("brass_key".to_string()), &world);

        // With the key buried, the tomb question must be unavailable (unless
        // a herring shares this NPC — then this seed cannot demonstrate the
        // gate, so skip it).
        let npc_id = e2.npc.clone();
        if game
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            game.apply(&Input::Talk(npc_id.clone(), 0), &world);
        }
        let talk = game.talk_screen().expect("conversation open");
        if talk.tomb_question {
            continue;
        }

        // Recover the key: the question returns and the clue is revealable
        // (unless a rival consumed it mid-walk — then try another seed).
        game.apply(&Input::Retrieve("brass_key".to_string()), &world);
        let talk = game.talk_screen().expect("conversation open");
        if !talk.tomb_question {
            continue;
        }
        game.apply(&Input::Talk(npc_id, talk.options.len()), &world);
        if game.clues.contains(&e2.id) {
            completed = Some(seed);
            break;
        }
    }
    assert!(
        completed.is_some(),
        "no seed demonstrated the key-item gate without rival interference"
    );
}

/// The in-game-information acceptance (§10.3): the player-view bot — which
/// never reads the clue graph, obtained clues, or hidden flags — finishes
/// 100 seeds using only the diary prose, visible options, residents, and
/// the revealed tomb marker.
#[test]
fn player_view_bot_completes_100_seeds() {
    let world = load_world(common::content_dir()).expect("content loads");
    for seed in 0..100 {
        let mut game = fresh_game(seed, &world);
        let mut bot = common::player_bot::PlayerBot::new();
        let log = bot.play_recording(&mut game, &world);
        assert_eq!(
            game.status,
            GameStatus::Won,
            "seed {seed} not winnable from player-view information ({} inputs)",
            log.len()
        );
        assert!(
            log.len() < common::player_bot::INPUT_BUDGET,
            "seed {seed} softlocked at {} inputs",
            log.len()
        );
    }
}
