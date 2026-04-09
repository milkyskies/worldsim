---
paths:
  - "src/**"
  - "tests/**"
---

# Testing

**Write tests for any logic you add or change.** Don't ask — just write them. If a function matches the "worth testing" criteria below, it must have tests before shipping.

## Running tests locally

**Never run the full test suite.** Run only your specific test with `cargo nextest run -E 'test(name)'`. CI runs the full suite.

## When to write tests

**Test logic, not plumbing.** If a function makes decisions, transforms data, or enforces invariants — test it. If it mostly calls an external API and passes through results — don't.

### Worth testing (unit tests)
- State machines and lifecycle transitions
- Parsers, extractors, and data transformers
- Ranking/scoring/prioritization logic
- Context assembly and truncation
- Anything with clear invariants or edge cases

### Worth testing (integration tests)
- Database queries — does the query return what you expect against a real test database?
- Multi-module flows — message in -> task created -> queryable
- Pipeline composition — do chained steps produce correct output?

### Schema and constraint tests (recommended for schema changes)
- **UNIQUE indexes**: test that duplicates are rejected, and that allowed combinations succeed
- **CHECK constraints**: test valid and invalid values
- **DEFAULT values**: test that records created without the field get the expected default
- **Complex queries**: test that JOINs, CTEs, or window functions return expected results

### Not worth testing
- Thin API wrappers — you'd be testing your mock
- Single-line delegations or trivial getters
- Serialization/deserialization already covered by derives

## How to write tests

### Unit tests live next to the code
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_cannot_transition_from_idle_to_done() {
        // ...
    }
}
```

### Test naming
- Name tests after the behavior, not the function: `task_with_past_due_date_is_overdue` not `test_is_overdue`
- Group related tests in the same `mod tests` block

### Keep tests fast
- Use a dedicated test database for integration tests
- No network calls in tests — if you must, use a mock server behind a feature flag
- No `sleep` in tests — use `tokio::time::pause()` for time-dependent logic

## Property testing
Use `proptest` for functions with wide input spaces (parsers, serialization roundtrips, state machines). Add when a unit test feels like it's only covering the happy path.

---

## Worldsim Agent Tests

Project-specific rules for testing the agent simulation. The testing infrastructure lives in `src/testing/` and is re-exported from the `worldsim::testing` module.

### Use the TestWorld harness

All agent behavior tests use the `TestWorld` harness from `src/testing/world.rs`. Never spin up a full Bevy `App` directly — `TestWorld` gives you a real headless Bevy app with all simulation plugins loaded, just without rendering or input.

```rust
use worldsim::testing::{TestWorld, AgentConfig};
use bevy::math::Vec2;

#[test]
fn hungry_agent_near_food_eats() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(10.0, 10.0),
        hunger: 90.0,
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(12.0, 10.0), 5);

    world.tick(200);

    assert!(world.agent_hunger(agent) < 50.0);
}
```

### TestWorld API

**Construction (always deterministic):**
- `TestWorld::new()` — seed 0
- `TestWorld::with_seed(seed: u64)` — use this when the test needs reproducibility
- `world.seed() -> u64`

**Spawning:**
- `world.spawn_agent(config: AgentConfig) -> Entity`
- `world.spawn_agent_cluster(n: usize, near: Vec2) -> Vec<Entity>`
- `world.spawn_deer(pos: Vec2) -> Entity`
- `world.spawn_berry_bush(pos: Vec2, berries: u32) -> Entity`
- `world.spawn_apple_tree(pos: Vec2, apples: u32) -> Entity`

**Simulation:**
- `world.tick(n: u64)` — advance N ticks; all simulation systems run
- `world.current_tick() -> u64`

**Inspection:**
- `world.get::<Component>(entity) -> &Component`
- `world.get_mut::<Component>(entity) -> Mut<Component>`
- `world.entity_exists(entity) -> bool`
- `world.distance(a, b) -> f32`
- `world.all_agents() -> Vec<Entity>`

**Convenience queries (prefer these over raw `get`):**
- `world.agent_knows(agent, other) -> bool`
- `world.agent_trust(agent, other) -> f32`
- `world.agent_hunger(agent) -> f32`
- `world.agent_energy(agent) -> f32`
- `world.has_item(entity, concept) -> bool`
- `world.item_count(entity, concept) -> u32`
- `world.current_action(agent) -> Option<ActionType>`
- `world.has_registered_action(action) -> bool`

### Scenario Builder (preferred for non-trivial setups)

For any test beyond a single agent, use `TestWorld::scenario(seed)` instead of raw `spawn_*` calls. The builder handles map sizing, terrain, named agents, groups, relationships, and resources in one fluent chain.

```rust
use worldsim::testing::TestWorld;
use bevy::math::Vec2;

