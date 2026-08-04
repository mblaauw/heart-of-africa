# AGENTS.md — Operating Instructions for AI Agents

This file tells AI coding agents — and the humans directing them — how to work in this
repository. It is written in the style of Andrej Karpathy's LLM prompting method:
give complete context up front, state a plan before acting, show your work, and verify
results by running them. Follow it literally; it exists because short instructions
produce sloppy work.

## 0. The one-sentence version

You are a very smart, very eager junior engineer who has never seen this project.
This file is your onboarding; `docs/DESIGN.md` is the authoritative design document;
`cargo test` is the referee. When in doubt the order is always: **read → state a plan →
execute → verify**.

## 1. Know the project first (mandatory)

- Read `docs/DESIGN.md` before touching code. It supersedes any assumption you hold
  about games, Rust projects, or this codebase. The repo currently contains no `src/`
  yet — the design doc is the spec, and Phase 1 ("Foundations") is the next step.
- This is a remake of *Heart of Africa* (1985): an open-world 1890s Africa expedition
  mystery. Rust + Macroquad, **one crate, two binaries**, all content in TOML/PNG data files.
- Pillar 7 of the design: "The codebase is the second product." It must be small, boring,
  explicit, and mainstream enough that another LLM agent can extend and debug it.
- The whole design was written to be *agent-implementable by design* (§8). When you feel
  tempted to do something clever, the design doc likely already made the boring choice —
  follow it.

## 2. How to work (the method)

### 2.1 State your plan before you act
Before writing any code, state: what you will do, in what order, which files you will
touch, and how you will verify. Then do exactly that. Do not start editing mid-thought.

### 2.2 Show your work
Reason step by step before your conclusions. If you change multiple things, show the
chain: problem → cause → change → test result. A bare "I fixed it" is not a report.

### 2.3 Put all relevant context in your message
Never make the reader guess. When reporting a problem include: the exact command, the
exact error text, file and line, and the relevant snippet. When asking a question,
include everything needed to answer it without opening files.

### 2.4 Divide and conquer
Break large tasks into small, independent units; complete and verify each one before
starting the next. Do not attempt "implement the whole game" in one pass — do one
system (sim clock, then world load, then movement), test it, then the next.

### 2.5 Verify, never assume
- Every claim about code is backed by a run: `cargo build`, `cargo test`,
  `cargo clippy --all-targets`, `cargo fmt --check`.
- "I think it works" is not a result. "`cargo test` passes, output attached" is.
- After any change, run the full suite — not just the tests you touched.

### 2.6 Iterate in place, don't restart
When a result is wrong, fix the specific step and retry: keep the conversation, the
context, and the partial results. Never abandon work and start over because of one failure.

### 2.7 Fix the context, not the output
If a result is wrong, the fix is almost always in the instructions or the spec, not in
repeating the same request louder. Improve the specification, then re-run.

### 2.8 Be explicit; zero guessing
- No assumptions about unspecified behavior. If a requirement is ambiguous, ask before
  implementing — the design doc resolves most questions (§12 decisions log).
- State what you are *not* doing too. Explicit non-goals prevent drift
  (see DESIGN.md §11 non-goals: no multiplayer, no mods, no async runtime, no ECS…).

### 2.9 Examples over explanation
When a pattern already exists in the repo — a module shape, a TOML content file, a test —
mirror it exactly instead of inventing a parallel style. Copy the shape of the closest
existing thing.

## 3. Repo rules (non-negotiable, from DESIGN.md)

- **The import rule**: `src/sim/` and `src/world/` must NEVER import `macroquad` or `egui`.
  The entire game logic must run headless in `cargo test`. Violating this is a P0.
- **Determinism**: world state at tick T is a pure function of (seed, player inputs).
  No wall-clock time, no unseeded randomness, no HashMap iteration order in gameplay
  logic. Replays must be byte-identical — there is a seed-replay test for this.
- **One crate, module folders**: no new crates. No new dependencies without updating
  the dependency list in DESIGN.md §8.4 (mainstream crates only: serde, toml, rand_chacha,
  ureq, image, noise…).
- **Explicit over clever**: plain structs and enums, `match` over trait-object dispatch,
  files under ~400 lines, doc comments on public items stating their invariants.
- **Content is data**: TOML files are validated by `cargo test`. Every referenced ID
  must resolve; a typo in a content file fails CI — that is the intended guardrail.
- **No async**: the ceiling of concurrency is a blocking HTTP client (`ureq`) on a
  background `std::thread` in `dialogue/llm.rs`. No tokio, no async runtimes.
- **Agent-implementable**: prefer the most mainstream, well-known Rust idioms. Exotic
  crates or clever patterns are how this repo rots.
- **The LLM garnish stays garnish**: `llm.enabled = false` is the default. Scripted
  dialogue is the game; the LLM is a narration skin that owns no state, no facts, no
  outcomes (§6).

## 4. Standard session workflow

1. Read `docs/DESIGN.md` and every file you're about to modify.
2. State a plan, including verification steps.
3. Implement the smallest correct unit.
4. Run: `cargo fmt` → `cargo clippy --all-targets` → `cargo test`.
5. Report: files changed, commands run with their output, deviations from the plan (with reasons).

## 5. How humans should prompt agents in this repo

The method works best when the prompt is a complete brief, not a one-liner:

- **Include context**: point at the relevant DESIGN.md section and files
  (e.g. "implement Phase 2 per DESIGN.md §10.2; the survival module goes in `src/sim/survival.rs`").
- **State the acceptance criterion**: "Done when the sim bot can finish a mini-run" beats
  "make survival work".
- **Ask for a plan first**: "State your plan before editing" is a valid first instruction.
- **Ask for verification**: require test output in the report; unverified claims are worse
  than "not done".
- **Fix the brief, not the agent**: if the result is wrong, tighten the instructions and
  re-run — the context was wrong, not the effort.
