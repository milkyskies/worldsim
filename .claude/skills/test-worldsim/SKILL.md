---
name: test-worldsim
description: >
  Write tests for the worldsim agent simulation. Use the TestWorld harness, scenario builder, and the right test type for the situation.
  TRIGGER when: writing a new test, refactoring existing tests, or implementing a feature issue that requires tests before closing.
  DO NOT TRIGGER when: only debugging a failing test or investigating runtime behavior — use the `debug-worldsim` skill for that.
---

# Test Worldsim

Write tests for the agent simulation.

For investigating runtime behavior or debugging a failing test (headless CLI, JSONL log, decision trace, inspection methods), use the `debug-worldsim` skill instead. The two skills overlap on `print_*` inspection methods because debugging often happens from inside a test.

## Where the testing infrastructure lives

Read these files before writing tests — they're the source of truth, not this skill:

- `src/testing/world.rs` — `TestWorld` harness, spawn methods, inspection methods
- `src/testing/scenario.rs` — `ScenarioBuilder` for composable test setup
- `src/testing.rs` — public exports
- `src/agent/events.rs` — `SimEvent` enum for what events you can observe
- `src/agent/invariants.rs` — automatic state validation in debug builds

## Writing a new test

1. Use `TestWorld::scenario(seed)` (the builder) for any test with more than one agent or any custom setup.
2. Use `TestWorld::with_seed(seed) + spawn_agent(...)` only for trivial single-agent tests.
3. Use `TestWorld::game_defaults(seed)` to reproduce the exact game world (128×128 noise map, Realistic placement, same counts as `cargo run`). Identical to `--headless --game-defaults --seed N`.
4. Always seed the RNG. No wall-clock time, no unseeded randomness. Flaky test = non-determinism somewhere.
5. Place tests in `#[cfg(test)] mod tests` next to the code, OR in a top-level integration test under `tests/`.

Read `src/testing/world.rs` for the spawn/inspection API. Read `src/testing/scenario.rs` for the builder API. Read `src/world/spawn_config.rs` for `WorldSpawnConfig` and `SpawnLayout`. Don't guess method names.

## Test types

- **Unit test** — pure function math (decay, urgency curves). `#[cfg(test)] mod tests` next to the code.
- **Scenario test** — behavioral chain (perception → brain → action → outcome). Uses `TestWorld::scenario()`.
- **Statistical test** — emergent properties. Run 50+ iterations with different seeds, assert property appears in >X% of runs. Never claim emergence from a single run.
- **Invariant** — continuous validity check. Add to `check_invariants_system` in `src/agent/invariants.rs`.

## When a test fails

Don't guess. Invoke the `debug-worldsim` skill — it covers `world.print_*` inspection methods, headless reproduction, JSONL event log, decision trace, and ad-hoc inspection.

The short version: add `world.print_agent_state(agent)` or `world.print_agent_events(agent, 200)` before the failing assertion, then run with `cargo nextest run -E 'test(name)' --no-capture` to see the output (nextest swallows stderr/stdout by default). For anything beyond that, use the debug skill.

## Rules

- Feature issues must ship with at least one test before closing.
- All tests use seeded RNG.
- Inspection methods are for debugging — remove temporary `print_*` calls before merging unless they're in a permanent diagnostic block.
- Don't reimplement TestWorld helpers in tests — read the existing API first.
