# AGENTS.md

Compact operating notes for AI agents in this repo. **Read `docs/DESIGN.md`
first** — it is the authoritative spec (every phase's acceptance criteria,
§10). This file only covers what the spec doesn't make obvious; the repo
already implements all six roadmap phases.

## Commands

- Verification order (mirrors CI): `cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test`.
- **Test both feature configs**: `cargo test` *and* `cargo test --features llm`. The `llm` feature compiles the HTTP client; changes can build in one config and break the other. CI only runs the default config.
- Content lint CLI: `cargo run --bin hoatool -- validate content assets`.
- Tiled geography workflow (`src/bin/hoatool/map_{gen,import,validate,preview}.rs`, WS1): `hoatool map gen <world.toml> <out-dir>` writes a `geography.tsx` template; `hoatool map import <world.toml> <x.tmj> <out-dir>` parses a Tiled JSON export and writes `geography.toml` + `manifest.toml`; `hoatool map validate <content-dir>` checks geometry against the biome grid; `hoatool map preview <content-dir> [out.png]` renders a diagnostic PNG. `content/geography.toml` (Tiled import output) overrides the legacy world.toml geography when present. LOD pyramid is still `hoatool gen-map`'s job; import deliberately does not generate it. The importer lives in the library (`src/content/import.rs`) and is covered by `tests/map_pipeline.rs` against the committed fixture `tests/fixtures/geography.tmj` — coordinates in the fixture are tile-midpoints so projection rounding can never flip a floor.
- **Generated files — never hand-edit; regenerate and commit both sides:**
  - Biome map: edit `content/maps/world.txt` (the diffable ASCII source), then `hoatool gen-map content/world.toml content/maps/world.txt content/maps/world.png`. The PNG is committed derived data.
  - Procedural art/audio: `hoatool gen-art content/world.toml assets` — seeded and deterministic (`tests/art.rs` asserts byte-identical output; the atlas rows must match the loader's biome order).
- Slow suite to know about: `tests/living_world.rs` plays 100 seeds through the sim bot (~30s).

## Architecture (not obvious from filenames)

- **P0 import rule**: `src/sim/` and `src/world/` must never import `macroquad`/`egui`. The whole game logic runs headless in `cargo test`; a gameplay bug can never be a frame-timing bug.
- **Explore map view (WS1)**: `src/render/map/` splits the renderer into `camera.rs` (follow + explore cameras; input mapping via the pure `ViewInput`/`apply_view_input` — no macroquad types inside), `tile_cache.rs`, `layers.rs`, `labels.rs`, `overlays.rs`. In-game controls: Tab toggles the explore map, wheel/± zoom, left-drag pans, F = player, Home = known world, Esc = gameplay. Camera state is presentation-only — `tests/map_view.rs` asserts camera inputs never change the save hash or replay log; keep it that way (never record camera actions as `Input`s). The `TileCache` LRU in `tile_cache.rs` is tested but un-consumed until the WS2 LOD pyramid lands — don't delete it as dead code.
- **WS2 M1 — SpatialHint & discovery (done)**: the location clue is a *hint* (origin settlement, octant relation, distance band, confidence) computed from real geometry at generation time — it never names the exact tile. The tomb is *discovered* by standing on the tile with the location clue in hand (`after_movement` → `tomb_revealed`). `tomb_hint()` is a pure function of the seed, never serialized. The oracle sim bot sweeps the hint sector via `build_sector` (concentric-ring arc ordering so consecutive scan tiles are spatially adjacent); endgame supply thresholds are (30,30) with cap (12,8) — one fully-funded pass, no mid-sweep market treks. The player-view bot parses the diary clue text into the same semantics and sweeps with its own memory. Rivals spread rumors but **do not consume exclusives before day 9** (`CONSUME_GRACE_DAY` in `src/sim/rivals.rs`; `process_day` gates consumption — legs are always raceable, DESIGN.md §4.8). Tests: `hint_sector_contains_tomb_for_all_seeds`, `hint_is_a_deterministic_function_of_seed`, `tomb_not_revealed_without_location_clue`, `tomb_revealed_on_arrival_with_location_clue`, plus the 100-seed oracle + player-view bot suites green in both feature configs.
- **WS2 next — Regional knowledge**: scaffolding `content/regions/*.toml` (`RegionKnowledge`: direction/distance terms, gifts, terrain lore, landmark terms, etiquette per region). Landmark origins replace the M1 settlement anchors in `SpatialHint`. Progressive glossary (`known_terms` in Save; diary re-interprets old entries; every required term must have a reachable source — a 1000-seed property test eventual). Social preferences (Unknown/Rumored/Confirmed; unknown disliked gift = small penalty + taboo revealed, never permanent lockout). In-game Expedition Reference. The LOD pyramid (tile_cache.rs) is coded but dormant — don't delete, it un-blocks when a visual base render exists.
- `src/lib.rs` exists so `tests/` integration tests can import the crate. `src/main.rs` (game) and `src/bin/hoatool/main.rs` + `art.rs` (dev tool) are thin binaries over it.
- **Determinism is the contract**: no wall-clock, no unseeded RNG, no HashMap iteration order in gameplay. Weather, market drift, rivals, and tile variants are pure functions of `(seed, day)` / coordinates. Validated LLM actions are recorded in the input log (`Input::LlmAction`), so replays stay byte-identical.
- Dialogue: the "Ask about the tomb" question is a runtime slot filled by the seeded clue graph — not authored dialogue. NPC data files need a `tomb_answer` node and a gift option in every node; never write clue-specific dialogue options.
- Content is data, validated by tests: a typo in `content/*.toml` fails `cargo test` (intended guardrail). The item universe is goods ∪ clue-granted items (e.g. `brass_key` is not tradable).
- The game loads `content/` and `assets/` relative to CWD — run from the repo root. Runtime saves go to `saves/` (gitignored).
- `config.toml` at the root holds `[llm]` (`enabled = false` default). The API key comes only from the `HOA_OPENCODE_API_KEY` env var (stored in `~/.secrets.zsh` on the dev machine) — never from files, never committed.

## Toolchain quirks

- `egui` is pinned to 0.31 to match `egui-macroquad` 0.17.3 — never bump one without the other (a fresh `cargo add egui` picks 0.35 and breaks the build).
- `rand` 0.10: range sampling lives on `rand::RngExt` (`random_range`, renamed from `gen_range`); `rand::Rng` is re-exported from `rand_core`.
- macroquad: `get_fps()` returns `i32`; `Texture2D::from_rgba8` takes `u16` dimensions.
- `ureq` 3: per-call timeouts go on an Agent (`Agent::config_builder().timeout_global(...)`); bodies are read with `response.into_body().read_to_string()`.
- The LLM model is a *reasoning* model: the strict-JSON reply lives in `choices[0].message.content` (see `llm::parse_response`), and 300 `max_tokens` truncates it — ~1000 is the tuned headroom.

## Testing quirks

- `tests/common/bot.rs` is the greedy sim bot (night travel through savanna, emergency water runs); the acceptance suite is `tests/living_world.rs` (100 seeds winnable, seed distinctness, herring falsification, rival solvability under schedule, plus WS2 M1's four hint/sector/discovery tests).
- `herrings_are_falsifiable` intentionally searches seeds because rivals can consume clues mid-test — keep the retry loop; a fixed seed goes flaky when content changes.
- LLM validator tests are headless (no network). The live check is `hoatool llm-demo` with `--features llm` and `llm.enabled = true` in config.toml.
- Replay determinism is asserted via `Save::state_hash()` (FNV-1a over the save JSON): apply a recorded input log to a fresh game and compare hashes.

## Working rules

- State a plan before editing; every claim about code must be backed by a run of the suite above ("cargo test passes" is a result; "I think it works" is not).
- `docs/DESIGN.md` §8.4 is the dependency allowlist — no new crates without updating it there first.

# Agent Coding Guidelines

Follow these rules for every coding task unless the user explicitly overrides them.

## 1. Understand Before Editing

Before changing files:

* Read the relevant code, configuration, tests, and nearby documentation.
* Identify the smallest set of files likely required.
* Briefly state:

  * your understanding of the goal;
  * any important assumptions;
  * the intended implementation and verification steps.

Do not begin implementation based only on filenames or guesses about the repository.

### Handling ambiguity

Use reasonable inference for small, low-risk details when the repository already establishes a clear pattern.

Ask a clarifying question only when the uncertainty:

* materially changes behavior, architecture, security, data, or public APIs;
* has multiple significantly different implementation paths;
* could cause destructive or difficult-to-reverse changes;
* cannot be resolved by inspecting the repository.

Do not block progress over naming, formatting, or other easily reversible details. State the assumption and continue.

## 2. Prefer the Simplest Complete Solution

Implement the smallest solution that fully satisfies the request.

* Prefer direct code over unnecessary abstractions.
* Reuse existing functions, utilities, libraries, conventions, and patterns.
* Do not introduce speculative extensibility or future-proofing.
* Do not add configuration options unless they are required.
* Do not create wrappers that merely rename or forward existing behavior.
* Avoid new dependencies when the existing stack can solve the problem cleanly.
* Do not implement requirements that were not requested or clearly implied.

If the solution becomes disproportionately complex, stop and reconsider whether a simpler implementation exists.

## 3. Make Surgical Changes

Limit changes to what is required for the task.

* Do not refactor unrelated code.
* Do not reformat entire files.
* Do not rename unrelated symbols.
* Do not rewrite working code merely to match personal preferences.
* Do not fix unrelated warnings or failing tests unless they prevent completion.
* Do not modify generated files, lockfiles, migrations, snapshots, or vendored code unless the task requires it.
* Remove temporary code, debug output, unused imports, and files introduced by your changes.

When an unrelated issue is discovered, report it separately instead of silently expanding the scope.

## 4. Preserve Existing Behaviour

Unless the task explicitly requires a breaking change:

* preserve public APIs and command-line interfaces;
* preserve configuration compatibility;
* preserve existing defaults;
* preserve error semantics where practical;
* follow the repository’s established architecture and style.

Do not silently change behavior outside the requested scope.

## 5. Work Toward Explicit Success Criteria

Before implementation, translate the request into observable success criteria.

For bug fixes:

* identify the failing behavior;
* reproduce it when feasible;
* add or update a regression test where appropriate;
* verify that the test fails before the fix when practical.

For features:

* define the expected inputs, outputs, edge cases, and failure behavior;
* add focused tests for the new behavior;
* avoid broad snapshot tests when direct assertions are clearer.

Tests should validate behavior, not merely mirror implementation details.

## 6. Verify Before Finishing

Run the narrowest relevant checks first, followed by broader checks when practical.

Use the repository’s own documented commands and tooling. Typical checks include:

* targeted unit or integration tests;
* formatter or formatting check;
* linter;
* type checker such as `tsc`, `pyright`, or `mypy`;
* build or compile command;
* relevant end-to-end or smoke test.

Do not claim that a command passed unless it was actually run successfully.

If a check cannot be run:

* state which check was skipped;
* explain why;
* describe any remaining risk.

Do not hide existing failures. Clearly distinguish failures caused by your changes from failures already present in the repository.

## 7. Protect Data and Repository State

Do not perform destructive or irreversible operations without explicit permission.

This includes:

* deleting user data;
* resetting databases;
* force-pushing;
* rewriting Git history;
* deleting branches or tags;
* running destructive migrations;
* discarding uncommitted changes;
* using commands such as `git reset --hard`, `git clean -fd`, or equivalent destructive operations.

Never overwrite or remove changes that you did not create.

Do not expose, log, commit, or hard-code secrets, tokens, credentials, private keys, or sensitive environment values.

## 8. Keep Communication Focused

During the task:

* provide brief progress updates for multi-step work;
* report blockers as soon as they are known;
* avoid narrating routine tool usage;
* do not present internal chain-of-thought or speculative reasoning.

When finished, provide:

1. a concise summary of what changed;
2. the important files changed;
3. the verification commands run and their results;
4. any assumptions, limitations, or remaining risks.

Do not produce a long retrospective unless requested.

## 9. Definition of Done

A task is complete only when:

* the requested behavior is implemented;
* the changes are limited to the intended scope;
* relevant tests or checks have been run;
* temporary artifacts have been removed;
* the final response accurately describes what was and was not verified.
