# Heart of Africa — Remake Design Document (v2)

**Working title:** *Heart of Africa* remake (unofficial, inspired-by, not a port)
**Original inspiration:** *Heart of Africa* (1985, Ozark Softscape / Electronic Arts, design by Danielle Bunten Berry)
**Engine/stack:** Rust + Macroquad, single binary, single language, agent-implementable by design
**Status:** Supersedes the v1 draft. All open questions from v1 are resolved in §12.

---

## 0. Revision history — what changed from v1, and why

The v1 draft was solid on game design but had three structural problems this revision addresses:

1. **The LLM was framed backwards.** V1 called it a "layer" with scripted dialogue as "fallback" — that inverts the risk. Here, scripted, data-driven dialogue *is* the dialogue system: complete and winnable entirely on its own. The LLM is an optional garnish behind an `enabled = false` default, cloud-only via the OpenCode API with `deepseek-v4-flash` — no local model, no provider abstraction until a second provider actually exists. The guardrails are fully specified in §6: strict JSON response contract, a closed action set validated per dialogue node, a fact whitelist so the model never receives a clue it isn't allowed to reveal, a leak scan on output text, a hard timeout, and fail-closed-to-scripted-line on any error. LLM actions flow through the *same* code path as scripted node effects, so replays stay deterministic even with the LLM on.
2. **The architecture was over-engineered for its actual implementer.** Ten crates, an async runtime, an ECS, RON, a tilemap dependency, a pathfinding dependency — each one is either ceremony or a training-data blind spot for a coding agent. This revision collapses to one crate with module folders and two binaries, drops async entirely (a blocking HTTP client on a background thread), drops the ECS, swaps RON for TOML/JSON, replaces a map-editor dependency with an indexed-color PNG biome map plus a `world.toml`, and hand-rolls A*. One rule does the architectural heavy lifting: simulation code never imports the render/UI libraries, so the entire game logic runs headless in `cargo test`.
3. **Nothing enforced correctness.** This revision adds a determinism contract with seed-replay tests, a generation-time solvability proof for the clue graph, content validation as `cargo test`, a headless sim bot for pacing/softlock checks, and adversarial fixtures for the LLM validator. Every roadmap phase now has an acceptance criterion.

Everything v1 got right is retained and tightened: real macro-geography, named 1890s polities, telegraphed survival ailments replacing "delirium," the clue *graph*, reputation-gated information, caravan fast-travel over scouted routes, and content-ID-addressed art. Scope is also cut to a shippable v1 — two regions instead of five — with the five-region shape preserved purely as data, so scale-out is content work, not code work. All three of v1's open questions are resolved in §12.

---

## 1. Vision & Design Pillars

An open-world 1890s Africa expedition mystery. You land in Cairo with a dead explorer's diary, a modest purse, and one goal: find the Lost Tomb of Pharaoh Ahnk-Ahnk before your money, your health, or your rivals run out. The tomb — and the entire web of clues pointing to it — is regenerated every playthrough.

**Pillars, in priority order:**

1. **Keep the hook, deepen everything around it.** The clue-driven mystery and earn-your-way-in diplomacy were the soul of the original. Preserve them; multiply the paths through them.
2. **Deterministic core, decorative AI.** Every gameplay outcome is computable from `(seed, player inputs)`. Language models may *phrase* things; they never *decide* things.
3. **Real geography, invented moments.** Macro-map fixed and historically accurate; micro-content (clue graph, NPC placement, weather runs, rival behavior) seeded per playthrough.
4. **NPCs are people, not resource nodes.** Every settlement belongs to a specific, named 1890s polity. Named NPCs have identity cards, memory of you, and gated knowledge.
5. **Content over code.** Regions, NPCs, items, quests, and dialogue are data files validated by tests. Adding a third region should require zero new Rust.
6. **Legible systems over opaque randomness.** No "controls go random." Every danger is named, telegraphed, and counterable.
7. **The codebase is the second product.** It must be small, boring, explicit, and mainstream enough that an LLM coding agent can implement, extend, and debug it reliably (§8).

---

## 2. Original vs. Remake (verdicts table)

