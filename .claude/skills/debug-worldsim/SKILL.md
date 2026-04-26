---
name: debug-worldsim
description: >
  Investigate agent behavior in the worldsim simulation. Use the headless CLI, Parquet/JSONL event log (queried with DuckDB via `worldsim --debug`), decision trace, ad-hoc inspection, and MindGraph queries to figure out why agents are doing what they're doing.
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
- `src/core/event_log.rs` — JSONL / Parquet writer, DuckDB setup-script generator, filter parsing
- `src/core/field_logger.rs` — per-tick agent state logger (field resolver, presets, emission rules)
- `src/agent/body/contributions.rs` — shared metabolism contributor breakdown (used by `--why` and `:why`)
- `src/agent/brains/trace.rs` — decision trace ring buffer and config
- `src/testing/world.rs` — `TestWorld` inspection methods (work in tests AND in the headless runner setup)
- `src/core/perf.rs` — per-system tick timer (`PerfTracker`, `PerfBucket` + `PerfSubBucket`), drives the F3 overlay and `--perf` output
- `src/ui/perf_overlay.rs` — F3 overlay renderer
- `docs/perf_overlay.md` — user-facing explanation of the overlay, bucket list, and the latency-vs-CPU-time caveat

## Debug output location

**All debug artefacts go under `debug/<run-name>/` inside the project root.** Keep every sim log, field-logger file, trace, and generated SQL script under that root so the filesystem stays tidy and `rm -rf debug` always cleans up everything. The `debug/` directory is gitignored. Don't scatter files at the project root.

```bash
mkdir -p debug/<run-name>   # always do this first
```

## Default workflow: Parquet + DuckDB

For anything past a few thousand ticks, the fast path is **Parquet → DuckDB**:

1. Run headless with `--log events.parquet`. The `.parquet` extension flips the logger from JSONL to columnar Parquet — 10-100x smaller than JSONL, and DuckDB reads it instantly.
2. `worldsim --debug <run-dir>` emits a DuckDB setup script that attaches every log in the directory (`events`, `trace`, `fields`, `mutations`) as a view, plus canned joins like `decisions_with_plans`.
3. Query with DuckDB — joins across event streams, aggregations, window functions, all without a second sim run.

```bash
# Capture a full run into a directory
mkdir -p debug/run42
cargo run --release -- --headless --game-defaults --ticks 5000 --seed 42 \
  --log debug/run42/events.parquet

# Generate the DuckDB setup script and launch a REPL with views attached
cargo run --release -- --debug debug/run42 > debug/run42/setup.sql
duckdb -init debug/run42/setup.sql

# Or one-shot it
duckdb -c "$(cargo run --release -q -- --debug debug/run42) SELECT type, COUNT(*) FROM events GROUP BY type ORDER BY 2 DESC;"
```

Prefer JSONL (`--log events.jsonl` or `--log -`) only when the run is small, you want to grep/jq directly, or you need to stream to stdout.

Every event schema is stable across JSONL and Parquet — the payload column in Parquet holds the raw JSONL line, plus `tick`, `event_type`, and `agent` are extracted into dedicated columns for fast filtering.

## The observability channels

Pick the right one for what you're trying to learn:

### 0. Map matrix dump — for terrain generation

`cargo run --release -- --dump-map` prints the default-seed terrain as ASCII (one char per tile, y inverted so north is on top) and exits. Use it when debugging `generate_terrain` / `carve_river` instead of launching the game.

### 1. Event log (Parquet or JSONL) — for post-mortem analysis

Captures every `SimEvent`. Format is picked by extension:

- `--log events.parquet` — columnar, compact, DuckDB reads it directly (preferred)
- `--log events.jsonl` — one JSON object per line, grep/jq-friendly
- `--log -` — stream JSONL to stdout for piping

```bash
# Capture everything for 5000 ticks (Parquet)
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.parquet

# Filter at capture time to keep file size down (both formats support this)
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.parquet \
  --log-filter agent:alice \
  --log-filter type:Decision,ActionStarted \
  --log-filter tick:1000-2000
```

Every Entity field serializes as a nested object `{"name": "alice", "id": "0v0"}`. In queries, pull the name with `$.agent.name` and the stable id with `$.agent.id`. Same shape for other Entity fields (`$.target.name`, `$.other.id`, etc.). Filter by whichever you have — `.id` is safer when names repeat or the agent dies and gets despawned.

