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

Flags (all optional): `--headless`, `--ticks N` (default 1000), `--seed N` (default 0), `--report`, `--humans N`, `--berry-bushes N`, `--apple-trees N`, `--deer N`.

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