| Original element | Verdict | Remake approach |
|---|---|---|
| Clue-driven mystery, chief-by-chief | Core loop, keep | Randomized clue **graph** with red herrings, corroboration requirements, faction-gated nodes |
| Diary auto-log | Loved | Keep, deterministic entries from event data; optional LLM-phrased reflective entries (fact-bound) |
| Gift-for-info diplomacy | Worked but shallow | Persistent per-settlement/faction reputation; gifts are one input among several (favors, escort quests, conduct) |
| Cache system (bury & return) | Elegant | Keep essentially as-is |
| One merchant, flat prices | Thin | 1–3 merchants per settlement; price = base × season × supply/demand × reputation modifier |
| Slow/cumbersome movement | Fair critique | Deliberate first-visit pace; caravan fast-travel on scouted routes |
| "Delirium" random input | Flaw | Named ailments (malaria, dysentery, sunstroke, trypanosomiasis) with visible onset and period-appropriate countermeasures |
| Run-derailing surprise events | Flaw | Events telegraphed: weather fronts visible on map, rumors precede rivals, per-region/season disease risk shown |
| Tomb findable "by accident" | Flaw | Final site requires assembling N corroborating clues; terrain search without them is astronomically unlikely and the game says so in-fiction |
| Icon-hunting UI | Dated | Always-on HUD, hotkeys, grid inventory, searchable diary |
| Save at the pub | Charming friction | File-based saves anywhere; pubs/caravanserais remain as flavor + rest locations |

---

## 3. World & Setting

### 3.1 Macro-geography (fixed, data-authored)
Single continuous tile world at region scale. Five macro-regions are defined in data from day one; **two are populated for v1**:

- **Lower Egypt & the Nile** *(v1)* — Cairo start, Delta, river travel to the Aswan/Khartoum axis.
- **Sahara & Sahel corridor** *(v1)* — trans-Saharan caravan routes, salt/gold trade towns, Lake Chad basin, Sokoto-era polities.
- The Congo Basin, Great Lakes/Swahili coast, and Southern Africa *(post-v1, content-only additions)*.

The macro-map is authored as two source files: a biome/terrain map as an indexed-color PNG (one color = one biome, trivially editable in any paint tool, trivially parsed) and a `world.toml` listing rivers, settlements, routes, and region boundaries with coordinates. No map-editor dependency, no custom binary format.

### 3.2 Historical & cultural framing
Every settlement is tied to a real, named 1890s community or polity. The Scramble for Africa is ambient pressure (checkpoints, rumors, prices, closed routes), never the player's job to resolve. Where history is brutal, the game states it factually in diary/dialogue text; it never dramatizes it, never rewards it, and never casts the player as perpetrator. The player is a private individual dependent on hospitality and permission. A region research pass (real ethnography/history, plus reference text pulled from the original disk via the existing `d64-converter` pipeline) precedes writing any region's content files.

### 3.3 Weather, seasons, ecology
Per-region seasonal model drives: travel speed, river navigability (Nile flood season matters), disease-risk multipliers (wet-season malaria, tsetse belts), NPC schedules, and market supply. Flora/fauna are period- and biome-accurate and feed both atmosphere and survival inputs. Weather is generated from seeded noise over the seasonal baseline, so it varies per playthrough but stays plausible.

---

## 4. Core Gameplay

### 4.1 The expedition loop
Outfit in town → travel (manage water/food/health/party) → arrive at settlement → build standing → extract clue(s)/trade/quests → cache or carry → repeat → triangulate tomb region → final expedition → win/lose. A run is intended to take 6–12 hours.

### 4.2 Movement & travel
Tile-based movement on foot, canoe/felucca, camel/caravan; terrain type × season × party composition sets speed. Fog of war on first visit. Scouted routes unlock **caravan fast-travel**: abstracted time + supply cost, with a telegraphed encounter roll only on routes flagged risky (bandit country, disease belt, war rumor) — safe scouted routes are genuinely safe.

### 4.3 Survival (replaces delirium)
Tracked: hydration, nutrition, rest, and per-ailment status. Ailments: malaria, dysentery, sunstroke, trypanosomiasis — each with region/season/behavior-driven risk, visible early symptoms, and a real countermeasure (quinine, boiled water, shade/night travel, avoiding tsetse country). Effects are legible: reduced speed, narrowed vision radius, locked dialogue options, party morale — never randomized controls. Untreated escalation can kill; the game always tells you why.