**DuckDB patterns** (against a Parquet log, or JSONL via `read_json_auto`):

```sql
-- All decisions for one agent
SELECT * FROM events WHERE json_extract_string(payload, '$.agent.name') = 'alice' AND type = 'Decision' ORDER BY tick;

-- Event type distribution
SELECT type, COUNT(*) AS n FROM events GROUP BY type ORDER BY n DESC;

-- Timeline of an agent's actions
SELECT tick, json_extract_string(payload, '$.action') AS action
FROM events WHERE type = 'ActionStarted' AND json_extract_string(payload, '$.agent.name') = 'alice' ORDER BY tick;

-- Join decisions with the plan that drove them (canned view)
SELECT * FROM decisions_with_plans WHERE json_extract_string(payload, '$.agent.name') = 'alice' AND tick BETWEEN 4500 AND 4600;

-- Why did alice take action X at tick Z? One query.
SELECT d.tick, d.winner, d.actions, p.plan_id, p.goal, p.driving_urgency,
       array_agg(u.source || '=' || u.value) AS urgencies
FROM events d
LEFT JOIN events p ON p.type = 'PlanGenerated' AND json_extract_string(p.payload, '$.agent.name') = json_extract_string(d.payload, '$.agent.name') AND p.tick <= d.tick
LEFT JOIN (SELECT tick, agent, unnest(urgencies) AS u_rec FROM events WHERE type = 'Decision') u
  ON u.tick = d.tick AND json_extract_string(u.payload, '$.agent.name') = json_extract_string(d.payload, '$.agent.name')
WHERE d.type = 'Decision' AND json_extract_string(d.payload, '$.agent.name') = 'alice' AND d.tick = 4521
GROUP BY d.tick, d.winner, d.actions, p.plan_id, p.goal, p.driving_urgency;
```

**jq patterns** (JSONL only):

```bash
# All decisions for one agent
cat events.jsonl | jq 'select(.agent.name == "alice" and .type == "Decision")'

# Count events by type
cat events.jsonl | jq -r .type | sort | uniq -c | sort -rn

# Timeline of an agent's actions
cat events.jsonl | jq -r 'select(.agent.name == "alice" and .type == "ActionStarted") | "\(.tick)\t\(.action)"'
```

**Key event types** (see `src/agent/events.rs` for the canonical list):

- `Decision` — per-tick arbitration: winner, brain powers, proposals, and the full `urgencies` contributor list
- `ActionStarted` / `ActionCompleted` / `ActionFailed` / `ActionPreempted` — action lifecycle. `ActionStarted` carries `plan_id` + `plan_step` when the action came from an executing rational-brain plan
- `PlanAbandoned` — a running plan was dropped
- `PlanGenerated` — GOAP produced a new plan (plan_id, goal, step_count, driving_urgency, subjective_cost)
- `GoapSearchTelemetry` — per-search iteration count, exhausted flag, best_unmet_goals
- `TargetEnumerated` — every (action, target) pair the brain considered during planning, with inclusion_reason
- `PatternRejected` — a goal the planner could not satisfy (unmet_patterns — catches the "stone is not food" class of bugs)
- `MindGraphMutation` — every triple added/removed (op, subject, predicate, object). Replay from tick 0 to reconstruct MindGraph state at any tick.
- `AgentStateHash` — per-tick hash of (position_tile, urgencies, plan_ids). Diff between two runs to find the exact tick of non-determinism divergence.
- `Death`, `RelationshipChanged`, `EmotionTriggered`, `CombatHit`, `KnowledgeShared`, `PhenotypeDeveloped`, and more — see `SimEvent`.

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

### 4. Per-tick field logger — for continuous numeric state over time

Captures one JSONL line per tick per agent, where each line carries a selected set of fields resolved from the component tree. Fills the gap between snapshot inspection (`--inspect`) and event streams (`--log events.jsonl`) — use it when you need "show me Alice's glucose curve for a full game day", not "what was Alice doing at tick 4521".

