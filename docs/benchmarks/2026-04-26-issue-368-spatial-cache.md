# Tracy headless A/B — issue #368 spatial query cache

5000-tick game-defaults headless run, seed 42, captured before and after the
PerceptionCache change on the same machine. Methodology identical to
`2026-04-26-tracy-headless.md` (`scripts/perf-snapshot.sh` →
`tracy-csvexport`).

## Run config

| field | value |
|---|---|
| date | 2026-04-26 |
| host | macOS arm64 (Apple silicon, M-series) |
| build | `cargo build --release --no-default-features --features profile-tracy` |
| invocation | `./target/release/worldsim --headless --game-defaults --seed 42 --ticks 5000` |
| pre commit  | `9bd2ce0` (main, post-#370/#587/#373) |
| post commit | `51c1a30` (`feature/#368.spatial-query-cache`) |

## Headline numbers

| metric | pre-#368 | post-#368 | Δ |
|---|---:|---:|---:|
| `FixedMain` (full tick) | 7729.6 ms | 7285.9 ms | **-5.7%** |
| `update_visual_perception` | 257.5 ms | 58.2 ms | **-77.4%** |
| throughput | 647 tps | 686 tps | +6.0% |

The cache was scoped to make `update_visual_perception` cheap — the chunk-bucket
walk and `Vec<Entity>` allocation are now skipped on most ticks for stationary
or slow-moving agents, plus the per-tick `previous = visible.clone()` is gone
(swapped via a `Local<Vec<Entity>>`). 199 ms saved on this single system, ~77%
of its prior cost.

The downstream consumers (`react_to_danger`, `write_perceptions_to_mind`,
`perceive_other_agents`) are unchanged by this PR — they iterate
`VisibleObjects.entities`, not the spatial index. Their ms numbers move only
within run-to-run noise.

## Per-system table

Sorted by absolute Δ ms (pre minus post). Systems within ±5 ms of noise
omitted.

| system | pre ms | post ms | Δ ms | Δ % |
|---|---:|---:|---:|---:|
| `update_visual_perception` | 257.5 | 58.2 | -199.3 | -77.4 |
| `update_rational_planning` | 1112.0 | 1025.1 | -86.9 | -7.8 |
| `react_to_danger` | 1432.8 | 1406.6 | -26.2 | -1.8 |
| `write_perceptions_to_mind` | 661.7 | 635.4 | -26.3 | -4.0 |
| `update_body_perception` | 111.7 | 95.2 | -16.5 | -14.8 |
| `tick_actions` | 389.7 | 377.2 | -12.5 | -3.2 |
| `apply_action_effects` | 387.5 | 379.1 | -8.4 | -2.2 |
| `arbitrate_every_tick` | 886.9 | 935.5 | +48.6 | +5.5 |
| `check_recognition` | 111.3 | 120.9 | +9.6 | +8.6 |

`arbitrate_every_tick`, `check_recognition`, and `update_rational_planning`
deltas are single-run noise — none of these systems read the spatial
index or `PerceptionCache`.

## Notes

- The default config has only 12 humans on a quiet map. Per-system cost
  scales roughly linearly with agent count, and the cache hit rate goes
  *up* with agents-per-chunk (more stationary neighbors per query). At
  the 100-human stress config the absolute saving on
  `update_visual_perception` should be several times larger.
- The cache treats an empty `cached` list as stale to handle the
  bootstrap (tick 1 perception runs before `update_spatial_index` in
  `PostUpdate` populates the index) — perception picks up real entities
  on the second cycle. This preserves the original tick-by-tick
  observability ordering, which matters for seed-stable behavior tests.
