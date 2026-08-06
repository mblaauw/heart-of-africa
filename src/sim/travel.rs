//! Caravan fast-travel (DESIGN.md §4.2): time + supply cost, scouting,
//! and the telegraphed risky-route encounter. Kept out of game.rs.
//!
//! Routes carry committed polyline geometry (precomputed at import time):
//! `path_km` for time calculation and `tiles` for the hour-by-hour sim
//! (visited marking, weather, encounter position). Preview and execution
//! use the same data — they cannot diverge.

use crate::sim::events;
use crate::world::{KM_PER_CELL, Route, World};

use super::game::Game;

impl Game {
    /// Caravan fast-travel (DESIGN.md §4.2): the journey is simulated
    /// hour-by-hour over the committed polyline path at caravan pace
    /// (2 tiles per in-game hour) — the same per-hour survival rule as
    /// walking. Crossed days run the daily director via `apply`. The
    /// encounter roll happens once, only on routes flagged risky
    /// (telegraphed before departure).
    pub(crate) fn apply_travel(&mut self, route_id: &str, world: &World) -> Vec<String> {
        let Some(route) = world.routes.iter().find(|r| r.id == route_id) else {
            return Vec::new();
        };
        if route.blocked {
            return Vec::new();
        }
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        let (from_id, to_id) = if route.from == settlement.id {
            (route.from.as_str(), route.to.as_str())
        } else if route.to == settlement.id {
            (route.to.as_str(), route.from.as_str())
        } else {
            return Vec::new();
        };
        if !self.route_scouted(route, world) {
            return Vec::new();
        }
        let to = world.settlement(to_id).expect("validated route endpoint");
        let _from = world.settlement(from_id).expect("validated route endpoint");

        // Walk the committed polyline tile path (precomputed at import).
        let tiles = &route.tiles;
        if tiles.is_empty() {
            return Vec::new();
        }
        // Caravan pace: 2 tiles per in-game hour; hours derived from
        // committed km length so preview == executed by construction. The
        // tile-path length is also covered (a winding committed polyline
        // must still be walked to its end within the same ride).
        let speed_km_per_hour = 2.0 * KM_PER_CELL;
        let hours = ((route.path_km / speed_km_per_hour).ceil() as u64)
            .max((tiles.len() as u64).div_ceil(2))
            .max(1);

        let tiles_per_hour = 2usize;
        // Ride direction: the committed polyline runs from -> to; reverse it
        // when boarding from the far end, or the ride would land at the
        // wrong settlement.
        let forward: Vec<(u32, u32)> = if to_id == route.to {
            route.tiles.to_vec()
        } else {
            route.tiles.iter().rev().copied().collect()
        };
        for hour in 0..hours {
            let start = hour as usize * tiles_per_hour;
            for &(tx, ty) in forward.iter().skip(start).take(tiles_per_hour) {
                self.player.teleport(tx, ty); // marks visited along the way
                self.advance_travel(world);
            }
        }
        debug_assert_eq!(self.player.pos(), (to.x, to.y));

        let mut events = vec![format!(
            "Caravan to {} — {} hours on the road.",
            to.name, hours
        )];
        if route.risky && events::encounter(self.seed, self.tick.as_hours()) {
            events.push(self.ambush());
        }
        self.after_movement();
        events
    }

    /// A telegraphed risky-route encounter: lose some supplies or money.
    fn ambush(&mut self) -> String {
        // Bandits take a carried supply (never a clue item or gear); if the
        // pack has none, they take money instead.
        let victim = self
            .player
            .items()
            .iter()
            .find(|(id, _)| {
                self.content
                    .goods(id)
                    .is_some_and(|g| g.category == crate::sim::economy::Category::Supply)
            })
            .map(|(id, _)| id.clone());
        if let Some(id) = victim {
            self.player.remove_item(&id);
            let name = self
                .content
                .goods(&id)
                .map_or(id.clone(), |g| g.name.clone());
            format!("Waylaid on the road — the bandits took your {name}.")
        } else {
            let loss = 10;
            self.player.spend(loss);
            format!("Waylaid on the road — the bandits took {loss} piastres.")
        }
    }

    /// A route is scouted when both endpoint settlements have been visited.
    pub(crate) fn route_scouted(&self, route: &Route, world: &World) -> bool {
        let Some(from) = world.settlement(&route.from) else {
            return false;
        };
        let Some(to) = world.settlement(&route.to) else {
            return false;
        };
        self.player.has_visited(from.x, from.y) && self.player.has_visited(to.x, to.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::load::{load_game_content, load_world};

    fn fixture_world() -> World {
        load_world(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/content"
        )))
        .expect("content loads in tests")
    }

    /// Preview == executed: the caravan arrives exactly at the destination
    /// and the whole committed path is walked and visited — the same tiles
    /// the map overlay draws. Hours match the committed km length.
    #[test]
    fn travel_preview_matches_execution_for_all_committed_routes() {
        let world = fixture_world();
        let content = load_game_content(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/content"
        )))
        .expect("game content loads in tests");
        assert!(!world.routes.is_empty(), "content has routes");

        for route in &world.routes {
            let mut game = Game::new(0, content.clone(), &world);
            let from = world.settlement(&route.from).unwrap();
            let to = world.settlement(&route.to).unwrap();
            // Scout both endpoints, then start from `from`.
            game.player.teleport(to.x, to.y);
            game.player.teleport(from.x, from.y);

            let events = game.apply_travel(&route.id, &world);
            assert!(
                events.iter().any(|e| e.starts_with("Caravan to")),
                "route {} announces departure",
                route.id
            );
            assert_eq!(
                game.player.pos(),
                (to.x, to.y),
                "route {} preview == executed (arrives exactly at destination)",
                route.id
            );
            for &(x, y) in &route.tiles {
                assert!(
                    game.player.has_visited(x, y),
                    "route {} walked its committed path (corridor visited)",
                    route.id
                );
            }
            // Hours derive from committed km (preview cost == executed cost).
            let speed = 2.0 * KM_PER_CELL;
            let expected = ((route.path_km / speed).ceil() as u64)
                .max((route.tiles.len() as u64).div_ceil(2))
                .max(1);
            assert!(
                events
                    .iter()
                    .any(|e| e.contains(&format!("{expected} hours"))),
                "route {} reports {expected} hours: {events:?}",
                route.id
            );
        }
    }

    /// An unresolvable route id fails closed: no movement.
    #[test]
    fn travel_for_unknown_route_is_a_noop() {
        let world = fixture_world();
        let content = load_game_content(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/content"
        )))
        .expect("game content loads in tests");
        let mut game = Game::new(0, content, &world);
        let before = game.player.pos();
        let events = game.apply_travel("no_such_route", &world);
        assert!(events.is_empty());
        assert_eq!(game.player.pos(), before);
    }
}
