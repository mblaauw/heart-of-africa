//! Scripted dialogue (DESIGN.md §6.1).
//!
//! Scripted dialogue is the game: every named NPC has a complete authored
//! tree in data. Nodes carry conditions on game state and effects expressed
//! as the same action set the LLM garnish (§6.4) would use. The tree runner
//! is pure: it asks a `DialogueView` about game state and returns effects for
//! the caller to apply, so it stays headless-testable.

pub mod llm;
pub mod npc;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use npc::{DialogueOption, NodeId, Npc};

/// What the dialogue runner needs to know about the game state. Implemented
/// by `GameSnapshot`; keeping this a trait keeps dialogue decoupled.
pub trait DialogueView {
    /// Reputation in the given settlement.
    fn reputation(&self, settlement: &str) -> i32;
    /// Whether the player carries at least one unit of the item.
    fn has_item(&self, item_id: &str) -> bool;
    /// Whether the clue has already been obtained.
    fn has_clue(&self, clue_id: &str) -> bool;
}

/// The single visibility rule: conditions hold, and the option does not
/// re-reveal an already-obtained clue. Shared by `visible_options` and
/// `choose` so the two can never drift apart.
fn filtered_options<'a>(
    node: &'a npc::Node,
    settlement: &str,
    view: &dyn DialogueView,
) -> Vec<&'a DialogueOption> {
    node.options
        .iter()
        .filter(|o| o.condition.is_met(settlement, view))
        .filter(|o| {
            o.effect
                .reveal_clue
                .as_ref()
                .is_none_or(|id| !view.has_clue(id))
        })
        .collect()
}

/// An owned snapshot of the game state a dialogue needs. The caller builds
/// it so the runner never borrows the game while it mutates the conversation.
#[derive(Debug, Clone, Default)]
pub struct GameSnapshot {
    pub reputation: BTreeMap<String, i32>,
    pub items: BTreeMap<String, u32>,
    pub clues: BTreeSet<String>,
}

impl DialogueView for GameSnapshot {
    fn reputation(&self, settlement: &str) -> i32 {
        self.reputation.get(settlement).copied().unwrap_or(0)
    }
    fn has_item(&self, item_id: &str) -> bool {
        self.items.get(item_id).copied().unwrap_or(0) > 0
    }
    fn has_clue(&self, clue_id: &str) -> bool {
        self.clues.contains(clue_id)
    }
}

/// An active conversation with one NPC.
///
/// Invariant: the NPC must exist in content, and `node` is always a node of
/// that NPC. Ends (`ended == true`) when a chosen option says so; the caller
/// then drops the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub npc_id: String,
    node: NodeId,
    ended: bool,
}

impl Conversation {
    /// Start a new conversation at the NPC's opening node.
    pub fn start(npc_id: String) -> Self {
        Conversation {
            npc_id,
            node: NodeId::Opening,
            ended: false,
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The current node of the conversation.
    pub fn node<'a>(&self, npc: &'a Npc) -> &'a npc::Node {
        match &self.node {
            NodeId::Opening => npc.opening(),
            NodeId::Named(id) => npc
                .node(id.as_str())
                .expect("conversation node always exists"),
        }
    }

