//! The LLM garnish (DESIGN.md §6): a narration skin over scripted dialogue.
//!
//! It owns no state, no facts, no outcomes. When enabled, it re-phrases the
//! current conversation node; the ONLY action it may emit is revealing the
//! pending clue (the same one the scripted tree would reveal), validated
//! against the same gates. Malformed JSON, smuggled clue text, and
//! disallowed actions all fall back to the scripted line (§6.4, fail
//! closed). Validated actions are recorded in the input log as
//! `Input::LlmAction`, so replays stay byte-identical with the LLM on (§9).
//!
//! The HTTP client is behind the `llm` cargo feature; the validator and
//! cache are always compiled and headless-testable.

use std::collections::HashMap;

#[cfg(feature = "llm")]
use std::sync::mpsc::{Receiver, channel};
#[cfg(feature = "llm")]
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dialogue::npc::Effect;

/// [`llm`] section of config.toml (DESIGN.md §6.2). Defaults keep the
/// garnish OFF; the API key comes from `HOA_OPENCODE_API_KEY`, never files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub timeout_ms: u64,
    pub max_output_tokens: u32,
    pub cache: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            enabled: false,
            provider: "opencode".into(),
            model: "deepseek-v4-flash".into(),
            base_url: "https://opencode.ai/zen/go/v1".into(),
            timeout_ms: 6000,
            max_output_tokens: 300,
            cache: true,
        }
    }
}

impl LlmConfig {
    /// Read `[llm]` from `config.toml` at the repo root; defaults otherwise.
    pub fn load() -> LlmConfig {
        let Ok(text) = std::fs::read_to_string("config.toml") else {
            return LlmConfig::default();
        };
        #[derive(Deserialize)]
        struct File {
            llm: Option<LlmConfig>,
        }
        toml::from_str::<File>(&text)
            .ok()
            .and_then(|f| f.llm)
            .unwrap_or_default()
    }

    /// The API key from the environment; absent = the garnish stays silent.
    pub fn api_key() -> Option<String> {
        std::env::var("HOA_OPENCODE_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
    }
}

/// Everything the model may know about the current turn, assembled from the
/// game state (DESIGN.md §6.3): identity, relationship, the scripted node,
/// and the fact whitelist. Pure data — the sim never touches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmContext {
    pub npc_id: String,
    pub npc_name: String,
    pub settlement: String,
    pub reputation: i32,
    /// The scripted node the model may re-phrase.
    pub node_text: String,
    /// The player's current options (for the model to nod toward).
    pub options: Vec<String>,
    /// The only fact the model may reveal: (id, whitelisted text).
    pub pending_clue: Option<(String, String)>,
}

impl LlmContext {
    /// Stable identity of this turn, for the cache and the UI.
    pub fn key(&self) -> String {
        format!(
            "{}:{:016x}",
            self.npc_id,
            fnv(&[
                &self.npc_name,
                &self.settlement,
                &self.reputation.to_string(),
                &self.node_text,
                &self.options.join("|"),
                &self
                    .pending_clue
                    .as_ref()
                    .map_or(String::new(), |(id, _)| id.clone()),
            ])
        )
    }
}

/// The validators' view of the world at reply time (the same gates the
/// scripted tree uses, §6.4).
#[derive(Debug, Clone)]
pub struct LlmGate {
    /// The clue this turn may reveal: (id, whitelisted text).
    pub pending_clue: Option<(String, String)>,
    /// All clue texts in the graph — used by the leak scan.
    pub all_clue_texts: Vec<String>,
}

/// A validated LLM reply: prose plus effects that passed every gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedReply {
    pub say: String,
    /// Only ever `reveal_clue` of the pending clue (or empty).
    pub actions: Vec<Effect>,
}

/// Cap on `say` length; longer replies are discarded (fail closed).
pub const SAY_MAX_CHARS: usize = 400;

/// Extract the model's message content from an OpenAI-compatible chat
/// completion envelope (`choices[0].message.content`). The content is the
/// strict JSON of the response contract; the envelope is transport.
pub fn parse_response(raw: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Envelope {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(Deserialize)]
    struct Message {
        content: Option<String>,
    }
    let envelope: Envelope = serde_json::from_str(raw).ok()?;
    envelope.choices.first()?.message.content.clone()
}

/// The closed action set of §6.4, as the validator knows it.
const KNOWN_ACTION_TYPES: &[&str] = &[
    "reveal_clue",
    "offer_quest",
    "adjust_reputation",
    "propose_trade",
    "end_conversation",
];

