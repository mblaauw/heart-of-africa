mod common;

use heart_of_africa::content::load::{load_game_content, load_world};
use heart_of_africa::sim::Input;
use heart_of_africa::sim::game::{Game, GameStatus};
use heart_of_africa::world::World;

#[test]
#[ignore]
fn dbg_seed() {
    let world = load_world(common::content_dir()).expect("world");
    let seed: u64 = std::env::var("HOA_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let content = load_game_content(common::content_dir()).expect("content");
    let mut game = Game::new(seed, content, &world);
    println!("=== seed {seed} ===");
    println!("tomb tile {:?}", game.graph.expedition.tomb_tile);
    if let Some(h) = &game.graph.spatial_hint {
        println!(
            "hint origin {:?} origin_tile {:?} relation {:?} band {:?}",
            h.origin,
            h.origin_tile(&world),
            h.relation,
            h.band
        );
        let sec = common::bot::build_sector(&world, h);
        let tomb = game.graph.expedition.tomb_tile;
        println!(
            "sector size {} tomb {:?} in it {} | km origin..tomb {:.0}",
            sec.len(),
            tomb,
            sec.contains(&tomb),
            heart_of_africa::world::geo::tile_distance_km(
                h.origin_tile(&world).unwrap(),
                tomb,
                heart_of_africa::world::coords::KM_PER_CELL
            )
        );
        println!(
            "first 4 sector tiles {:?} ... last 4 {:?}",
            &sec[..4],
            &sec[sec.len() - 4..]
        );
    }
    for c in game.graph.clues.values() {
        println!(
            "clue {} kind={:?} @ {} npc={} gate={} herring={} prereq={:?} req_item={:?} grant={:?}",
            c.id,
            c.kind,
            c.settlement,
            c.npc,
            c.rep_gate,
            c.red_herring,
            c.prereq,
            c.requires_item,
            c.gives_item
        );
    }
    let use_player = std::env::var("HOA_PLAYER").is_ok_and(|v| v == "1");
    #[allow(clippy::type_complexity)]
    let mut bot: Box<dyn FnMut(&mut Game, &World) -> Option<Input>> = if use_player {
        let mut b = common::player_bot::PlayerBot::new();
        Box::new(move |g, w| b.next_input_pub(g, w))
    } else {
        let mut b = common::bot::Bot::new();
        Box::new(move |g, w| b.next_input_pub(g, w))
    };
    let mut log = Vec::new();
    let (mut spent_water, mut spent_food, mut spent_other, mut spent_travel, mut spent_talk) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut other_by_item: std::collections::BTreeMap<String, u32> = Default::default();
    let mut other_when: Vec<(usize, String, u32)> = Vec::new();
    let mut move_count = 0usize;
    let mut rest_count = 0usize;
    let mut use_count = 0usize;
    let mut talked = 0usize;
    let mut hint_at: Option<(usize, u32, u32, u32, u32)> = None;
    let mut tells: Vec<(String, String)> = Vec::new();
    let mut prev_clues: std::collections::BTreeSet<String> = Default::default();
    while game.status == GameStatus::Playing && log.len() < 4000 {
        let Some(input) = bot(&mut game, &world) else {
            println!(
                "STUCK at {} inputs; pos {:?} money {} hyd {:.0} nut {:.0} items {:?}",
                log.len(),
                game.player.pos(),
                game.player.money(),
                game.survival.hydration,
                game.survival.nutrition,
                game.player.items()
            );
            println!(
                "  ledger: water {spent_water} food {spent_food} other {spent_other} travel {spent_travel} talk {spent_talk} (start 300, left {})",
                game.player.money()
            );
            println!("  other breakdown: {other_by_item:?}");
            println!("  other buys: {:?}", other_when);
            println!(
                "  tally: moves {move_count} rests {rest_count} uses {use_count} talks {talked} (of {} inputs)",
                log.len()
            );
            println!("  hint: {:?}", game.tomb_hint());
            println!(
                "  hint appeared at input #{} with money {} water x{} food x{} hyd {}",
                hint_at.map(|h| h.0).unwrap_or(9999),
                hint_at.map(|h| h.1).unwrap_or(0),
                hint_at.map(|h| h.2).unwrap_or(0),
                hint_at.map(|h| h.3).unwrap_or(0),
                hint_at.map(|h| h.4).unwrap_or(0)
            );
            for item in game.player.items().keys() {
                println!(
                    "   sell {} at here: {:?}",
                    item,
                    game.sell_price("kano", item)
                );
            }
            println!("  rivals_consumed: {:?}", game.rivals_consumed);
            for d in game.diary.iter().filter(|d| d.text.starts_with("Rumor")) {
                println!("    day {}: {}", d.tick.day(), d.text);
            }
            for d in game
                .diary
                .iter()
                .filter(|d| d.text.starts_with("Clue recorded"))
            {
                println!("    day {}: {}", d.tick.day(), d.text);
            }
            print_spreads(&game, &world);
            break;
        };
        let pre_money = game.player.money();
        let pre_pos = game.player.pos();
        match &input {
            Input::Move(_) => move_count += 1,
            Input::Rest => rest_count += 1,
            Input::Use(_) => use_count += 1,
            Input::Talk(_, _) => talked += 1,
            Input::PinClue(_) | Input::UnpinClue(_) => {}
            _ => {}
        }
        if hint_at.is_some() && log.len() % 60 == 0 {
            println!(
                "  [{}] day {} at {:?} hyd {:.0} nut {:.0} m{} items {:?}",
                log.len(),
                game.tick.day(),
                game.player.pos(),
                game.survival.hydration,
                game.survival.nutrition,
                game.player.money(),
                game.player.items()
            );
        }
        if hint_at.is_none() && game.tomb_hint().is_some() {
            hint_at = Some((
                log.len(),
                pre_money,
                game.player.item_count("water"),
                game.player.item_count("food"),
                game.survival.hydration as u32,
            ));
        }
        log.push(input.clone());
        game.apply(&input, &world);
        let lost = pre_money.saturating_sub(game.player.money());
        if game.clues.len() > prev_clues.len() {
            let newc: Vec<String> = game.clues.difference(&prev_clues).cloned().collect();
            println!(
                "  [{}] day {} GAINED clue(s) {newc:?} at {pre_pos:?} money {} hyd {:.0}",
                log.len(),
                game.tick.day(),
                game.player.money(),
                game.survival.hydration
            );
            println!(
                "      next_clue would be {:?}",
                next_clue_here(&game, &world)
            );
        }
        prev_clues = game.clues.clone();
        match &input {
            Input::Buy(id) if id == "water" => spent_water += lost,
            Input::Buy(id) if id == "food" => spent_food += lost,
            Input::Buy(id) => {
                spent_other += lost;
                *other_by_item.entry(id.clone()).or_default() += lost;
                other_when.push((log.len(), id.clone(), lost));
            }
            Input::Talk(who, _) => {
                if lost > 0 {
                    spent_talk += lost;
                    tells.push((
                        format!("#{} @ {:?} to {who}", log.len(), pre_pos),
                        format!("-{lost}"),
                    ));
                }
            }
            Input::Travel(_id) => spent_travel += lost,
            _ => {}
        }
        if log.len() >= 212 && log.len() % 150 == 0 {
            println!(
                "  [{}] day {} at {:?} hyd {:.0} nut {:.0} m{} items {:?}",
                log.len(),
                game.tick.day(),
                game.player.pos(),
                game.survival.hydration,
                game.survival.nutrition,
                game.player.money(),
                game.player.items()
            );
        }
    }
    println!("final status {:?} inputs {}", game.status, log.len());
    for (i, inp) in log.iter().enumerate().skip(log.len().saturating_sub(30)) {
        println!("  [{i}] {inp:?}");
    }
    println!("stopped at {} inputs", log.len());
}

fn next_clue_here(game: &Game, _world: &World) -> Option<(String, String)> {
    game.graph
        .clues
        .values()
        .filter(|c| !c.red_herring)
        .filter(|c| !game.clues.contains(&c.id))
        .filter(|c| !game.rivals_consumed.contains(&c.id))
        .find(|c| game.graph.prereqs_met(&c.id, &game.clues))
        .map(|c| (c.id.clone(), c.settlement.clone()))
}

fn print_spreads(game: &Game, world: &World) {
    let here = "kano";
    let mut best_f = None;
    let mut best_r = None;
    for gid in game.content.goods.keys() {
        for s in &world.settlements {
            if let (Some(buy_h), Some(buy_s)) =
                (game.buy_price(here, gid), game.buy_price(&s.id, gid))
            {
                if let Some(sell_s) = game.sell_price(&s.id, gid) {
                    let f = sell_s.saturating_sub(buy_h);
                    if best_f.as_ref().is_none_or(|(_, _, p, _, _)| f > *p) {
                        best_f = Some((gid.clone(), s.id.clone(), f, buy_h, sell_s));
                    }
                }
                if let Some(sell_h) = game.sell_price(here, gid) {
                    let r = sell_h.saturating_sub(buy_s);
                    if best_r.as_ref().is_none_or(|(_, _, p, _, _)| r > *p) {
                        best_r = Some((gid.clone(), s.id.clone(), r, buy_s, sell_h));
                    }
                }
            }
        }
    }
    println!("  best forward (buy here->sell there): {best_f:?}");
    println!("  best reverse (buy there->sell here): {best_r:?}");
    for gid in ["cloth", "coffee", "water", "dates", "salt"] {
        for s in &world.settlements {
            let b = game.buy_price(&s.id, gid);
            let se = game.sell_price(&s.id, gid);
            if b.is_some() {
                println!("    {gid} @ {} buy {b:?} sell {se:?}", s.id);
            }
        }
    }
}
