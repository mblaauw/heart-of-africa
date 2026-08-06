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

- `tests/common/bot.rs` is the greedy sim bot (night travel through savanna, emergency water runs); the acceptance suite is `tests/living_world.rs` (100 seeds winnable, seed distinctness, herring falsification, rival solvability under schedule).
- `herrings_are_falsifiable` intentionally searches seeds because rivals can consume clues mid-test — keep the retry loop; a fixed seed goes flaky when content changes.
- LLM validator tests are headless (no network). The live check is `hoatool llm-demo` with `--features llm` and `llm.enabled = true` in config.toml.
- Replay determinism is asserted via `Save::state_hash()` (FNV-1a over the save JSON): apply a recorded input log to a fresh game and compare hashes.

## Working rules

- State a plan before editing; every claim about code must be backed by a run of the suite above ("cargo test passes" is a result; "I think it works" is not).
- `docs/DESIGN.md` §8.4 is the dependency allowlist — no new crates without updating it there first.