/// Validate a raw model reply against the gates. Returns `None` (scripted
/// fallback) on: unparseable JSON, missing `say`, an over-long `say`, or a
/// leak — any non-whitelisted clue text inside `say`. Disallowed actions
/// are dropped individually, keeping `say` (§6.4).
pub fn validate(raw: &str, gates: &LlmGate) -> Option<ValidatedReply> {
    #[derive(Deserialize)]
    struct RawReply {
        say: Option<String>,
        #[serde(default)]
        actions: Vec<RawAction>,
    }
    #[derive(Deserialize)]
    struct RawAction {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        clue_id: Option<String>,
    }

    let raw: RawReply = serde_json::from_str(raw).ok()?;
    let say = raw.say?;
    if say.chars().count() > SAY_MAX_CHARS {
        return None;
    }
    // Leak scan: the model may only repeat the whitelisted clue text.
    let allowed = gates.pending_clue.as_ref().map(|(_, text)| text.as_str());
    for text in &gates.all_clue_texts {
        if Some(text.as_str()) != allowed && say.contains(text) {
            return None;
        }
    }

    let mut actions = Vec::new();
    for action in raw.actions {
        if !KNOWN_ACTION_TYPES.contains(&action.kind.as_str()) {
            continue; // not in the closed set
        }
        // This turn permits exactly one action: revealing the pending clue.
        let Some((pending_id, _)) = &gates.pending_clue else {
            continue;
        };
        if action.kind == "reveal_clue" && action.clue_id.as_deref() == Some(pending_id.as_str()) {
            actions.push(Effect {
                reveal_clue: Some(pending_id.clone()),
                ..Default::default()
            });
        }
        // Anything else is dropped.
    }

    Some(ValidatedReply { say, actions })
}

/// Per-session response cache keyed by (npc_id, context hash) (§6.5): the
/// same turn is always answered the same way within a session. `None`
/// entries cache failures too, so a flaky reply never flickers.
#[derive(Debug, Default)]
pub struct LlmCache {
    entries: HashMap<String, Option<ValidatedReply>>,
}

impl LlmCache {
    pub fn new() -> Self {
        LlmCache::default()
    }

    pub fn lookup(&self, key: &str) -> Option<Option<ValidatedReply>> {
        self.entries.get(key).cloned()
    }

    pub fn store(&mut self, key: String, reply: Option<ValidatedReply>) {
        self.entries.insert(key, reply);
    }
}

/// Build the OpenAI-compatible request body (raw JSON — no extra features).
pub fn build_request(config: &LlmConfig, ctx: &LlmContext) -> String {
    let system = "\
You are an NPC in an 1890s African expedition game. Stay in period, stay in \
character, speak plainly and respectfully. Never mention game mechanics, \
stats, or the player's reputation. You know only what the scene gives you. \
Reply with STRICT JSON only, no prose around it: \
{\"say\": \"your in-character reply\", \"actions\": [{\"type\": \"reveal_clue\", \
\"clue_id\": \"...\"}]}. \
\"actions\" may be empty; the only action ever allowed is revealing the clue \
named in the scene, phrased in your own words.";

    let whitelist = ctx.pending_clue.as_ref().map_or_else(
        || "You hold no secret to reveal this turn.".to_string(),
        |(id, text)| {
            format!(
                "You may reveal this secret (clue_id {id:?}) if the traveler \
                 earns it, phrased in your own words: \"{text}\""
            )
        },
    );

    let user = format!(
        "You are {name}, in {settlement}. \
         The traveler's standing here: {rep}. \
         Your current line to rephrase: \"{node}\". \
         Their options: {options}. \
         {whitelist} \
         Keep the reply short (under 120 words).",
        name = ctx.npc_name,
        settlement = ctx.settlement,
        rep = ctx.reputation,
        node = ctx.node_text,
        options = if ctx.options.is_empty() {
            "(none)".to_string()
        } else {
            ctx.options.join(" | ")
        },
    );

    serde_json::json!({
        "model": config.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "max_tokens": config.max_output_tokens,
    })
    .to_string()
}

/// The blocking HTTP call; only compiled with the `llm` feature (§8.4:
/// ureq on a background thread, no async runtime).
#[cfg(feature = "llm")]
pub fn request(config: &LlmConfig, key: &str, body: &str) -> Result<String, String> {
    let url = format!("{}/chat/completions", config.base_url);
    let response = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(config.timeout_ms)))
        .build()
        .new_agent()
        .post(&url)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(|e| e.to_string())?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// Dispatch a request on a background thread; the UI polls the receiver
/// each frame ("…considering" meanwhile; walking away drops it).
#[cfg(feature = "llm")]
pub fn spawn_request(
    config: LlmConfig,
    _key: String,
    body: String,
) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = LlmConfig::api_key()
            .map(|k| request(&config, &k, &body))
            .unwrap_or_else(|| Err("no HOA_OPENCODE_API_KEY".to_string()));
        let _ = tx.send(result);
    });
    rx
}

/// Without the `llm` feature the garnish is structurally absent: a receiver
/// whose sender is already dropped, so the first poll fails closed to the
/// scripted line. No thread, no HTTP, no key.
#[cfg(not(feature = "llm"))]
pub fn spawn_request(
    _config: LlmConfig,
    _key: String,
    _body: String,
) -> std::sync::mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    rx
}