#[test]
fn alice_warns_bob_about_wolf() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(16, 16)
        .noise_biomes(false)
        .agent("alice")
            .pos(Vec2::new(40.0, 40.0))
            .personality(|p| p.extraversion(0.8).neuroticism(0.6))
            .knowledge(vec![/* wolf sighting triple */])
            .done()
        .agent("bob")
            .pos(Vec2::new(42.0, 40.0))
            .personality(|p| p.agreeableness(0.9))
            .done()
        .relationship("alice", "bob", |r| r.trust(0.8).affection(0.7))
        .build();

    let alice = agents["alice"];
    let bob = agents["bob"];

    world.tick(200);

    // assert bob now knows about the wolf
}
```

**ScenarioBuilder methods:**
- `.map_size(width, height)` — smaller is faster (default 128x128, prefer 16x16 or 32x32 for unit tests)
- `.tile_at(x, y, tile_type)` — place a single tile
- `.fill_rect(x, y, w, h, tile_type)` — fill a region
- `.noise_biomes(false)` — disable noise gen for flat-grass speed
- `.agent(name)` — start an `AgentBuilder` (terminate with `.done()`)
- `.group(name)` — start a `GroupBuilder` (terminate with `.done()`)
- `.relationship(a, b, |r| r.trust(0.8))` — set pre-existing relationship
- `.berry_bushes(count, near)` / `.apple_trees(count, near)` — clustered resources
- `.build() -> (TestWorld, ScenarioEntities)`

**AgentBuilder methods:**
- `.pos(Vec2)` / `.hunger(f32)` / `.energy(f32)` / `.social_drive(f32)`
- `.personality(|p| p.extraversion(0.8).neuroticism(0.6))`
- `.in_group(name)` — assign to a previously declared group
- `.knowledge(Vec<Triple>)` — pre-load MindGraph triples
- `.done()` — return to ScenarioBuilder

**GroupBuilder methods:**
- `.agents(n)` — how many to spawn
- `.near(Vec2)` — cluster center
- `.personality(|p| ...)` / `.hunger(f32)` / `.energy(f32)` — shared traits
- `.knows_each_other(true)` — write mutual `Knows` triples
- `.done()` — return to ScenarioBuilder

**PersonalityBuilder:** `.openness`, `.conscientiousness`, `.extraversion`, `.agreeableness`, `.neuroticism` (all clamped 0..1)

**RelBuilder:** `.trust`, `.affection`, `.respect` (all clamped 0..1)

**ScenarioEntities access:**
- `agents["alice"]` — index by name (panics if not found)
- `agents.get("alice")` — same as index
- `agents.group("village_a") -> &[Entity]` — group lookup

**Presets** (thin wrappers — use the builder for anything custom):
- `TestWorld::solo_agent(seed) -> (Self, Entity)`
- `TestWorld::two_strangers(seed) -> (Self, Entity, Entity)`

### Text inspection (debugging failing tests)

Output goes to stderr — visible in `cargo test -- --nocapture` and CI logs.

- `world.print_agent_state(agent)` — full snapshot: position, action, brain, needs, emotions, body
- `world.print_brain_decision(agent)` — last brain decision with all proposals, urgencies, powers
- `world.print_mind_graph(agent)` — full MindGraph dump with metadata
- `world.print_relationships(agent)` — all relationships with trust/affection/respect
- `world.print_conversation(agent)` — current conversation state if in one
- `world.query_knowledge(agent, "Wolf") -> Vec<String>` — text search across triples
- `world.print_recent_events(last_n_ticks)` — SimEvents from the last N ticks
- `world.print_agent_events(agent, last_n_ticks)` — SimEvents for one agent in last N ticks

```rust
#[test]
fn hungry_agent_eats() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig { hunger: 90.0, ..Default::default() });
    world.spawn_berry_bush(Vec2::new(12.0, 10.0), 5);
    world.tick(200);

    if world.agent_hunger(agent) >= 50.0 {
        // Test is about to fail — dump everything
        world.print_agent_state(agent);
        world.print_brain_decision(agent);
        world.print_agent_events(agent, 200);
    }
    assert!(world.agent_hunger(agent) < 50.0);
}
```

### When to write which kind of test

- **Unit test** — pure function with clear inputs/outputs (urgency math, decay formulas, triple queries). Lives in `#[cfg(test)] mod tests` next to the code.
- **Scenario test** — behavioral chain spanning multiple systems (perception → brain → action → outcome). Uses `TestWorld`. Lives in `#[cfg(test)] mod tests` next to the relevant system OR in a top-level integration test file under `tests/`.
- **Statistical test** — emergent properties that are probabilistic (leaders emerge, gossip spreads). Uses `TestWorld::with_seed()` + a loop of N iterations with different seeds. Lives under `tests/statistical/` (when that directory is created).
- **Invariant** — continuous validity checks. Added to `InvariantPlugin` in `src/agent/invariants.rs`. Runs every tick in debug builds via the `Last` schedule.

