//! Content validation (DESIGN.md §9): schema checks and ID resolution across
//! all content. `cargo test` fails on any problem reported here — that is the
//! guardrail that lets content scale without reviewing every line.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::sim::clues::ClueKind;

use super::load;

/// Validate all content under `content_dir` (world + game content).
///
/// Returns a list of problems; an empty list means the content is valid.
/// Schema errors and pixel-legend mismatches fail hard (they bubble up as
/// `Err`), while structural problems (bad IDs, off-map positions, rivers not
/// on water, dangling dialogue references) are collected and returned.
pub fn validate_content(content_dir: &Path) -> Result<Vec<String>> {
    let world = load::load_world(content_dir)?;
    let mut problems = validate_world(&world);

    let content = load::load_game_content(content_dir)?;
    problems.extend(validate_game_content(&world, &content));

    Ok(problems)
}

/// World geometry and settlement checks (map, rivers, settlements).
fn validate_world(world: &crate::world::World) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();

    if world.settlements.is_empty() {
        problems.push("no settlements defined".to_string());
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for s in &world.settlements {
        if !seen.insert(s.id.as_str()) {
            problems.push(format!("duplicate settlement id {:?}", s.id));
        }
        if !world.grid.in_bounds(s.x, s.y) {
            problems.push(format!(
                "settlement {:?} at ({}, {}) is outside the map",
                s.id, s.x, s.y
            ));
        }
    }

    if world.start().is_none() {
        problems.push(format!(
            "region start_settlement {:?} does not resolve to a settlement",
            world.start_settlement
        ));
    }

    let mut river_ids: HashSet<&str> = HashSet::new();
    for r in &world.rivers {
        if !river_ids.insert(r.id.as_str()) {
            problems.push(format!("duplicate river id {:?}", r.id));
        }
        if r.points.is_empty() {
            problems.push(format!("river {:?} has no points", r.id));
        }
        for &(x, y) in &r.points {
            if !world.grid.in_bounds(x, y) {
                problems.push(format!(
                    "river {:?} point ({x},{y}) is outside the map",
                    r.id
                ));
            } else if world.grid.passable_at(x, y) {
                problems.push(format!(
                    "river {:?} point ({x},{y}) is not on a water tile",
                    r.id
                ));
            }
        }
    }

    problems
}

