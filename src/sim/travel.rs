//! Caravan fast-travel (DESIGN.md §4.2): time + supply cost, scouting,
//! and the telegraphed risky-route encounter. Kept out of game.rs.

use crate::sim::events;
use crate::world::{Route, World};

use super::game::Game;

impl Game {
    /// Caravan fast-travel (DESIGN.md §4.2): the journey is simulated
    /// hour-by-hour over the real route path at caravan pace (2 tiles per
    /// in-game hour) — the same per-hour survival rule as walking, so
    /// weather, day/night, ailment exposure, and rest drain apply. Crossed
    /// days run the daily director via `apply`. The encounter roll happens
    /// once, only on routes flagged risky (telegraphed before departure).
    pub(crate) fn apply_travel(&mut self, route_id: &str, world: &World) -> Vec<String> {
        let Some(route) = world.routes.iter().find(|r| r.id == route_id) else {
            return Vec::new();
        };
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
        let from = world.settlement(from_id).expect("validated route endpoint");

        // The straight-line path between the settlements (Manhattan).
        let path = straight_path((from.x, from.y), (to.x, to.y));
        let tiles = path.len() as u64;
        let hours = tiles.div_ceil(2); // caravan pace: 2 tiles/hour

        for hour in 0..hours {
            let start = (hour * 2) as usize;
            for &(tx, ty) in path.iter().skip(start).take(2) {
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

/// The straight-line tile path from `from` to `to` (Manhattan), inclusive.
fn straight_path(from: (u32, u32), to: (u32, u32)) -> Vec<(u32, u32)> {
    let mut path = Vec::new();
    let (mut cx, mut cy) = (from.0 as i64, from.1 as i64);
    path.push((cx as u32, cy as u32));
    let (dx, dy) = (to.0 as i64 - from.0 as i64, to.1 as i64 - from.1 as i64);
    while cx != to.0 as i64 {
        cx += dx.signum();
        path.push((cx as u32, cy as u32));
    }
    while cy != to.1 as i64 {
        cy += dy.signum();
        path.push((cx as u32, cy as u32));
    }
    path
}