### Headless mode CLI

For batch runs and manual exploration beyond unit tests:

```bash
# Run headless for 5000 ticks with seed 42, print report on exit
cargo run --release -- --headless --ticks 5000 --seed 42 --report

# Control population
cargo run --release -- --headless --ticks 1000 --humans 10 --deer 5 --berry-bushes 15 --apple-trees 8

# Standard unit/scenario tests
cargo test
```

Population flags: `--humans N`, `--berry-bushes N`, `--apple-trees N`, `--deer N`.

### Observability: SimEvent bus

Every meaningful state change in the simulation is emitted as a `SimEvent` from `src/agent/events.rs`. This is the foundation for logging, decision tracing, and debugging — every observability tool below subscribes to this stream.

**Variants** (one event per state change, all carry `tick: u64` and the relevant `agent: Entity`):

```rust
SimEvent::Decision { agent, tick, winner, chosen_actions, powers, proposals }
SimEvent::ActionStarted { agent, tick, action, target }
SimEvent::ActionCompleted { agent, tick, action }
SimEvent::ActionPreempted { agent, tick, preempted_action }
SimEvent::ActionFailed { agent, tick, action, reason }
SimEvent::ConversationStarted { participants, tick, conversation_id }
SimEvent::ConversationEnded { participants, tick, conversation_id }
SimEvent::ConversationAbandoned { abandoner, abandoned, tick }
SimEvent::RelationshipChanged { agent, other, tick, dimension, old_value, new_value }
SimEvent::EmotionTriggered { agent, tick, emotion, intensity }
SimEvent::Death { agent, tick, cause }
SimEvent::EntityPerceived { agent, tick, target }
SimEvent::StrangerDetected { agent, tick, stranger }
SimEvent::KnowledgeShared { speaker, listener, tick, triple_count }
```

**Any new system that produces meaningful state changes must emit SimEvents.** Add new variants to `src/agent/events.rs` if your system needs to report something not covered above.

### Observability: structured event log (JSONL)