```bash
# Full vitals for Alice, every tick, to a file
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-preset vitals --log-file debug/alice/fields.jsonl

# Add actions and channels
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-preset vitals \
  --log-field actions --log-field channels --log-file debug/alice/fields.jsonl

# Only when something changed (much smaller output)
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-preset vitals \
  --log-on-change actions --log-on-change needs.glucose \
  --log-file debug/alice/fields.jsonl

# Heartbeat every 5 sim-seconds PLUS all action transitions in between
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-preset vitals \
  --log-every 300 --log-on-change actions \
  --log-file debug/alice/fields.jsonl

# Whole species, exported as CSV for a spreadsheet
cargo run --release -- --headless --game-defaults --seed 42 --ticks 10000 \
  --log-agent species:Deer --log-preset vitals \
  --log-as csv --log-file debug/deer/fields.csv

# Inline the contributor breakdown for a metabolism metric using the `:why` modifier
cargo run --release -- --headless --game-defaults --seed 42 --ticks 30000 \
  --log-agent alice --log-field needs.glucose:why --log-file debug/alice-glucose/fields.jsonl

# Delta-since-last-emission with the `:delta` modifier
cargo run --release -- --headless --game-defaults --seed 42 --ticks 30000 \
  --log-agent alice --log-field needs.glucose:delta

# Dry run: print the expanded field list without running the sim
cargo run --release -- --log-list-fields --log-preset full
```

**Selectors** (`--log-agent`, repeatable): `all`, `species:Human|Deer|Wolf|Rabbit|Bird`, `name:<substring>` (case-insensitive), or a literal agent name / Bevy entity id (`alice`, `19v0`).

**Fields**: dotted paths into the component tree. Top-level namespaces:

- `needs.*` — aerobic, anaerobic, glucose, stomach, reserves, hunger, hydration, wakefulness, health
- `consciousness.alertness`
- `actions`, `actions.primary` — active actions with brain attribution and reason
- `channels`, `channels.<name>` — body channels with load/cap/holders (locomotion, manipulation, consumption, vocalization, bite, carry, fullbody, focus, awareness)
- `brain.winner`, `brain.powers[.survival|.emotional|.rational]`, `brain.proposals[.count]`
- `cns.urgencies`, `cns.urgencies.<source>` (hunger, thirst, stamina, social, fun, fear, pain, curiosity, territoriality, sleepiness), `cns.sleep_wake_trigger`
- `plans`, `plans.executing`, `plans.count`
- `mind.size`, `mind.knows:<Concept>` (e.g. `mind.knows:Apple`, `mind.knows:Wolf`)
- `position`
- `emotions.mood`, `emotions.active`

Use `--log-list-fields --log-preset full` to dump the canonical path list.

**Presets** (`--log-preset`, repeatable):

- `vitals` — aerobic + glucose + stomach + reserves + hunger + wakefulness + health
- `actions` — actions + channels + brain.winner
- `brain` — brain.winner + brain.powers + cns.urgencies + plans.executing
- `full` — vitals + actions + brain

**Wildcards**: `needs.*`, `cns.urgencies.*` expand to every child under that prefix.

**Modifiers**:

- `<path>:delta` — emit a `<path>_delta` sibling with the change since the last emitted line
- `<path>:why` — emit a `<path>_why` sibling with the contributor breakdown (only valid for `needs.glucose`, `needs.aerobic`, `needs.hydration`, `needs.stomach` — same logic as `--why`)

**Emission rules** (OR rule between heartbeat and change-detection):

| `--log-every N` | `--log-on-change` | Behavior |
|---|---|---|
| no | no | every tick (default) |
| yes | no | every Nth tick |
| no | yes | only when a watched field changed |
| yes | yes | heartbeat OR change |

**Change thresholds**: `0.05` for normalized `[0,1]` metrics (hunger, wakefulness, alertness, mood, brain powers, urgency values), `1.0` for raw stats (glucose, aerobic, reserves, stomach, hydration, health), structural equality for list/object fields. Override per-field: `--log-on-change needs.aerobic:2.0`.

**Debounce** (`--log-debounce N`): hold change-driven emissions for N ticks. If the state reverts to the last-emitted signature inside the window, the change is dropped entirely; if it mutates to yet another state, the timer restarts against the new state. Applies only to `--log-on-change` — heartbeats (`--log-every`) and the default every-tick mode bypass debounce. Use this to suppress sub-second flickers (tick-level preemptions, emergency wakes) when you only care about persistent transitions.

