//! Conversation glue (DESIGN.md §6.1): the Game-side half of dialogue —
//! applying scripted and LLM effects, the tomb question runtime slot, and
//! the LLM turn context. Kept out of game.rs so no module balloons.

use crate::dialogue::npc::{Effect, Npc};
use crate::dialogue::{Conversation, GameSnapshot};
use crate::sim::clues::Clue;
use crate::world::World;

use super::game::{Game, TalkScreen};

impl Game {
    /// The current conversation turn, or None.
    pub fn talk_screen(&self) -> Option<TalkScreen> {
        let conv = self.conversation.as_ref()?;
        let npc = self.content.npcs.get(&conv.npc_id)?;
        let snapshot = self.snapshot_view();
        let options = conv
            .visible_options(npc, &snapshot)
            .iter()
            .map(|o| o.text.clone())
            .collect();
        Some(TalkScreen {
            npc_id: conv.npc_id.clone(),
            npc_name: npc.name.clone(),
            text: conv.node(npc).text.clone(),
            options,
            tomb_question: self.tomb_question_available(npc),
        })
    }

    /// The NPCs present at the current settlement, for the UI.
    pub fn npcs_here<'a>(&'a self, world: &'a World) -> Vec<&'a Npc> {
        let Some(settlement) = self.settlement_at(world) else {
            return Vec::new();
        };
        self.content
            .npcs
            .values()
            .filter(|npc| npc.settlement == settlement.id)
            .collect()
    }

    pub(crate) fn apply_talk(
        &mut self,
        npc_id: &str,
        option_index: usize,
        world: &World,
    ) -> Vec<String> {
        let Some(npc) = self.content.npcs.get(npc_id).cloned() else {
            return Vec::new();
        };
        // Conversations happen in the NPC's settlement.
        if self.settlement_at(world).map(|s| s.id.as_str()) != Some(npc.settlement.as_str()) {
            return Vec::new();
        }
        let mut events = Vec::new();

        // First word (or switching NPCs) opens the conversation.
        if self
            .conversation
            .as_ref()
            .is_none_or(|c| c.npc_id != npc_id)
        {
            self.conversation = Some(Conversation::start(npc_id.to_string()));
            let conv = self.conversation.as_ref().expect("just started");
            events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
            return events;
        }

        let authored = self
            .conversation
            .as_ref()
            .expect("conversation exists")
            .visible_options(&npc, &self.snapshot_view())
            .len();

        if option_index == authored {
            // The runtime tomb question (last option).
            if !self.tomb_question_available(&npc) {
                return Vec::new();
            }
            events.extend(self.ask_tomb(&npc));
            if !npc.tomb_answer.is_empty() {
                self.conversation
                    .as_mut()
                    .expect("conversation exists")
                    .jump_to(npc.tomb_answer.clone());
                let conv = self.conversation.as_ref().expect("just moved");
                events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
            } else {
                self.conversation = None;
            }
            return events;
        }

        // An authored option.
        let (effect, ended) = {
            let snapshot = self.snapshot_view();
            let conv = self.conversation.as_mut().expect("conversation exists");
            match conv.choose(&npc, &snapshot, option_index) {
                Some(option) => (option.effect, conv.ended()),
                None => return Vec::new(),
            }
        };
        events.extend(self.apply_effect(&effect, &npc));
        if ended {
            self.conversation = None;
        } else {
            let conv = self.conversation.as_ref().expect("not ended");
            events.push(format!("{}: {}", npc.name, conv.node(&npc).text));
        }
        events
    }

    /// Whether the runtime "ask about the tomb" question is available from
    /// this NPC: they hold a graph clue that is not yet obtained, not taken
    /// by a rival, with prerequisites and reputation met.
    fn tomb_question_available(&self, npc: &Npc) -> bool {
        self.pending_clue_of_npc_inner(&npc.id).is_some()
    }

    /// The clue this NPC can currently reveal. An NPC may hold several clues
    /// (e.g. a herring next to a real one); the first askable one in id
    /// order wins: not obtained, not consumed by a rival, prerequisites and
    /// rep gate met, and the required item carried (DESIGN.md §4.5 keys).
    pub(crate) fn pending_clue_of_npc_inner(&self, npc_id: &str) -> Option<&Clue> {
        self.graph
            .clues
            .values()
            .filter(|c| c.npc == npc_id)
            .find(|c| {
                !self.clues.contains(&c.id)
                    && !self.rivals_consumed.contains(&c.id)
                    && self.graph.prereqs_met(&c.id, &self.clues)
                    && self.reputation(&c.settlement) >= c.rep_gate
                    && c.requires_item
                        .as_ref()
                        .is_none_or(|item| self.player.item_count(item) > 0)
            })
    }

    /// Ask about the tomb: reveal the NPC's pending clue (the one runtime
    /// action the seeded graph controls).
    fn ask_tomb(&mut self, npc: &Npc) -> Vec<String> {
        let Some(clue_id) = self
            .pending_clue_of_npc_inner(&npc.id)
            .map(|c| c.id.clone())
        else {
            return Vec::new();
        };
        let effect = Effect {
            reveal_clue: Some(clue_id),
            ..Default::default()
        };
        self.apply_effect(&effect, npc)
    }

    /// The LLM turn context (DESIGN.md §6.3): identity, relationship, the
    /// scripted node, and the fact whitelist. `None` when no conversation
    /// is active.
    pub fn llm_context(&self) -> Option<crate::dialogue::llm::LlmContext> {
        let conv = self.conversation.as_ref()?;
        let npc = self.content.npcs.get(&conv.npc_id)?;
        Some(crate::dialogue::llm::LlmContext {
            npc_id: conv.npc_id.clone(),
            npc_name: npc.name.clone(),
            settlement: npc.settlement.clone(),
            reputation: self.reputation(&npc.settlement),
            node_text: conv.node(npc).text.clone(),
            options: self.talk_screen().map(|t| t.options).unwrap_or_default(),
            pending_clue: self
                .pending_clue_of_npc_inner(&npc.id)
                .map(|c| (c.id.clone(), c.text.clone())),
        })
    }

    /// The validator gates at reply time: the pending clue and every clue
    /// text (for the leak scan).
    pub fn llm_gates(&self) -> crate::dialogue::llm::LlmGate {
        crate::dialogue::llm::LlmGate {
            pending_clue: self
                .conversation
                .as_ref()
                .and_then(|conv| self.pending_clue_of_npc_inner(&conv.npc_id))
                .map(|c| (c.id.clone(), c.text.clone())),
            all_clue_texts: self.graph.clues.values().map(|c| c.text.clone()).collect(),
        }
    }

    /// An owned view of the game state for the dialogue runner.
    pub(crate) fn snapshot_view(&self) -> GameSnapshot {
        GameSnapshot {
            reputation: self.reputation.clone(),
            items: self.player.items().clone(),
            clues: self.clues.clone(),
        }
    }
}