/// FNV-1a over the joined parts (deterministic context hashing).
fn fnv(parts: &[&String]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> LlmContext {
        LlmContext {
            npc_id: "cairo_hakim".into(),
            npc_name: "Hakim Ahmed".into(),
            settlement: "Cairo".into(),
            reputation: 5,
            node_text: "The scholar looks up.".into(),
            options: vec!["Farewell.".into()],
            pending_clue: Some(("nile.c1".into(), "The trail leads to Asyut.".into())),
        }
    }

    fn gates() -> LlmGate {
        LlmGate {
            pending_clue: Some(("nile.c1".into(), "The trail leads to Asyut.".into())),
            all_clue_texts: vec![
                "The trail leads to Asyut.".into(),
                "The key is in Luxor.".into(),
                "The tomb is in the hills.".into(),
            ],
        }
    }

    #[test]
    fn valid_reply_passes() {
        let raw = r#"{"say": "Follow the old road south.", "actions": [{"type": "reveal_clue", "clue_id": "nile.c1"}]}"#;
        let reply = validate(raw, &gates()).expect("valid reply accepted");
        assert_eq!(reply.say, "Follow the old road south.");
        assert_eq!(reply.actions.len(), 1);
        assert_eq!(reply.actions[0].reveal_clue.as_deref(), Some("nile.c1"));
    }

    #[test]
    fn invalid_json_falls_back() {
        assert!(validate("not json at all", &gates()).is_none());
        assert!(validate(r#"{"say": "x""#, &gates()).is_none());
        assert!(validate(r#"{"actions": []}"#, &gates()).is_none()); // no say
    }

    #[test]
    fn smuggled_clue_text_is_rejected() {
        // Exact string match against a non-whitelisted clue text (§6.4).
        let raw = r#"{"say": "Psst — The key is in Luxor. Follow it."}"#;
        assert!(validate(raw, &gates()).is_none(), "leak must fail closed");
    }

    #[test]
    fn whitelisted_text_may_be_repeated() {
        let raw = r#"{"say": "The trail leads to Asyut, traveler."}"#;
        assert!(validate(raw, &gates()).is_some());
    }

    #[test]
    fn overlong_say_is_rejected() {
        let raw = format!(r#"{{"say": "{}"}}"#, "x".repeat(SAY_MAX_CHARS + 1));
        assert!(validate(&raw, &gates()).is_none());
    }

    #[test]
    fn disallowed_actions_are_dropped_but_say_kept() {
        let raw = r#"{"say": "Hello.", "actions": [
            {"type": "reveal_clue", "clue_id": "nile.c2"},
            {"type": "adjust_reputation", "delta": 5},
            {"type": "explode"}
        ]}"#;
        let reply = validate(raw, &gates()).expect("say survives bad actions");
        assert!(reply.actions.is_empty());
    }

    #[test]
    fn no_pending_clue_means_no_actions() {
        let mut gates = gates();
        gates.pending_clue = None;
        let raw =
            r#"{"say": "Hello.", "actions": [{"type": "reveal_clue", "clue_id": "nile.c1"}]}"#;
        let reply = validate(raw, &gates).expect("valid");
        assert!(reply.actions.is_empty());
    }

    #[test]
    fn context_key_is_stable_and_distinct() {
        let a = ctx();
        let mut b = ctx();
        assert_eq!(a.key(), b.key());
        b.reputation = 3;
        assert_ne!(a.key(), b.key());
        b = ctx();
        b.pending_clue = None;
        assert_ne!(a.key(), b.key());
    }

    #[test]
    fn parse_response_extracts_message_content() {
        let envelope = r#"{
            "choices": [ { "message": { "content": "{\"say\": \"Hello.\", \"actions\": []}" } } ]
        }"#;
        assert_eq!(
            parse_response(envelope).as_deref(),
            Some(r#"{"say": "Hello.", "actions": []}"#)
        );
        assert!(parse_response("not json").is_none());
        assert!(parse_response(r#"{"choices": []}"#).is_none());
        assert!(parse_response(r#"{"choices": [{"message": {}}]}"#).is_none());
    }

    #[test]
    fn cache_round_trip() {
        let mut cache = LlmCache::new();
        let key = ctx().key();
        assert_eq!(cache.lookup(&key), None);
        let reply = ValidatedReply {
            say: "Hi".into(),
            actions: Vec::new(),
        };
        cache.store(key.clone(), Some(reply.clone()));
        assert_eq!(cache.lookup(&key), Some(Some(reply)));
        cache.store(key.clone(), None);
        assert_eq!(cache.lookup(&key), Some(None));
    }

    #[test]
    fn default_config_is_off() {
        let config = LlmConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.model, "deepseek-v4-flash");
    }
}
