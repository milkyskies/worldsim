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

### Worth testing
- State machines and lifecycle transitions
- Parsers, extractors, and data transformers
- Ranking/scoring/prioritization logic
- Multi-module flows (message in → task created → queryable)
- Anything with clear invariants or edge cases

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
- No network calls in tests — if you must, use a mock server behind a feature flag
- No `sleep` in tests — use `tokio::time::pause()` for time-dependent logic

## Property testing
Use `proptest` for functions with wide input spaces (parsers, serialization roundtrips, state machines). Add when a unit test feels like it's only covering the happy path.

---

## Worldsim Agent Tests

For writing or debugging tests for the agent simulation, **invoke the `test-worldsim` skill**. It walks through the TestWorld harness, scenario builder, observability tooling, and headless debugging — and points to the source files where the actual API lives.

### Hard rules (apply to all agent tests)

- **Use `TestWorld`, never spin up a raw Bevy `App`.** It lives in `src/testing/world.rs`.
- **Use `TestWorld::scenario(seed)` (the builder) for any test with more than one agent or any custom setup.** Builder lives in `src/testing/scenario.rs`.
- **All tests use seeded RNG.** No wall-clock time, no unseeded randomness. Flaky test = non-determinism somewhere — fix that before adding retries.
- **Feature issues must ship with at least one test before closing.** Scenario test for behavioral chains, unit test for pure functions.
- **Don't test emergent properties with single runs.** Either test the individual mechanism, or write a statistical test that runs N iterations with different seeds and asserts the property appears in >X% of runs.
- **Any new system that produces meaningful state changes must emit `SimEvent`s** (defined in `src/agent/events.rs`). Add new variants there if needed. This is what makes logging, tracing, and debugging tools work.

### Invariants run automatically

The `InvariantPlugin` runs every tick in debug builds (including `cargo test`) and panics immediately on invalid state. When adding a new component with bounds or invariants, extend `check_invariants_system` in `src/agent/invariants.rs`.