```bash
# Only emit action changes that stick for at least 60 ticks (~1 sim-second)
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-preset vitals \
  --log-on-change actions --log-debounce 60 \
  --log-file debug/alice/fields.jsonl
```

**Output**: `--log-file <path>` (default stderr, `-` for stdout). `--log-as csv` post-processes the buffered JSONL into a flat CSV with dotted-path columns for spreadsheet work.

**jq patterns:**

```bash
# Alice's glucose curve
cat debug/alice/fields.jsonl | jq -r '"\(.tick)\t\(.needs.glucose)"'

# Every tick where she changed action
cat debug/alice/fields.jsonl | jq 'select(.actions[0].type != .actions[1].type)'

# Find the first tick where hunger crossed 0.5
cat debug/alice/fields.jsonl | jq 'select(.needs.hunger > 0.5) | .tick' | head -1
```

### 5. Per-system tick timer (`--perf` / F3) — for "which system is slow?"

Headless `--perf` collects per-bucket wall-clock timings every tick and prints a sorted Minecraft-F3-style table every N ticks. Parent buckets (perception / memory / psyche / skills / biology / brain / communication / action) expand into sub-buckets where it matters: brain → urgency / planning / arbitration / history, perception → visual / sensory / social, memory → wm_tick / consolidation / mindgraph_drain, action → execution / world_mutation, psyche → emotions / relationships / social_drives / territoriality, communication → lifecycle / turn. `skills` and `biology` stay flat (2 systems each). See `docs/perf_overlay.md` for the full bucket → system mapping. For function-level timings inside a hot sub-bucket, see §6 (Tracy CLI).

```bash
# Collect timings for 5000 ticks, print every 1000, and dump the final snapshot into the report JSON
mkdir -p debug/perf
cargo run --release -- --headless --game-defaults --seed 42 --ticks 5000 \
  --perf --perf-every 1000 --report 2> debug/perf/perf.log > debug/perf/report.json

# The same table is live in the windowed app: press F3 in the running game.
```

**Reading the numbers honestly.** Bucket latencies include wall-clock time spent waiting on cross-bucket `.after()` constraints, not just their own CPU work. When you see `Σ bucket avg` > `tick avg` in the header, the scheduler is giving you parallelism for free — the ranking is still trustworthy even when the percentages don't add up to 100%. See `docs/perf_overlay.md` for the full caveat.

**When to reach for it:**
- "Sim feels slow, is it the planner or consolidation?" — parent buckets rank, sub-buckets split the expensive ones.
- "Did my refactor make the brain cheaper?" — capture `--report` before and after, diff `perf_stats`.
- **Not for** microbenchmark regression tracking (use criterion in `benches/`) or per-agent cost (tracker is sim-wide). For function-level detail inside a sub-bucket, see §6 (Tracy CLI).

### 6. Tracy CLI — for per-Bevy-system function-level timings

When `--perf` says a sub-bucket is hot but you need to know *which function inside it* is the culprit, use Tracy's headless CLI. Build with the `profile-tracy` Cargo feature (forwards to `bevy/trace_tracy` and installs the tracy subscriber globally), capture with `tracy-capture`, then export to CSV with `tracy-csvexport` — one row per unique span (every Bevy system + any manually-annotated zone).

**`--no-default-features` is mandatory.** The default `fast-link` feature enables `bevy/dynamic_linking`, and `bevy_dylib` swallows tracy's C symbols without re-exporting them, breaking the link step. See `docs/perf_overlay.md` for the install + version pinning details and the macOS `CPLUS_INCLUDE_PATH` workaround for building tracy from source.

```bash
mkdir -p debug/tracy

# Build once with the right feature set (no default features → no bevy_dylib).
cargo build --release --no-default-features --features profile-tracy

# Terminal 1 (or `&`): headless capture daemon. Exits when the client disconnects.
tracy-capture -o debug/tracy/run.tracy &

# Terminal 2: run the resulting binary directly. Tracy connects on startup.
./target/release/worldsim --headless --game-defaults --seed 42 --ticks 5000

# Export to CSV once the sim exits
tracy-csvexport debug/tracy/run.tracy > debug/tracy/zones.csv
```

Per-system spans appear in the CSV as `system{name="<full::path::system_name>"}`. Sort by `total_ns` (column 4) to find the hottest functions in the run.

