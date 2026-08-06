# Heart of Africa

An unofficial, inspired-by remake of *Heart of Africa* (1985, Ozark Softscape /
Electronic Arts, design by Danielle Bunten Berry): an open-world 1890s Africa
expedition mystery. You land in Cairo with a dead explorer's diary, a modest
purse, and one goal — find the Lost Tomb of Pharaoh Ahnk-Ahnk before your
money, your health, or your rivals run out. The tomb and the entire web of
clues pointing to it regenerate every playthrough.

Written in Rust + Macroquad. **The full design document is
[`docs/DESIGN.md`](docs/DESIGN.md)** — including the ground rule that made the
whole thing possible: *simulation code never imports the render/UI libraries,
so the entire game logic runs headless in `cargo test`*.

## What's in it

- **Deterministic core** — world state is a pure function of (seed, inputs);
  runs replay byte-identically from their input logs (tested).
- **Seeded mystery** — a diamond clue graph (two disjoint paths, falsifiable
  red herrings, key-item gates) generated per playthrough with a
  generation-time solvability proof; the clue texts name the next stop, so
  the mystery is navigable from the diary alone (proven by a player-view
  sim bot that uses no hidden data).
- **Living world** — survival (hydration, nutrition, rest, four period
  ailments with real countermeasures), per-settlement markets with
  weather-driven prices, caravan fast-travel that runs the same hourly
  survival rules as walking, daily weather and market drift, and two rival
  explorers who circuit the region off-screen and may consume exclusive
  clues ahead of you (encounters, alliances, and a rival victory are future
  work).
- **Geography** — the world spans Lower Egypt (Nile delta to Aswan) and the
  Sahara/Sahel corridor (Kharga, Dakhla, Kufra, Lake Chad, Kano) in one
  continuous map. The data model is currently a single region (multi-region
  files are future work), but everything gameplay-relevant is data: adding
  settlements, routes, markets, and NPCs needs no source changes.
- **Scripted dialogue is the game** — named NPCs with authored trees; an
  optional, default-off LLM garnish (`--features llm`) re-phrases lines
  through a strict validator — it owns no state, no facts, no outcomes.
- **Procedural art & audio** — a style-bible tileset, markers, and an ambient
  wind loop generated deterministically by `hoatool gen-art`.

**Status:** a complete, winnable vertical slice with deterministic replay,
content linting, and two headless 100-seed acceptance suites (structural and
player-view). Not yet implemented: party/porter mechanics, faction
reputation, rival encounters, per-region weather, NPC schedules, and the
clue-board diary beyond the current clue list — see `docs/DESIGN.md` for the
full roadmap.

## Play

```sh
cargo run --release
```

Arrows walk · R rest · S save · L load · after a run ends, N starts a new
expedition (fresh seed).

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                 # ~90 tests, incl. two 100-seed sim-bot suites (~1 min)
cargo run --bin hoatool -- validate content assets
cargo run --bin hoatool -- map validate content   # geography.toml vs biome grid
```

The repo is written to be *agent-implementable by design*: plain structs,
mainstream crates, one crate with module folders. See [`AGENTS.md`](AGENTS.md)
for how to work in it.

## License

MIT (see [LICENSE](LICENSE)). Not affiliated with
Electronic Arts; the original *Heart of Africa* is their property.
