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

## The four observability channels

Pick the right one for what you're trying to learn:

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

**jq patterns:**

```bash
# All decisions for one agent
cat events.jsonl | jq 'select(.agent == "alice" and .type == "Decision")'

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
# Trace one agent to stderr (text format)
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

```bash
# Full state snapshot of an agent at a specific tick
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --at-tick 4521

# Multiple agents in one run
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --inspect agent:bob --at-tick 4521

# Dump the full MindGraph (everything an agent believes/knows)
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-mind agent:alice --at-tick 4521

# Search the MindGraph for specific knowledge
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --query "alice Wolf" --at-tick 4521
```

`--inspect`, `--dump-mind`, and `--query` are all repeatable — combine them in one run to get a full picture.

### 4. TestWorld inspection methods — when debugging from inside a test

If you're debugging a failing test (not a headless run), call these BEFORE the failing assertion. Output goes to stderr — visible with `cargo nextest run -E 'test(name)' --no-capture` (nextest swallows output by default; `--no-capture` streams it live) or in CI logs.

- `world.print_agent_state(agent)` — full snapshot
- `world.print_brain_decision(agent)` — last decision with all proposals and powers
- `world.print_mind_graph(agent)` — full MindGraph dump
- `world.print_relationships(agent)` — all relationships with trust/affection/respect
- `world.print_conversation(agent)` — current conversation state
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

## Notes

- Always use `--seed` for reproducibility. Different seeds = different worlds.
- Headless runs are fast — don't be afraid to re-run with different observability flags until you get the right view.
- If you need an event type that doesn't exist yet, add a variant to `SimEvent` in `src/agent/events.rs` and emit it from the relevant system. The logger, trace, and inspection tools all pick it up automatically.
- For writing new tests, see the `test-worldsim` skill.