**CSV schema** (one row per zone, fields comma-separated):
`name, src_file, src_line, total_ns, count, mean_ns, median_ns, min_ns, max_ns, stddev_ns`

**Query patterns:**

```bash
# Top 20 hottest zones by cumulative time (total_ns is column 4)
sort -t',' -k4 -gr debug/tracy/zones.csv | head -20

# Top 20 by mean per-call (mean_ns is column 6) — finds expensive one-shots
sort -t',' -k6 -gr debug/tracy/zones.csv | head -20

# Pull one system's stats
grep 'drain_mindgraph_mutations' debug/tracy/zones.csv

# Every zone whose name matches a pattern, sorted by total time
grep 'perception' debug/tracy/zones.csv | sort -t',' -k4 -gr
```

**When to reach for it:**
- "`memory.mindgraph_drain` is 1.6ms — which function inside it?" — `grep mindgraph` against the CSV, or look at the top zones overall.
- "Is `update_visual_perception` doing N² iteration?" — `mean_ns × count` shows per-call cost vs. call frequency.
- Any time `--perf` sub-buckets flatten out more detail than you need.

**Not for:**
- Per-agent cost (zones are sim-wide, same as `--perf`).
- Tests — tracy requires a live client; use `--perf` counters or `TestWorld` inspection instead.
- Small runs — needs at least a few hundred ticks to get meaningful averages.

### 7. TestWorld inspection methods — when debugging from inside a test

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

### "Why did agent X take action Y at tick Z?"

The one-shot DuckDB query the Parquet log was designed for:

```bash
# Run and capture
mkdir -p debug/run
cargo run --release -- --headless --game-defaults --ticks N --seed S \
  --log debug/run/events.parquet

# Query
cargo run --release -q -- --debug debug/run > debug/run/setup.sql
duckdb -init debug/run/setup.sql
```

Inside DuckDB:

```sql
-- The decision itself: winner, admitted actions, urgencies
SELECT * FROM events WHERE type = 'Decision' AND json_extract_string(payload, '$.agent.name') = 'alice' AND tick = 4521;

-- The plan that drove it (joins Decision → PlanGenerated on agent, plan_id)
SELECT * FROM decisions_with_plans WHERE json_extract_string(payload, '$.agent.name') = 'alice' AND tick = 4521;

-- What targets the brain considered during planning that tick
SELECT * FROM events WHERE type = 'TargetEnumerated' AND json_extract_string(payload, '$.agent.name') = 'alice' AND tick = 4521;

-- Why the planner failed (if no plan was produced)
SELECT * FROM events WHERE type = 'PatternRejected' AND json_extract_string(payload, '$.agent.name') = 'alice' AND tick BETWEEN 4500 AND 4521;

-- The GOAP search's own report card
SELECT tick, iterations, exhausted, best_unmet_goals
FROM events WHERE type = 'GoapSearchTelemetry' AND json_extract_string(payload, '$.agent.name') = 'alice' AND tick BETWEEN 4500 AND 4521;
```

For anything the events don't cover, fall back to `--trace agent:alice` and `--dump-mind agent:alice --at-tick 4521`.

### "Why did agents end up in this weird state at the end?"

1. Capture the full event log: `--log debug/run/events.parquet`
2. In DuckDB, find the unusual events:
   ```sql
   SELECT * FROM events WHERE type = 'Death' ORDER BY tick;
   SELECT * FROM events WHERE type = 'PlanAbandoned' ORDER BY tick DESC LIMIT 50;
   ```
3. Walk backward from the symptom — what `MindGraphMutation` / `Decision` / `PlanGenerated` events preceded it?
4. If you need live state, drop into `--inspect` at the tick before the unusual event.

### "This test is flaky"

Flaky tests are non-determinism, period. Don't add retries.

1. Find the seed difference — does the test use `with_seed(0)` consistently?
2. Capture `AgentStateHash` from two runs with different seeds and diff them in DuckDB to find the first tick that diverges:
   ```sql
   -- After attaching two run dirs as `events_a` and `events_b`:
   SELECT a.tick, json_extract_string(a.payload, '$.agent.name'), a.hash AS hash_a, b.hash AS hash_b
   FROM events_a a JOIN events_b b USING (tick, agent)
   WHERE a.type = 'AgentStateHash' AND b.type = 'AgentStateHash' AND a.hash != b.hash
   ORDER BY a.tick LIMIT 1;
   ```