### 4.4 Trade & economy
Per-settlement market with a small goods list (trade goods, supplies, gifts, expedition gear). Price = base × seasonal factor × local supply/demand (event-director driven) × reputation discount/penalty. No global market sim — each settlement is a small independent state struct updated on its own tick.

### 4.5 Reputation & the clue graph
- Reputation is per settlement and per faction, moved by gifts, quests, conduct (e.g., hiring locally, honoring agreements), and rumor propagation between allied settlements.
- The mystery is a seeded DAG of clue nodes distributed across settlements, ruins, and NPCs. Node types: location hints, corroborations, keys (an item/phrase needed elsewhere), and red herrings (marked internally, discoverable as false through corroboration). Reputation thresholds and prerequisite clues gate node access.
- **Solvability is verified at generation time**: the generator proves at least two disjoint-enough paths from start to a winnable tomb identification before accepting a seed (§9).

### 4.6 NPCs
Two tiers. **Background NPCs**: schedules + scripted barks, zero LLM, cheap. **Named NPCs** (chiefs, guides, merchants, rivals, clue-holders): identity card (personality, history, faction, speech style), relationship memory, knowledge whitelist, and a dialogue tree authored in data. Only named NPCs are *eligible* for LLM phrasing, and only when the feature is on (§6).

### 4.7 Party & caravan
Hire porters, guides, animals. Composition affects capacity, terrain speed, safety, and local reception (a respected local guide raises reputation gains). Desertion/illness risks are telegraphed via morale and health indicators.

### 4.8 Rivals & the event director
2–3 seeded rival explorers pursue the same tomb using the same clue graph under simplified rules: they travel between settlements off-screen, can consume exclusive clue nodes, spread rumors, and can be encountered, allied with, misled, or raced. A lightweight **event director** ticks the off-screen world: market drift, weather systems, rival moves, political tension events — a weighted-random system reacting to state, not per-NPC agent simulation.

### 4.9 Diary
Deterministic auto-log of every significant event (structured data → templated prose). The diary is also the player's quest log and clue board: collected clues are pinnable to a map overlay for triangulation. Optional LLM mode adds reflective end-of-day entries generated *only from* the day's logged event data.

### 4.10 Caches & saves
Bury-and-retrieve caching kept as-is. Saves are plain JSON files, save-anywhere, with the world seed and event-director state included so a save is fully reproducible.

---

## 5. Art & Content Pipeline

