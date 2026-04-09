---
name: test-worldsim
description: >
  Write or debug tests for the worldsim agent simulation. Use the TestWorld harness, scenario builder, and observability tooling.
  TRIGGER when: writing a new test, debugging a failing test, or implementing a feature issue that requires tests before closing.
  DO NOT TRIGGER when: editing docs, config, or files outside src/ and tests/.
---

# Test Worldsim

Write or debug tests for the agent simulation.

## Where the testing infrastructure lives

Read these files before writing tests — they're the source of truth, not this skill:

- `src/testing/world.rs` — `TestWorld` harness, spawn methods, inspection methods
- `src/testing/scenario.rs` — `ScenarioBuilder` for composable test setup
- `src/testing.rs` — public exports
- `src/agent/events.rs` — `SimEvent` enum for what events you can observe
- `src/agent/invariants.rs` — automatic state validation in debug builds
- `src/cli.rs` — CLI flags for headless mode, logging, tracing, inspection

## Writing a new test

1. Use `TestWorld::scenario(seed)` (the builder) for any test with more than one agent or any custom setup.
2. Use `TestWorld::with_seed(seed) + spawn_agent(...)` only for trivial single-agent tests.
3. Always seed the RNG. No wall-clock time, no unseeded randomness. Flaky test = non-determinism somewhere.
4. Place tests in `#[cfg(test)] mod tests` next to the code, OR in a top-level integration test under `tests/`.

Read `src/testing/world.rs` for the spawn/inspection API. Read `src/testing/scenario.rs` for the builder API. Don't guess method names.

## Debugging a failing test

When a test fails and you need to understand why:

1. Add `world.print_agent_state(agent)`, `world.print_brain_decision(agent)`, or `world.print_agent_events(agent, 200)` BEFORE the failing assertion.
2. Run with `cargo test <test_name> -- --nocapture` to see the output.
3. Inspection methods write to stderr — visible in CI logs and `--nocapture`.
4. For full event history during a test, use `world.print_recent_events(N)`.

The full list of inspection methods is in `src/testing/world.rs` — search for `pub fn print_` and `pub fn query_`.

## Debugging via headless mode

For bugs that only reproduce after many ticks or with specific seeds, skip the test runner and use the headless CLI:

```bash
# Capture every event to JSONL for jq analysis
cargo run --release -- --headless --ticks 5000 --seed 42 --log events.jsonl

# Trace one agent's decisions
cargo run --release -- --headless --ticks 5000 --seed 42 --trace agent:alice

# Pause at a specific tick and inspect
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --inspect agent:alice --at-tick 4521

# Search MindGraph
cargo run --release -- --headless --ticks 5000 --seed 42 \
  --query "alice Wolf" --at-tick 4521
```

The full flag list is in `src/cli.rs`. Read it when you need details on filters or formats.

## Test types

- **Unit test** — pure function math (decay, urgency curves). `#[cfg(test)] mod tests` next to the code.
- **Scenario test** — behavioral chain (perception → brain → action → outcome). Uses `TestWorld::scenario()`.
- **Statistical test** — emergent properties. Run 50+ iterations with different seeds, assert property appears in >X% of runs. Never claim emergence from a single run.
- **Invariant** — continuous validity check. Add to `check_invariants_system` in `src/agent/invariants.rs`.

## Rules

- Feature issues must ship with at least one test before closing.
- All tests use seeded RNG.
- Inspection methods are for debugging — remove temporary `print_*` calls before merging unless they're in a permanent diagnostic block.
- Don't reimplement TestWorld helpers in tests — read the existing API first.
