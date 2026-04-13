---
name: debug-worldsim
description: >
  Investigate agent behavior in the worldsim simulation. Use the headless CLI, JSONL event log, decision trace, ad-hoc inspection, and MindGraph queries to figure out why agents are doing what they're doing.
  TRIGGER when: investigating a bug report, asking "why did agent X do Y?", reproducing a flaky issue, validating a feature outside of a test, or doing ad-hoc exploration of the simulation.
  DO NOT TRIGGER when: writing a new test from scratch (use the test-worldsim skill instead — though debugging WITHIN a test is fair game here).
---

# Debug Worldsim

Investigate agent behavior using the observability tooling. Everything described here works in headless mode — no GUI required.

For writing new tests, use the `test-worldsim` skill instead. The two skills overlap on inspection methods because debugging often crosses the boundary.

## Where the debugging tools live

Read these files for current API and flag definitions — don't trust documented copies that may be stale:

- `src/cli.rs` — every CLI flag, repeatable args, default values
- `src/headless.rs` — what the headless runner does between ticks
- `src/agent/events.rs` — `SimEvent` enum, the source of all observability
- `src/core/event_log.rs` — JSONL logger, filter parsing
- `src/agent/brains/trace.rs` — decision trace ring buffer and config
- `src/testing/world.rs` — `TestWorld` inspection methods (work in tests AND in the headless runner setup)

## The five observability channels

Pick the right one for what you're trying to learn:

### 0. Map matrix dump — for terrain generation

`cargo run --release -- --dump-map` prints the default-seed terrain as ASCII (one char per tile, y inverted so north is on top) and exits. Use it when debugging `generate_terrain` / `carve_river` instead of launching the game.

### 1. JSONL event log — for post-mortem analysis with jq

Captures every `SimEvent` to a file. Best for "I want to slice the data later."

```bash
# Capture everything for 5000 ticks
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.jsonl

# Stream to stdout for piping
cargo run --release -- --headless --ticks 5000 --seed 42 --log -

# Filter at capture time to keep file size down
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.jsonl \
  --log-filter agent:alice \
  --log-filter type:Decision,ActionStarted \
  --log-filter tick:1000-2000
```

Every event in the log carries both `agent` (display name, e.g. `"alice"`) and `agent_id` (stable Entity debug string, e.g. `"0v0"`). Filter by whichever you have — `agent_id` is safer when names repeat or the agent dies and gets despawned.

**jq patterns:**

```bash
# All decisions for one agent (by name)
cat events.jsonl | jq 'select(.agent == "alice" and .type == "Decision")'

# Same, filtering by entity id instead (works after death / renames)
cat events.jsonl | jq 'select(.agent_id == "0v0" and .type == "Decision")'

# All deaths
cat events.jsonl | jq 'select(.type == "Death")'

# Count events by type
cat events.jsonl | jq -r .type | sort | uniq -c | sort -rn

# Timeline of an agent's actions
cat events.jsonl | jq -r 'select(.agent == "alice" and .type == "ActionStarted") | "\(.tick)\t\(.action)"'

# Find when relationships changed
cat events.jsonl | jq 'select(.type == "RelationshipChanged" and .agent == "alice")'
```

### 2. Decision trace — for focused per-agent decision history

A ring buffer of brain decisions and recent events for one agent. Best for "why does this specific agent keep making bad choices?"

```bash
# Trace one agent to stderr (text format). Selector accepts name OR entity id
# — same rules as --inspect: `agent:alice` or `agent:0v0`.
cargo run --release -- --headless --ticks 5000 --seed 42 --trace agent:alice

# Trace ALL agents (verbose, only useful for small populations)
cargo run --release -- --headless --ticks 5000 --seed 42 --trace all

# Limit the trace to a tick range you care about
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --trace agent:alice --trace-ticks 4500-4600

# JSONL trace to file (for programmatic processing)
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --trace agent:alice --trace-format jsonl --trace-file alice_trace.jsonl
```

The trace shows brain proposals, the winning brain, urgencies, powers, and recent SimEvents — everything you need to reconstruct a decision.

### 3. Ad-hoc CLI inspection — pause at a tick and snapshot

When you already know roughly where the bug is and want to take a hard look at agent state at one moment.

All `--inspect` / `--dump-*` / `--why` / `--trace` flags accept an agent selector as either a display name (`agent:alice`) or a stable entity id (`agent:0v0`). `find_agent` tries id first, then falls back to name, so either works — prefer the id when you're scripting or when the same name might appear twice.

`--at-tick` can be repeated to inspect at multiple points in a single run. The sim pauses at each tick, runs all inspection commands, then continues. Use `2>/dev/null` to suppress brain progress traces that interleave with inspection output.

```bash
# Full state snapshot of an agent at a specific tick
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --at-tick 4521

# Multiple agents in one run
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --inspect agent:bob --at-tick 4521

# Inspect at multiple ticks in one run
cargo run --release -- --headless --ticks 90000 --seed 42 \
  --inspect agent:alice --why "alice metric:glucose" \
  --at-tick 500 --at-tick 5000 --at-tick 30000 --at-tick 60000 2>/dev/null

# Dump the full MindGraph (everything an agent believes/knows)
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-mind agent:alice --at-tick 4521

# Search the MindGraph for specific knowledge
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --query "alice Wolf" --at-tick 4521

# Body-channel occupancy (which running actions are holding which channels)
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-channels agent:alice --at-tick 4521

# What does the agent currently perceive (VisibleObjects with distance + kind)
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-perception agent:alice --at-tick 4521

# Why is a metric moving? Supported: glucose, stamina, hydration, stomach, mood.
# Prints every signed per-second contributor and the net rate.
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --why "alice metric:glucose" --at-tick 4521

# Everything at once — state, brain decision, channels, perception, mind graph
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-all agent:alice --at-tick 4521
```