- **Style bible first** (one page: palette, era mood — muted painterly, period-illustration feel) so AI-generated assets across sessions read as one game. (v1 uses `hoatool gen-art`: a seeded procedural tileset/markers/wind loop that satisfies the same content-ID pipeline; the bible's rules govern any later painted or AI art.)
- **Content-ID-addressed assets**: every tile/sprite/portrait resolved via an `assets.toml` manifest (e.g. `npc.portrait.emir_of_kano = "portraits/emir_of_kano.png"`). Placeholder art (tinted shapes + label) auto-generates for any missing ID, so the game *always runs* regardless of art coverage.
- **`hoatool` preprocessing**: normalize raw AI images into atlases (crop/pad, background removal, optional palette quantization), plus manifest and content validation.
- **Animation minimal**: 2–4 frame cycles. Early art budget goes to biome tiles and NPC portraits (highest narrative payoff).

---

## 6. Dialogue & LLM Architecture

### 6.1 Ground rules
1. **Scripted dialogue is the game.** Every named NPC has a complete authored dialogue tree in data (nodes, conditions on game state, effects expressed as the same action set the LLM would use). The game is 100% playable, winnable, and reviewable with `llm.enabled = false` — the default.
2. **The LLM, when enabled, re-phrases and extends conversation around those trees.** It is a narration skin. It owns no state, no facts, no outcomes.
3. **One provider path**: the OpenCode API, model `deepseek-v4-flash`, key from environment variable (`HOA_OPENCODE_API_KEY`) or config. No local inference, no second provider abstraction until a second provider actually exists.
4. **Fail closed, fail silent**: any error, timeout (hard cap ~6s), malformed output, or guardrail violation ⇒ show the scripted line for the current node. The player should never notice a degraded session beyond slightly plainer prose.

### 6.2 Config

```toml
# config.toml
[llm]
enabled = false            # master kill-switch; scripted dialogue when false
provider = "opencode"
model = "deepseek-v4-flash"
base_url = "https://opencode.ai/zen/go/v1"
timeout_ms = 15000
max_output_tokens = 1000
cache = true                # per-session response cache keyed by (npc_id, context_hash)
```

*Measured tuning (2026): `deepseek-v4-flash` is a reasoning model — its visible
JSON arrives in `choices[0].message.content` and needs ~1000-token headroom;
observed latency ~4.5s. The 6s/300-token starting values truncated replies.*

### 6.3 Request context (assembled fresh per turn, bounded)
- NPC identity card (≤ ~150 words), current relationship tier, mood flags.
- **Fact whitelist**: the only world facts the model receives are those this NPC is currently allowed to know *and* reveal, given reputation/prerequisites. Clue text is passed by ID with pre-authored phrasings; the model may select and wrap, never invent, clue content.
- Rolling conversation summary (last ~5 exchanges, summarized), not a growing transcript.
- The current dialogue node's scripted intent (e.g. "player asked about the western ruins; you may reveal clue `nile.c14` or deflect").
- System instructions: stay in period, stay in character, no anachronisms, no game-mechanics talk, respectful treatment per §3.2, output contract below.

### 6.4 Response contract & validation
The model must return strict JSON:

```json
{
  "say": "in-character reply text",
  "actions": [ { "type": "reveal_clue", "clue_id": "nile.c14" } ]
}
```

Allowed action types (closed set, each with a schema): `reveal_clue`, `offer_quest(template_id)`, `adjust_reputation(delta ∈ [-2,+2])`, `propose_trade(offer_id)`, `end_conversation`. The validator checks every action against **current game state and the node's permitted-action set**; any action not explicitly permitted by the active dialogue node is dropped. `say` text is length-capped and scanned for leaked restricted clue phrasings (string match against non-whitelisted clue texts); a hit discards the whole response in favor of the scripted line. Parse failure ⇒ scripted line. The sim applies surviving actions exactly as if a scripted node had emitted them — one shared code path.

### 6.5 Engineering shape
No async runtime. The dialogue UI dispatches a request to a **background `std::thread`** owning a blocking HTTP client (`ureq`); the reply comes back over an `mpsc` channel and is polled by the UI each frame ("…considering" indicator meanwhile; walking away cancels). Identical `(npc_id, context_hash)` requests are served from the session cache for consistency. Total LLM code target: one module, under ~600 lines including the validator.

---

## 7. Data Formats (all mainstream, all diffable)

| Data | Format | Why |
|---|---|---|
| Regions, factions, settlements, routes | TOML (`world.toml`, per-region files) | Comments, human-editable, massive LLM training coverage |
| Biome/terrain macro-map | Indexed-color PNG + color legend in TOML | Editable in any paint tool; ~20-line parser |
| Items, goods, price bases | TOML | — |
| NPC identity cards & dialogue trees | TOML (one file per named NPC) | Reviewable in a PR like prose |
| Quest templates, clue node templates | TOML | — |
| Asset manifest | TOML | — |
| Saves | JSON via `serde_json` | Debuggable in any editor, ubiquitous |
| Config | TOML | — |

RON is dropped: fine for humans, but comparatively rare in training data and tooling next to TOML/JSON — the boring, correct choice for an agent-maintained repo.

---

## 8. Technical Architecture — built for an LLM coding agent

### 8.1 Stack constraints (the actual requirements)
- **One language, one process, one binary.** No client/server split, no web frontend, no embedded scripting language, no codegen, no build.rs magic. Every behavior is greppable Rust or a data file.
- **Mainstream crates only**, each solving exactly one problem, each with a stable API and heavy training-data presence. Exotic or fast-churning dependencies are how agent-written code rots.
- **Explicit over clever**: plain structs and enums, `match` over trait-object dispatch where reasonable, no macro-heavy frameworks, small files (< ~400 lines), doc comments on every public item stating invariants.
- **Rust + Macroquad specifically because**: Rust's compiler catches the error classes agents most commonly introduce (state aliasing, forgotten variants on a new enum case); Macroquad is small, stable, immediate-mode, ECS-free, and well represented in examples — unlike engine frameworks with fast-moving APIs.

### 8.2 Repository layout — one crate, two binaries

```
heart-of-africa/
  Cargo.toml            # single crate
  src/
    main.rs             # game binary: init, main loop, mode switching
    bin/hoatool.rs      # dev tool binary: validate content, build atlases, gen seeds
    sim/                # ALL game logic. MUST NOT import macroquad. Headless-testable.
      mod.rs, clock.rs, player.rs, party.rs, survival.rs, travel.rs,
      economy.rs, reputation.rs, clues.rs, rivals.rs, events.rs, weather.rs
    world/              # map data, biome grid, pathfinding (hand-rolled A*, ~120 lines)
    content/            # TOML loaders + validation for all data types
    dialogue/           # tree runner, action set + validator, llm.rs (thread + HTTP + cache)
    render/             # macroquad drawing: camera, tiles, sprites, HUD
    ui/                 # egui screens: diary, inventory, trade, dialogue, map overlay
    save/               # serde_json snapshot/restore
  content/              # all TOML + PNG data files
  assets/               # art, audio (manifest-addressed)
  tests/                # integration: determinism, solvability, content lint
```

### 8.3 The one load-bearing rule
**`sim/` and `world/` never import macroquad or egui.** The simulation advances on a fixed tick (**1 tick = 1 in-game hour**); rendering runs per frame and interpolates. Consequences: the entire game logic runs headless in `cargo test`, a full seeded playthrough can be simulated in milliseconds by a test bot, and no gameplay bug can ever be a frame-timing bug.

### 8.4 Dependencies (complete list)

| Purpose | Crate | Note |
|---|---|---|
| Engine | `macroquad` | render/input/audio, native + WASM |
| UI panels | `egui` + `egui-macroquad` | diary/inventory/trade/dialogue + debug tools |
| Serialization | `serde`, `serde_json`, `toml` | ubiquitous |
| RNG | `rand`, `rand_chacha` | one named seeded stream per subsystem (worldgen, weather, events, rivals) — subsystems can't perturb each other's sequences |
| Image decode | `image` | biome-map PNG, `hoatool` art generation |
| HTTP (optional feature `llm`) | `ureq` | blocking client on a background thread; **no tokio, no async** |
| Error handling | `anyhow` | bins and loaders; this repo has no custom error types |

Deliberately absent: ECS (`hecs` — plain `Vec<Npc>` + ids until pain is demonstrated), a Tiled dependency, RON, async runtimes, a `pathfinding` crate (hand-rolled A* is fully visible and a pattern agents implement reliably), audio middleware (only if layered ambience is ever actually wanted), `noise` (weather and tile texture use hand-rolled deterministic hashing), `thiserror` (no custom error types).

### 8.5 Platforms
Native desktop primary; WASM build kept green from day one (near-free with Macroquad, great for sharing prototypes). The `llm` cargo feature is desktop-only initially.

---

## 9. Determinism, Testing & Validation

- **Determinism contract**: world state at tick T is a pure function of `(seed, ordered player inputs up to T)`. LLM output influences state only through validated actions, which are recorded in the input log like any player-visible event — replays remain exact even with LLM on.
- **Seed-replay test**: recorded input logs replayed headless must produce identical state hashes. Run in CI; any divergence is a P0 bug.
- **Clue-graph solvability**: property test — for 1,000 random seeds, the generator's output must contain ≥ 2 valid completion paths within budgeted reputation/travel cost, and every red herring must be falsifiable by some reachable corroboration. Unsatisfiable seeds are rejected and regenerated at runtime too.
- **Content lint as tests**: every TOML file schema-checked; every referenced ID (items, clues, portraits, factions, quest templates) must resolve; every dialogue-node permitted action must be in the closed action set. `cargo test` fails on a typo in a content file — this is the guardrail that lets content scale without code review of every line.
- **Sim bot**: a headless greedy agent that plays seeds start-to-finish nightly; flags softlocks, starvation-by-design, and pacing outliers (win in < 2h or > 30h of sim time).
- **LLM validator unit tests**: adversarial fixtures — invalid JSON, disallowed actions, out-of-range deltas, smuggled clue text — must all fall back cleanly to scripted lines.

---

## 10. Roadmap (with acceptance criteria)

1. **Foundations.** Single-crate scaffold, fixed-tick sim clock, biome-PNG world load, player movement + terrain costs, placeholder-art fallback, JSON save/load, CI (fmt, clippy, test). *Done when: a seeded walk across the Nile region replays deterministically in a headless test.*
2. **Vertical slice — Lower Egypt.** Full expedition loop in one region: survival system, one settlement fully populated (scripted dialogue, market, reputation), one complete deterministic clue chain, diary, caches, win/lose states. *Done when: a tester can finish a mini-run start to tomb using only in-game information.*
3. **Living world.** Event director, weather/disease per season, market drift, 2 rival explorers, seeded clue-graph generator + solvability check, caravan fast-travel. *Done when: three seeds produce visibly different but always-winnable runs; sim bot completes 100 seeds without softlocks.*
4. **Region 2 — Sahara/Sahel corridor.** Content-only addition proving the "regions are data" claim. *Done when: region 2 lands with zero changes under `src/` (loader/lint fixes excepted).*
5. **LLM garnish (optional, feature-flagged).** `dialogue/llm.rs`, OpenCode/deepseek-v4-flash client, validator, cache, kill-switch. *Done when: all adversarial validator tests pass; a full run with `enabled=false` and one with the key unset mid-session are indistinguishable from scripted play; no state divergence in replay with LLM on.*
6. **Art, audio & polish.** Style-bible art (procedural via `hoatool gen-art`) swapped in by content ID, ambient audio, historical-accuracy review pass per region, UX polish. Post-v1: regions 3–5 as data.

---

## 11. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| LLM output breaks fiction or leaks spoilers | Fact whitelist by ID, closed action set + node-level permission check, leak scan on `say`, fail-closed to scripted line, kill-switch default-off |
| OpenCode API down / key revoked mid-session | Fail silent to scripted lines; zero gameplay impact by construction |
| Agent-written code drifts into complexity | One crate, dependency allowlist in this doc, sim/render import rule, file-size and doc-comment conventions enforced in review, clippy in CI |
| Procedural clue graphs unsolvable or trivial | Generation-time solvability proof + sim-bot pacing checks |
| Historical inaccuracy / dated framing | Per-region research pass before content; named polities; factual non-glorifying treatment; accuracy review in roadmap step 6 |
| Scope creep | Two regions for v1; regions are data by construction; explicit non-goals below |
| Inconsistent AI art | Style bible + content-ID addressing; any asset regenerable independently |

**Explicit non-goals (v1):** multiplayer, mod support, 3D/isometric rendering, full NPC agent simulation, mobile, more than one LLM provider, voice, localization.

---

## 12. Decisions log (v1 open questions, resolved)

| Question | Decision | Rationale |
|---|---|---|
| Tick granularity | 1 tick = 1 in-game hour | Survival becomes planning (when to travel, when to rest) rather than micro-management; day/night and travel legs resolve naturally |
| Fast-travel encounters | Time+supply cost always; encounter roll only on routes flagged risky, and the flag is visible before departure | Preserves telegraphed-danger pillar; safe routes feel earned |
| Scope: five regions vs. fewer | Two regions v1, five-region data shape from day one | Vertical slice sooner; scale-out proven as content-only in roadmap step 4 |
| Local vs. cloud LLM | Cloud only (OpenCode, `deepseek-v4-flash`), optional, default-off | No second install story, no hardware assumptions; scripted dialogue is complete anyway |
| RON vs. TOML/JSON | TOML for content, JSON for saves | Mainstream formats an agent (and every diff tool) knows cold |
| ECS from the start | No — plain structs; revisit only on demonstrated pain | Fewer abstractions for the agent to misuse |
| Workspace of 10 crates | One crate, module folders, second bin for tools | Removes cross-crate ceremony; the sim/render import rule does the real architectural work |

---

*Note on tuning: the LLM timeout and output cap in §6.2 are measured values (see the note there).*