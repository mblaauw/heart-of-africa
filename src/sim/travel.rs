//! Caravan fast-travel (DESIGN.md §4.2): time + supply cost, scouting,
//! and the telegraphed risky-route encounter. Kept out of game.rs.

use crate::sim::events;
use crate::world::{Route, World};

use super::game::Game;

impl Game {
    /// Caravan fast-travel (DESIGN.md §4.2): time + supply cost; encounter
    /// roll only on routes flagged risky (telegraphed before departure).
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
        let hours = (from.x.abs_diff(to.x) + from.y.abs_diff(to.y)) * 2;

        self.tick.advance_by(hours as u64);
        self.survival.hydration = (self.survival.hydration - hours as f32 * 0.1).max(0.0);
        self.survival.nutrition = (self.survival.nutrition - hours as f32 * 0.1).max(0.0);
        self.player.mark_visited(to.x, to.y);
        self.player.teleport(to.x, to.y);

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