3. Find the timing dependency — is the test asserting after exactly N ticks when N is too tight?
4. Find the wall-clock dependency — `chrono`, `std::time`, anything that isn't `current_tick()`
5. Find the iteration order — are you iterating a `HashMap`? Use `BTreeMap` or sort first.

### "What knowledge does Alice have about Bob?"

```bash
cargo run --release -- --headless --ticks N --seed S --query "alice Bob"
```

### "When did Alice lose trust in Bob?"

```bash
cargo run --release -- --headless --ticks N --seed S --log debug/run/events.parquet
cargo run --release -q -- --debug debug/run > debug/run/setup.sql
duckdb -init debug/run/setup.sql -c "
  SELECT tick, payload FROM events
  WHERE type = 'RelationshipChanged' AND json_extract_string(payload, '$.agent.name') = 'alice'
    AND json_extract_string(payload, '\$.other') = 'bob'
  ORDER BY tick;
"
```

### "How many decisions did each agent make?"

```sql
SELECT agent, COUNT(*) AS n FROM events WHERE type = 'Decision' GROUP BY agent ORDER BY n DESC;
```

### "Reconstruct Alice's MindGraph as of tick 5000"

Replay every `MindGraphMutation` up to that tick. Adds/removes form the running set.

```sql
-- Running triple set at tick 5000: keep the latest op per (subject, predicate, object)
WITH mutations AS (
  SELECT tick, agent,
         json_extract_string(payload, '$.op')        AS op,
         json_extract_string(payload, '$.subject')   AS subject,
         json_extract_string(payload, '$.predicate') AS predicate,
         json_extract_string(payload, '$.object')    AS object
  FROM events WHERE type = 'MindGraphMutation' AND json_extract_string(payload, '$.agent.name') = 'alice' AND tick <= 5000
),
ranked AS (
  SELECT *, ROW_NUMBER() OVER (PARTITION BY subject, predicate, object ORDER BY tick DESC) AS rn
  FROM mutations
)
SELECT subject, predicate, object FROM ranked WHERE rn = 1 AND op = 'Add';
```

### "Show me Alice's glucose curve for a full game day"

```bash
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log-agent alice --log-field needs.glucose --log-field needs.glucose:why \
  --log-file debug/alice-glucose/fields.jsonl

# Plot it:
cat debug/alice-glucose/fields.jsonl | jq -r '"\(.tick)\t\(.needs.glucose)"' > debug/alice-glucose/fields.tsv
```

### "Give me a per-window dashboard of one agent's day" (hunger/thirst/wakefulness + actions)

The move-forward debugging view when you want to eyeball behaviour across a full day. Captures three needs at every tick, plus the full event log, and buckets both into 20-game-minute windows. Each window row shows the end-of-window level, the Δ since the start of the window, and an action breakdown (`Eat×42, Walk×1, ...`) so you can instantly see what drove each bump or drop.

Tune the bucket size (`1200` ticks = 20 game-min) to whatever resolution you want.

```bash
mkdir -p debug/day_sim
cargo run --release -- --headless --game-defaults --seed 42 --ticks 86400 \
  --log debug/day_sim/events.parquet --log-filter agent:Alice \
  --log-agent alice \
  --log-field needs.hunger --log-field needs.hydration --log-field needs.wakefulness \
  --log-file debug/day_sim/fields.jsonl
```

Pick the fields that match what you're investigating. `needs.hunger` is a derived sigmoid — if the question is "why is she hungry" log the raw pools instead (`--log-preset vitals` grabs stomach + glucose + reserves + aerobic + hunger + wakefulness + health in one flag).

