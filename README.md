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
  red herrings) generated per playthrough with a generation-time solvability
  proof; rivals race you for exclusive clues.
- **Living world** — survival (hydration, nutrition, rest, four period
  ailments), per-settlement markets with weather-driven prices, caravan
  fast-travel on scouted routes, daily weather and market drift.
- **Two regions as data** — Lower Egypt (Nile delta to Aswan) and the
  Sahara/Sahel corridor (Kharga, Dakhla, Kufra, Lake Chad, Kano). Adding a
  region is content work, not code work.
- **Scripted dialogue is the game** — named NPCs with authored trees; an
  optional, default-off LLM garnish (`--features llm`) re-phrases lines
  through a strict validator — it owns no state, no facts, no outcomes.
- **Procedural art & audio** — a style-bible tileset, markers, and an ambient
  wind loop generated deterministically by `hoatool gen-art`.

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
cargo test                 # 78 tests, including the 100-seed sim-bot run
cargo run --bin hoatool -- validate content assets
```

The repo is written to be *agent-implementable by design*: plain structs,
mainstream crates, one crate with module folders. See [`AGENTS.md`](AGENTS.md)
for how to work in it.

## License

MIT OR Apache-2.0 (as declared in `Cargo.toml`). Not affiliated with
Electronic Arts; the original *Heart of Africa* is their property.