Capture every SimEvent during a headless run as JSONL — one JSON object per line. Use with `jq` for post-mortem analysis.

```bash
# Log everything to file
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.jsonl

# Log to stdout (pipe-friendly)
cargo run --release -- --headless --ticks 5000 --seed 42 --log -

# Filter at capture time
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.jsonl \
  --log-filter agent:alice \
  --log-filter type:Decision,ActionStarted \
  --log-filter tick:1000-2000
```

**Flags:**
- `--log <path|->` — output file path or `-` for stdout
- `--log-filter <expr>` — repeatable, prefixes: `agent:<name>`, `type:<Type1,Type2>`, `tick:<start>-<end>`

**jq examples:**
```bash
# All decisions for one agent
cat events.jsonl | jq 'select(.agent == "alice" and .type == "Decision")'

# All deaths
cat events.jsonl | jq 'select(.type == "Death")'

# Count events by type
cat events.jsonl | jq -r .type | sort | uniq -c | sort -rn
```

### Observability: decision trace

A per-agent ring buffer of brain decisions and recent events, dumped on exit. Use this when you need a focused view of one agent's decision history without parsing the full event log.

```bash
# Trace one agent's decisions to stderr (text format)
cargo run --release -- --headless --ticks 5000 --seed 42 --trace agent:alice

# Trace all agents (verbose)
cargo run --release -- --headless --ticks 5000 --seed 42 --trace all

# Limit to a tick range
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --trace agent:alice --trace-ticks 4500-4600

# JSONL output to file
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --trace agent:alice --trace-format jsonl --trace-file alice_trace.jsonl
```

**Flags:**
- `--trace <agent:name|all>` — which agents to trace
- `--trace-ticks <start-end>` — only trace within this tick range
- `--trace-format <text|jsonl>` — output format (default text)
- `--trace-file <path>` — write to file instead of stderr

### Observability: ad-hoc inspection

Run the sim, pause at a specific tick, inspect agent state from the command line. No code changes needed.

```bash
# Snapshot agent state at a specific tick
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --at-tick 4521

# Dump full MindGraph
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --dump-mind agent:alice --at-tick 4521

# Search MindGraph by text
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --query "alice Wolf" --at-tick 4521
```

**Flags:**
- `--inspect <agent:name>` — repeatable; print full agent state snapshot
- `--dump-mind <agent:name>` — repeatable; print agent's MindGraph
- `--query "<agent> <text>"` — repeatable; search MindGraph by text
- `--at-tick <N>` — pause and run inspection commands at this tick (default: final tick)

### Invariants (automatic in debug builds)

The `InvariantPlugin` runs every tick in debug builds (including `cargo test`) and panics immediately on invalid state. It checks:

- `PhysicalNeeds` — hunger/thirst/energy/health ∈ [0, 100]
- `Consciousness.alertness` ∈ [0, 1]
- `PsychologicalDrives` — all drives ∈ [0, 1]
- `EmotionalState.mood` ∈ [-1, 1], `stress_level` ∈ [0, 100], emotion intensity/fuel valid
- `Body` — each part's `function_rate` ∈ [0, 1], `current_hp ≤ max_hp`
- `InConversation` references existing conversations (no dangling conversation IDs)

When adding a new component or invariant to uphold, extend `check_invariants_system` in `src/agent/invariants.rs`.

### Feature issues must ship with tests

Before closing a feature issue, there must be at least one test for the behavior. Scenario test if it's a behavioral chain, unit test if it's a pure function. Don't close issues without the test.

### Deterministic only

All tests use seeded RNG via `TestWorld::with_seed(42)`. No test depends on wall-clock time or unseeded randomness. If a test is flaky, the root cause is non-determinism — fix that before adding retries.

### Don't test emergent properties with single runs

Emergent behavior is probabilistic. A single run proving "a leader emerged" means nothing. Either test the individual mechanism (unit/scenario) OR write a statistical test that runs N iterations with different seeds and asserts the property appears in >X% of runs.
