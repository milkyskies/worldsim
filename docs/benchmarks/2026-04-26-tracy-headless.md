# Tracy headless profile — 2026-04-26

Per-Bevy-system timings for a 5000-tick headless run with the realistic game-defaults map. Captured via `--features profile-tracy` + `tracy-capture`, exported with `tracy-csvexport`.

## Run config

| field | value |
|---|---|
| commit | `682c002` |
| date | 2026-04-26 |
| host | macOS arm64 (Apple silicon, M-series) |
| build | `cargo build --release --no-default-features --features profile-tracy` |
| invocation | `./target/release/worldsim --headless --game-defaults --seed 42 --ticks 5000` |
| wall-clock | 11.98s (417 ticks/s) |
| total zones | 1,201,119 |
| unique zone names | 138 (94 per-Bevy-system spans) |

## Schedule-level breakdown

Almost all the work happens in `FixedMain` → `FixedUpdate`. `Last` is field/event logging; `PostUpdate` is mostly transform propagation.

| schedule | total ms | % of run |
|---|---:|---:|
| `FixedMain` | 11,469 | 91.7 |
| ↳ `FixedUpdate` | 11,216 | 89.7 |
| `Last` | 347 | 2.8 |
| `PostUpdate` | 142 | 1.1 |
| `FixedPreUpdate` | 107 | 0.9 |
| `FixedPostUpdate` | 58 | 0.5 |
| `FixedFirst` | 51 | 0.4 |

## Top 25 hottest systems

Sum of all `system{name=...}` zones over the run, sorted by `total_ns`. `mean μs` is per-call cost (each system runs once per tick).

| rank | total ms | mean μs | system |
|---:|---:|---:|---|
| 1 | 4063 | 813 | `agent::brains::brain_system::arbitrate_every_tick` |
| 2 | 1462 | 292 | `agent::mind::perception::react_to_danger` |
| 3 | 1052 | 210 | `agent::brains::rational::update_rational_planning` |
| 4 | 661 | 132 | `agent::mind::perception::write_perceptions_to_mind` |
| 5 | 609 | 122 | `agent::mind::perception::update_body_perception` |
| 6 | 498 | 100 | `agent::mind::social_perception::perceive_other_agents` |
| 7 | 378 | 75 | `agent::nervous_system::execution::apply_action_effects` |
| 8 | 372 | 74 | `agent::nervous_system::execution::tick_actions` |
| 9 | 361 | 72 | `agent::nervous_system::execution::start_actions` |
| 10 | 265 | 53 | `testing::world::collect_sim_events_into_log` |
| 11 | 170 | 34 | `agent::mind::perception::update_visual_perception` |
| 12 | 156 | 31 | `agent::mind::recognition::check_recognition` |
| 13 | 96 | 19 | `agent::mind::memory::decay_stale_knowledge` |
| 14 | 79 | 16 | `agent::mind::perception::perceive_grass_tiles` |
| 15 | 74 | 15 | `agent::mind::perception::perceive_water_tiles` |
| 16 | 68 | 14 | `agent::mind::consolidation::consolidate_knowledge` |
| 17 | 57 | 11 | `agent::mind::knowledge::drain_mindgraph_mutations` |
| 18 | 50 | 10 | `bevy_transform::systems::sync_simple_transforms` |
| 19 | 42 | 8 | `agent::biology::body::process_healing` |
| 20 | 24 | 5 | `agent::mind::perception::perceive_temperature` |
| 21 | 23 | 5 | `agent::mind::theory_of_mind::update_shared_experience_tom` |
| 22 | 22 | 4 | `agent::biology::combat::bleed_system` |
| 23 | 22 | 4 | `agent::nervous_system::metabolism::tick_metabolism` |
| 24 | 20 | 4 | `agent::psyche::emotions::update_stress` |
| 25 | 20 | 4 | `agent::biology::body::check_death` |

The remaining 69 systems each total under 20ms (under 0.2% of the run).

## Headlines

1. **`arbitrate_every_tick` is the single dominant cost** — 4.06s out of ~12s wall-clock. Roughly 4× the next-heaviest system (`react_to_danger`) and 4× the GOAP planner (`update_rational_planning`). The `--perf` bucket view hides this: arbitration cost is split across `brain.history` and `brain.arbitration` sub-buckets.
2. **`react_to_danger` (1.46s)** is the second-biggest, and the only perception system in 4-figure ms territory. Worth checking whether it scales with population × visible-entities.
3. **GOAP planning is not the bottleneck.** `update_rational_planning` is 1.05s, less than half of arbitration. Optimization effort against the planner has diminishing returns until arbitration is cheaper.
4. **Action pipeline is balanced** — `apply_action_effects` + `tick_actions` + `start_actions` total ~1.1s, split roughly evenly across the three.
5. **`update_visual_perception` is cheap** — 170ms. Despite the name, the FOV/visibility path is not where perception time goes; tile sampling and "react to danger" dominate.
6. **Memory and consolidation are nearly free** — `decay_stale_knowledge` + `consolidate_knowledge` + `drain_mindgraph_mutations` total ~220ms. The mind-graph layer is not a hotspot at this population size.

## Where to look first

If reducing `arbitrate_every_tick` cost is the goal:

- The function is in `src/agent/brains/brain_system.rs` — start there.
- 813μs per tick × 5000 ticks at default population (≈25 agents on the realistic map) = ~32μs per agent per tick. That's high for a function that mostly compares brain proposals; suggests either (a) per-proposal allocation overhead, (b) repeated O(targets) work that could be cached across the survival/emotional/rational arbitrators, or (c) avoidable work for agents that have a stable plan and don't need re-arbitration this tick.
- A 2× speedup here would shave ~17% off total tick time — bigger lever than any other single system.

## Reproduce

```bash
mkdir -p debug/tracy

cargo build --release --no-default-features --features profile-tracy

tracy-capture -o debug/tracy/run.tracy &
./target/release/worldsim --headless --game-defaults --seed 42 --ticks 5000

tracy-csvexport debug/tracy/run.tracy > debug/tracy/zones.csv
```

Then sort `zones.csv` by `total_ns` (column 4) and filter rows whose `name` starts with `system{`. See `docs/perf_overlay.md` for setup details (Tracy version pinning, macOS `CPLUS_INCLUDE_PATH` workaround, why `--no-default-features` is required).
