//! NPC dialogue data types (DESIGN.md §6.1, §6.4).
//!
//! `NodeId` distinguishes the opening node from named nodes. Conditions gate
//! options on game state; effects are the closed action set the scripted
//! tree (and, later, the LLM validator) may emit: `reveal_clue`,
//! `take_item`/`give_item`, `rep_delta`, `end`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dialogue::DialogueView;

/// Reference to a conversation node: the NPC's opening node or a named one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeId {
    Opening,
    Named(String),
}

/// One named NPC: identity card + a scripted dialogue tree (DESIGN.md §4.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npc {
    pub id: String,
    pub name: String,
    /// The settlement where this NPC is met.
    pub settlement: String,
    /// Node the conversation starts at.
    pub opening: String,
    /// All named nodes, by ID.
    pub nodes: BTreeMap<String, Node>,
}

impl Npc {
    pub fn opening(&self) -> &Node {
        self.nodes
            .get(&self.opening)
            .expect("content validation guarantees the opening node exists")
    }

    /// A named node; `None` when it does not exist (content validation
    /// guarantees it does for every referenced node).
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }
}

/// One dialogue node: the NPC's line plus the player's options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub text: String,
    pub options: Vec<DialogueOption>,
}

/// A player option: what is shown, where it leads, and under which
/// conditions. When `go_to` is `None`, choosing the option ends the talk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogueOption {
    pub text: String,
    #[serde(default)]
    pub go_to: Option<String>,
    #[serde(default)]
    pub condition: Condition,
    #[serde(default)]
    pub effect: Effect,
}

/// Conditions on game state (all optional; an empty condition always holds).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    /// Reputation required in the NPC's settlement.
    #[serde(default)]
    pub rep_ge: Option<i32>,
    /// Item the player must carry.
    #[serde(default)]
    pub has_item: Option<String>,
    /// Clue the player must already have.
    #[serde(default)]
    pub has_clue: Option<String>,
}

impl Condition {
    /// Whether this condition holds in the current game state.
    pub fn is_met(&self, settlement: &str, view: &dyn DialogueView) -> bool {
        if let Some(rep) = self.rep_ge
            && view.reputation(settlement) < rep
        {
            return false;
        }
        if let Some(item) = &self.has_item
            && !view.has_item(item)
        {
            return false;
        }
        if let Some(clue) = &self.has_clue
            && !view.has_clue(clue)
        {
            return false;
        }
        true
    }
}

/// The closed action set (DESIGN.md §6.4), as used by scripted effects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    #[serde(default)]
    pub reveal_clue: Option<String>,
    /// Remove one unit of an item from the player (e.g. a gift).
    #[serde(default)]
    pub take_item: Option<String>,
    /// Grant one unit of an item (e.g. the brass key).
    #[serde(default)]
    pub give_item: Option<String>,
    /// Reputation delta in the NPC's settlement.
    #[serde(default)]
    pub rep_delta: Option<i32>,
    /// End the conversation (implied when `go_to` is absent).
    #[serde(default)]
    pub end: bool,
}