`--inspect`, `--dump-mind`, `--query`, `--why`, `--dump-channels`, `--dump-perception`, `--dump-all` are all repeatable — combine them in one run to get a full picture.

### 4. TestWorld inspection methods — when debugging from inside a test

If you're debugging a failing test (not a headless run), call these BEFORE the failing assertion. Output goes to stderr — visible with `cargo nextest run -E 'test(name)' --no-capture` (nextest swallows output by default; `--no-capture` streams it live) or in CI logs.

- `world.print_agent_state(agent)` — full snapshot
- `world.print_brain_decision(agent)` — last decision with all proposals and powers
- `world.print_mind_graph(agent)` — full MindGraph dump
- `world.print_relationships(agent)` — all relationships with trust/affection/respect
- `world.print_conversation(agent)` — current conversation state
- `world.print_channels(agent)` — body-channel occupancy (which action holds which channel)
- `world.print_perception(agent)` — visible entities with kind and distance
- `world.print_why(agent, "glucose")` — signed contributor breakdown for one metric. Supported metrics: `glucose`, `stamina`, `hydration`, `stomach`, `mood`.
- `world.query_knowledge(agent, "Wolf") -> Vec<String>` — text search
- `world.print_recent_events(N)` — SimEvents from last N ticks
- `world.print_agent_events(agent, N)` — SimEvents for one agent in last N ticks

The full list of inspection methods is in `src/testing/world.rs` — search for `pub fn print_` and `pub fn query_`.

## Reproducing the exact game world headless

Use `--game-defaults` to run with the same 128×128 noise map and Realistic placement algorithm as `cargo run`. Individual counts can still be overridden.

```bash
# Exact game world for 5000 ticks
cargo run --release -- --headless --game-defaults --ticks 5000 --seed 42 --log events.jsonl

# Same world, 10 humans instead of the default 6
cargo run --release -- --headless --game-defaults --humans 10 --ticks 5000 --seed 42
```

In tests, `TestWorld::game_defaults(seed)` is the in-process equivalent — same positions and counts:

```rust
let world = TestWorld::game_defaults(42);
```

Without `--game-defaults`, headless uses a 64×64 flat map with uniform random scatter (fast, minimal setup).

## Debugging recipes

### "Why did agent X do Y?"

1. Reproduce headless with the same seed: `cargo run --release -- --headless --game-defaults --ticks N --seed S`
2. Trace that agent: add `--trace agent:X`
3. Look at the trace around the moment Y happened — find the winning brain, urgency, and proposals
4. If the trace is too noisy, narrow with `--trace-ticks START-END`
5. If you need to see what they knew at that moment, add `--dump-mind agent:X --at-tick N`

### "Why did agents end up in this weird state at the end?"

1. Run with full event log: `--log events.jsonl`
2. Use jq to find the unusual events: `cat events.jsonl | jq 'select(.type == "Death")'`
3. Walk backward from the symptom — what happened just before?
4. Drop into `--inspect` at the tick before the unusual event

### "This test is flaky"

Flaky tests are non-determinism, period. Don't add retries.

1. Find the seed difference — does the test use `with_seed(0)` consistently?
2. Find the timing dependency — is the test asserting after exactly N ticks when N is too tight?
3. Find the wall-clock dependency — `chrono`, `std::time`, anything that isn't `current_tick()`
4. Find the iteration order — are you iterating a `HashMap`? Use `BTreeMap` or sort first.
5. Reproduce locally with the same seed and trace the first agent that diverges

### "What knowledge does Alice have about Bob?"

```bash
cargo run --release -- --headless --ticks N --seed S --query "alice Bob"
```

### "When did Alice lose trust in Bob?"

```bash
cargo run --release -- --headless --ticks N --seed S --log events.jsonl
cat events.jsonl | jq 'select(.type == "RelationshipChanged" and .agent == "alice" and .other == "bob")'
```

### "How many decisions did each agent make?"

```bash
cat events.jsonl | jq -r 'select(.type == "Decision") | .agent' | sort | uniq -c | sort -rn
```

### "Why is Alice's glucose / stamina / hydration dropping?"

```bash
cargo run --release -- --headless --game-defaults --seed 42 --ticks 30000 \
  --why "alice metric:glucose" --at-tick 30000
```

Prints every signed per-second contributor (BMR, each running action's drain, digestion) and the net rate. The same breakdown is under "Details" on each bar in the in-game agent panel. Works for `glucose`, `stamina`, `hydration`, `stomach`, `mood`.

### "Why can't this agent start X right now?"

```bash
cargo run --release -- --headless --game-defaults --seed 42 --ticks 30000 \
  --dump-channels agent:alice --at-tick 30000
```

Shows every body channel with its current load and capacity plus which actions are holding it. An agent can't start a new action whose channel requirements exceed what's free.

## Notes

- Always use `--seed` for reproducibility. Different seeds = different worlds.
- Headless runs are fast — don't be afraid to re-run with different observability flags until you get the right view.
- If you need an event type that doesn't exist yet, add a variant to `SimEvent` in `src/agent/events.rs` and emit it from the relevant system. The logger, trace, and inspection tools all pick it up automatically.
- For writing new tests, see the `test-worldsim` skill.