```sql
-- Paste into duckdb, or pipe via `duckdb -c "..."`.
CREATE OR REPLACE VIEW fields AS SELECT CAST(tick AS BIGINT) AS tick,
  CAST(needs.hunger AS DOUBLE) AS hunger,
  CAST(needs.hydration AS DOUBLE) AS hydration,
  CAST(needs.wakefulness AS DOUBLE) AS wakefulness
FROM read_json_auto('debug/day_sim/fields.jsonl');

CREATE OR REPLACE VIEW events AS
SELECT tick, event_type, agent, payload
FROM read_parquet('debug/day_sim/events.parquet');

WITH field_buckets AS (
  SELECT CAST(FLOOR((tick - 1.0) / 1200.0) AS INTEGER) AS bucket,
         FIRST(hunger      ORDER BY tick ASC) AS h_s,  LAST(hunger      ORDER BY tick ASC) AS h_e,
         FIRST(hydration   ORDER BY tick ASC) AS hy_s, LAST(hydration   ORDER BY tick ASC) AS hy_e,
         FIRST(wakefulness ORDER BY tick ASC) AS w_s,  LAST(wakefulness ORDER BY tick ASC) AS w_e
  FROM fields GROUP BY 1
),
action_buckets AS (
  SELECT CAST(FLOOR((tick - 1.0) / 1200.0) AS INTEGER) AS bucket,
         json_extract_string(payload, '$.action') AS action, COUNT(*) AS n
  FROM events WHERE event_type = 'ActionStarted' AND json_extract_string(payload, '$.agent.name') = 'Alice'
  GROUP BY 1, 2
),
action_str AS (
  SELECT bucket, string_agg(action || '×' || n, ', ' ORDER BY n DESC) AS actions
  FROM action_buckets GROUP BY bucket
),
labelled AS (
  SELECT f.*, a.actions,
    -- Sim wall-clock starts at GameTime::START_HOUR (currently 6 — see
    -- src/core/time.rs). Mirror that offset in minutes so window labels
    -- match the game-time HUD and TestWorld's GameTime resource.
    (f.bucket * 20 + 6 * 60)       AS start_min,
    ((f.bucket + 1) * 20 + 6 * 60) AS end_min
  FROM field_buckets f LEFT JOIN action_str a USING (bucket)
)
SELECT
  bucket + 1 AS win,
  printf('D%d %02d:%02d-D%d %02d:%02d',
    start_min // 1440 + 1, (start_min % 1440) // 60, start_min % 60,
    end_min   // 1440 + 1, (end_min   % 1440) // 60, end_min   % 60) AS game_time,
  ROUND(h_e, 3)          AS hunger,
  ROUND(h_e  - h_s,  3)  AS d_hunger,
  ROUND(hy_e, 1)         AS hydration,
  ROUND(hy_e - hy_s, 2)  AS d_hydration,
  ROUND(w_e, 3)          AS wakeful,
  ROUND(w_e  - w_s,  3)  AS d_wakeful,
  COALESCE(actions, '-') AS actions
FROM labelled
ORDER BY bucket;
```

Notes:
- **Sim wall-clock starts at `GameTime::START_HOUR`** (6 at time of writing — check `src/core/time.rs` for the current value). The constant feeds an `INITIAL_TICK_OFFSET` that both `--headless` and `TestWorld` apply uniformly, so a 24-hour run goes from `D1 06:00` to `D2 06:00`, not `00:00-24:00`. The SQL above mirrors that offset in minutes — update the `+ 6 * 60` terms if you change `START_HOUR`, or drop them entirely for raw ticks-from-start labelling.
- Use `//` for integer division (the `/` operator returns DOUBLE and casting truncates-then-rounds on some paths).
- Bucket size `1200` = 20 game-min. For 10-min windows use `600`, for 1-game-hour use `3600`.
- `--log-filter agent:Alice` keeps the event-log file tiny when you only care about one agent. Matches case-insensitively against the name *or* the `agent_id` (stable entity debug string).
- The `actions` column counts `ActionStarted` events, not completions — a chain-preempted action can appear many times in one window, which is itself diagnostic (it means the action kept re-proposing but something is preventing progress).

### "Why is Alice's glucose / stamina / hydration dropping?"

```bash
cargo run --release -- --headless --game-defaults --seed 42 --ticks 30000 \
  --why "alice metric:glucose" --at-tick 30000
```

Prints every signed per-second contributor (BMR, each running action's drain, digestion) and the net rate. The same breakdown is under "Details" on each bar in the in-game agent panel. Works for `glucose`, `stamina`, `hydration`, `stomach`, `mood`.

### "Which system is eating my tick budget?"

See §5 — `--perf` (headless) or F3 (windowed). Heaviest parent bucket on top; sub-buckets under the subdivided parents. `jq '.perf_stats' report.json` for the final snapshot when `--report` is set.

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