    /// Options the player can currently choose: those whose conditions hold,
    /// excluding options that would re-reveal an already-obtained clue.
    pub fn visible_options<'a>(
        &'a self,
        npc: &'a Npc,
        view: &dyn DialogueView,
    ) -> Vec<&'a DialogueOption> {
        filtered_options(self.node(npc), &npc.settlement, view)
    }

    /// Jump to a named node (used for the runtime tomb answer).
    pub fn jump_to(&mut self, node_id: String) {
        self.node = NodeId::Named(node_id);
        self.ended = false;
    }

    /// Choose the option at `index` (among *visible* options). Returns the
    /// option's effect for the caller to apply, moves to the next node, or
    /// ends the conversation. `None` on an out-of-range index.
    pub fn choose(
        &mut self,
        npc: &Npc,
        view: &dyn DialogueView,
        index: usize,
    ) -> Option<npc::DialogueOption> {
        // Resolve the current node from the NPC (no self borrow), filter its
        // options, then mutate the conversation position.
        let node = match &self.node {
            NodeId::Opening => npc.opening(),
            NodeId::Named(id) => npc
                .node(id.as_str())
                .expect("conversation node always exists"),
        };
        let visible = filtered_options(node, &npc.settlement, view);
        let chosen = (*visible.get(index)?).clone();
        match &chosen.go_to {
            Some(id) => self.node = NodeId::Named(id.clone()),
            None => self.ended = true,
        }
        Some(chosen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc::{Condition, Effect};

    fn npc() -> Npc {
        // greet -> intro; greet has a gated clue option and a gift option.
        Npc {
            id: "test_npc".into(),
            name: "Test".into(),
            settlement: "cairo".into(),
            opening: "greet".into(),
            tomb_answer: "intro".into(),
            gift: None,
            nodes: std::collections::BTreeMap::from([
                (
                    "greet".into(),
                    npc::Node {
                        id: "greet".into(),
                        text: "Hello.".into(),
                        options: vec![
                            DialogueOption {
                                text: "Who are you?".into(),
                                go_to: Some("intro".into()),
                                condition: Condition::default(),
                                effect: Effect::default(),
                            },
                            DialogueOption {
                                text: "The tomb.".into(),
                                go_to: None,
                                condition: Condition {
                                    rep_ge: Some(5),
                                    ..Default::default()
                                },
                                effect: Effect {
                                    reveal_clue: Some("nile.c1".into()),
                                    ..Default::default()
                                },
                            },
                            DialogueOption {
                                text: "Farewell.".into(),
                                go_to: None,
                                condition: Condition::default(),
                                effect: Effect {
                                    end: true,
                                    ..Default::default()
                                },
                            },
                        ],
                    },
                ),
                (
                    "intro".into(),
                    npc::Node {
                        id: "intro".into(),
                        text: "I am the test.".into(),
                        options: vec![DialogueOption {
                            text: "Farewell.".into(),
                            go_to: None,
                            condition: Condition::default(),
                            effect: Effect {
                                end: true,
                                ..Default::default()
                            },
                        }],
                    },
                ),
            ]),
        }
    }

    struct View {
        rep: i32,
        clue: bool,
    }
    impl DialogueView for View {
        fn reputation(&self, _settlement: &str) -> i32 {
            self.rep
        }
        fn has_item(&self, _item_id: &str) -> bool {
            false
        }
        fn has_clue(&self, _clue_id: &str) -> bool {
            self.clue
        }
    }

    #[test]
    fn conditions_filter_options() {
        let npc = npc();
        let view = View {
            rep: 3,
            clue: false,
        };
        let c = Conversation::start("test_npc".into());
        let options = c.visible_options(&npc, &view);
        assert_eq!(options.len(), 2); // intro + farewell; clue gated out

        let view = View {
            rep: 5,
            clue: false,
        };
        let options = c.visible_options(&npc, &view);
        assert_eq!(options.len(), 3);
    }

    #[test]
    fn choosing_moves_nodes_and_applies_effects() {
        let npc = npc();
        let view = View {
            rep: 5,
            clue: false,
        };
        let mut c = Conversation::start("test_npc".into());

        // Option 1 ("The tomb.") reveals the clue; no go_to means the talk
        // ends here.
        let effect = c.choose(&npc, &view, 1).unwrap();
        assert_eq!(effect.effect.reveal_clue.as_deref(), Some("nile.c1"));
        assert!(c.ended());
    }

    #[test]
    fn choosing_navigation_and_end() {
        let npc = npc();
        let view = View {
            rep: 0,
            clue: false,
        };
        let mut c = Conversation::start("test_npc".into());

        let effect = c.choose(&npc, &view, 0).unwrap(); // "Who are you?"
        assert!(effect.go_to.is_some());
        assert_eq!(c.node(&npc).text, "I am the test.");

        c.choose(&npc, &view, 0).unwrap(); // "Farewell."
        assert!(c.ended());
    }

    #[test]
    fn out_of_range_choice_is_none() {
        let npc = npc();
        let view = View {
            rep: 0,
            clue: false,
        };
        let mut c = Conversation::start("test_npc".into());
        assert!(c.choose(&npc, &view, 99).is_none());
        assert!(!c.ended());
    }
}