/// Goods, markets, clues, and dialogue reference checks.
fn validate_game_content(
    world: &crate::world::World,
    content: &crate::sim::game::GameContent,
) -> Vec<String> {
    use crate::sim::economy::Category;

    let mut problems: Vec<String> = Vec::new();

    // Goods: unique ids (map key), sane numbers.
    for goods in content.goods.values() {
        if goods.base_price == 0 {
            problems.push(format!("goods {:?} has base_price 0", goods.id));
        }
        if goods.category == Category::Gift && goods.gift_value <= 0 {
            problems.push(format!("gift goods {:?} has gift_value <= 0", goods.id));
        }
        if goods.restores_hydration > 100.0 || goods.restores_nutrition > 100.0 {
            problems.push(format!(
                "goods {:?} restores more than 100 of a stat",
                goods.id
            ));
        }
    }

    // Markets: settlements and item ids resolve.
    for market in content.markets.values() {
        if world.settlement(&market.settlement_id).is_none() {
            problems.push(format!(
                "market for unknown settlement {:?}",
                market.settlement_id
            ));
        }
        for (item_id, entry) in &market.items {
            if !content.goods.contains_key(item_id) {
                problems.push(format!(
                    "market {:?} lists unknown goods {item_id:?}",
                    market.settlement_id
                ));
            }
            if !(0.25..=4.0).contains(&entry.demand) || !(0.25..=4.0).contains(&entry.supply) {
                problems.push(format!(
                    "market {:?} item {item_id:?} has factors outside 0.25..4.0",
                    market.settlement_id
                ));
            }
        }
    }

    // Clue generator inputs: settlements exist and host NPCs, templates
    // cover every (kind, herring) combination, tomb candidates are passable.
    if let Some(gen_input) = &content.generator {
        let config = &gen_input.config;
        if config.settlements.is_empty() {
            problems.push("generator: no settlements configured".to_string());
        }
        for settlement in &config.settlements {
            if world.settlement(settlement).is_none() {
                problems.push(format!("generator: unknown settlement {settlement:?}"));
            } else if !content
                .npcs
                .values()
                .any(|npc| &npc.settlement == settlement)
            {
                problems.push(format!(
                    "generator: settlement {settlement:?} has no named NPC to hold clues"
                ));
            }
        }
        if !config.settlements.contains(&world.start_settlement) {
            problems.push(format!(
                "generator: start settlement {:?} is not among the graph settlements",
                world.start_settlement
            ));
        }
        if !content.goods.contains_key(&config.key_item) {
            problems.push(format!(
                "generator: key_item {:?} is not a good",
                config.key_item
            ));
        }
        if config.rivals.is_empty() {
            problems.push("generator: no rivals configured".to_string());
        }
        if config.rep_gate_min < 0 || config.rep_gate_max < config.rep_gate_min {
            problems.push(format!(
                "generator: bad rep gate range {}..={}",
                config.rep_gate_min, config.rep_gate_max
            ));
        }
        let needed: [(ClueKind, bool); 5] = [
            (ClueKind::Hint, false),
            (ClueKind::Key, false),
            (ClueKind::Corroboration, false),
            (ClueKind::Location, false),
            (ClueKind::Hint, true), // red herrings are hint-shaped
        ];
        for (kind, herring) in needed {
            if !gen_input
                .templates
                .iter()
                .any(|t| t.kind == kind && t.red_herring == herring)
            {
                problems.push(format!(
                    "generator: no template for kind {kind:?} herring={herring}"
                ));
            }
        }
        if gen_input.tomb_candidates.is_empty() {
            problems.push("generator: no tomb candidates".to_string());
        }
        for &(tx, ty) in &gen_input.tomb_candidates {
            if !world.grid.in_bounds(tx, ty) {
                problems.push(format!("tomb candidate ({tx},{ty}) is outside the map"));
            } else if !world.grid.passable_at(tx, ty) {
                problems.push(format!("tomb candidate ({tx},{ty}) is impassable"));
            }
        }
    } else {
        problems.push("no clue generator configured (clues.toml missing)".to_string());
    }

    // NPCs: opening and tomb_answer nodes exist; option targets, items, and
    // condition items resolve. (Clue references in dialogue are gone: the
    // tomb question is a runtime slot of the seeded graph.)
    for npc in content.npcs.values() {
        if world.settlement(&npc.settlement).is_none() {
            problems.push(format!(
                "NPC {:?} lives in unknown settlement {:?}",
                npc.id, npc.settlement
            ));
        }
        if !npc.nodes.contains_key(&npc.opening) {
            problems.push(format!(
                "NPC {:?} opening node {:?} does not exist",
                npc.id, npc.opening
            ));
        }
        if !npc.tomb_answer.is_empty() && !npc.nodes.contains_key(&npc.tomb_answer) {
            problems.push(format!(
                "NPC {:?} tomb_answer node {:?} does not exist",
                npc.id, npc.tomb_answer
            ));
        }
        for node in npc.nodes.values() {
            for option in &node.options {
                if let Some(go_to) = &option.go_to
                    && !npc.nodes.contains_key(go_to)
                {
                    problems.push(format!(
                        "NPC {:?} node {:?} option {:?} targets missing node {go_to:?}",
                        npc.id, node.id, option.text
                    ));
                }
                if let Some(item) = &option.condition.has_item
                    && !content.goods.contains_key(item)
                {
                    problems.push(format!(
                        "NPC {:?} node {:?} condition item {item:?} does not exist",
                        npc.id, node.id
                    ));
                }
                let effect = &option.effect;
                for item in effect.take_item.iter().chain(effect.give_item.iter()) {
                    if !content.goods.contains_key(item) {
                        problems.push(format!(
                            "NPC {:?} node {:?} effect item {item:?} does not exist",
                            npc.id, node.id
                        ));
                    }
                }
                if effect.reveal_clue.is_some() {
                    problems.push(format!(
                        "NPC {:?} node {:?}: authored clue effects are gone; the tomb \
                         question is a runtime slot",
                        npc.id, node.id
                    ));
                }
            }
        }
    }

    // Routes: endpoints resolve; ids unique.
    let mut route_ids: HashSet<&str> = HashSet::new();
    for route in &world.routes {
        if !route_ids.insert(route.id.as_str()) {
            problems.push(format!("duplicate route id {:?}", route.id));
        }
        if world.settlement(&route.from).is_none() {
            problems.push(format!(
                "route {:?} from unknown settlement {:?}",
                route.id, route.from
            ));
        }
        if world.settlement(&route.to).is_none() {
            problems.push(format!(
                "route {:?} to unknown settlement {:?}",
                route.id, route.to
            ));
        }
        if route.from == route.to {
            problems.push(format!(
                "route {:?} connects a settlement to itself",
                route.id
            ));
        }
    }

    problems
}

/// Validate the art manifest (DESIGN.md §5).
///
/// Manifest schema errors fail hard; any file that EXISTS must load as an
/// image. Missing files are allowed by design — placeholder art covers them.
pub fn validate_assets(assets_dir: &Path) -> Result<Vec<String>> {
    let manifest = load::load_assets(assets_dir)?;
    let mut problems: Vec<String> = Vec::new();
    for (id, path) in &manifest.entries {
        if !path.exists() {
            continue; // optional by design; placeholder art replaces it
        }
        if let Err(e) = image::open(path) {
            problems.push(format!(
                "asset {id:?} at {} is not a loadable image: {e}",
                path.display()
            ));
        }
    }
    Ok(problems)
}
