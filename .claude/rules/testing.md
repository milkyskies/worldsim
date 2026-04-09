# Testing

**Write tests for any logic you add or change.** Don't ask — just write them. If a function matches the "worth testing" criteria below, it must have tests before shipping.

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

Project-specific rules for testing the agent simulation.

### Use the TestWorld harness

All agent behavior tests use the `TestWorld` harness from `tests/common/test_world.rs`. Never spin up a full Bevy `App` in tests unless you're specifically testing Bevy integration.

```rust
#[test]
fn hungry_agent_near_food_eats() {
    let mut world = TestWorld::new();
    let agent = world.spawn_agent(AgentConfig { hunger: 90.0, pos: (10.0, 10.0), ..default() });
    let _bush = world.spawn_berry_bush((12.0, 10.0));

    world.tick(100);

    assert!(world.agent_hunger(agent) < 50.0);
}
```

### When to write which kind of test

- **Unit test** — pure function with clear inputs/outputs (urgency math, decay formulas, triple queries). Lives in `#[cfg(test)] mod tests` next to the code.
- **Scenario test** — behavioral chain spanning multiple systems (perception → brain → action → outcome). Lives in `tests/scenarios/`.
- **Statistical test** — emergent properties that are probabilistic (leaders emerge, gossip spreads). Lives in `tests/statistical/`. Run 50+ iterations with different seeds.
- **Invariant** — continuous validity checks (no negative hunger, no dead entities in conversations). Added to the `InvariantPlugin`.

### Feature issues must ship with tests

Before closing a feature issue, there must be at least one test for the behavior. Scenario test if it's a behavioral chain, unit test if it's a pure function. Don't close issues without the test.

### Deterministic only

All tests use seeded RNG via `TestWorld::with_seed(42)`. No test depends on wall-clock time or unseeded randomness.

### Don't test emergent properties with single runs

Emergent behavior is probabilistic. A single run proving "a leader emerged" means nothing. Either test the individual mechanism (unit/scenario) OR write a statistical test that runs N times and asserts the property appears in >X% of runs.
