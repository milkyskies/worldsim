# Tracy headless A/B — issue #679 bucket VisibleObjects by concept

100-human stress headless run, seed 42, 2000 ticks, captured before and after
the per-concept bucketing change. Methodology follows
`scripts/perf-snapshot.sh` (Tracy `--features profile-tracy`,
`tracy-csvexport`) but with `--humans 100`.

## Run config

| field | value |
|---|---|
| date | 2026-04-27 |
| host | macOS arm64 (Apple silicon, M-series) |
| build | `cargo build --release --no-default-features --features profile-tracy` |
| invocation | `./target/release/worldsim --headless --game-defaults --humans 100 --seed 42 --ticks 2000` |
| pre commit  | `15b40ce` (main, post-#676) |
| post commit | `feature/#679.tile-visibility-cache` |

## Headline numbers

| metric | pre-#679 | post-#679 | Δ |
|---|---:|---:|---:|
| wall-clock (2000 ticks) | 31.0s | 20.1s | **-35%** |
| effective TPS | 64 | **100** | **+56%** |
| `FixedMain` per-tick mean | 15.06 ms | 9.55 ms | **-5.51 ms (-37%)** |
| `react_to_danger` per-tick mean | 5.60 ms | 0.46 ms | **-92%** |
| `arbitrate_every_tick` per-tick mean | 1.79 ms | 1.01 ms | **-44%** |

`react_to_danger`'s collapse is the headline — the per-visible-entity
`mind.has_trait(concept, Dangerous)` MindGraph query is replaced by one
trait check per visible *concept* (typically 5–10 concepts vs 20+ entities).
At 100 humans this drops the system from the dominant cost to a
near-noise contributor.

`arbitrate_every_tick` falls because `find_closest_dangerous` (called
inside it) uses the same bucketing path. Half-second saving on top of the
10 Hz brain-rate work in #676.

## Per-system table

Sorted by absolute Δ ms (pre minus post) over the 2000-tick run. Systems
within ±100 ms of single-run noise omitted.

| system | pre ms | post ms | Δ ms | Δ % |
|---|---:|---:|---:|---:|
| `react_to_danger` | 11196.6 | 927.9 | -10268.7 | -91.7 |
| `arbitrate_every_tick` | 3579.5 | 2020.8 | -1558.7 | -43.5 |
| `update_visual_perception` | 615.0 (est) | 861.4 | +246.4 | +40.1 |
| `write_perceptions_to_mind` | 6681.4 | 6890.3 | +208.9 | +3.1 |
| `update_rational_planning` | 525.9 | 547.0 | +21.1 | +4.0 |
| `check_recognition` | 3475.8 | 3495.4 | +19.6 | +0.6 |
| `perceive_other_agents` | 1167.5 | 1099.2 | -68.3 | -5.9 |

`update_visual_perception` grows because the scan loop now also writes
`by_concept` buckets — small absolute cost (~120 μs/tick) compared to
the ~5 ms saved downstream. `check_recognition` and
`write_perceptions_to_mind` are unchanged in shape; their costs move
within run-to-run noise.

## Notes

- The original issue (per-tile entity cache) was based on the assumption
  that downstream consumers were doing independent spatial scans. They
  are not — only `update_visual_perception` does. The actual hot path
  was per-visible-entity MindGraph queries inside loops over
  `VisibleObjects.entities`. This rewrite targets that path.
- `write_perceptions_to_mind` is now the dominant per-tick cost (3.45
  ms/tick, 36% of FixedMain). It writes 5 MindGraph triples per visible
  entity per agent and isn't trait-filtered, so concept bucketing
  doesn't help. Next 100-human perf work should target it (cadence
  reduction or batched assertion).
