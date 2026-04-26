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
| post commit | `625bb08` (`feature/#368.spatial-query-cache`) |

## Headline numbers

| metric | pre-#368 | post-#368 | Δ |
|---|---:|---:|---:|
| `update_visual_perception` | 257.5 ms | 60.0 ms | **-76.7%** |
| `FixedMain` (full tick) | 7729.6 ms | 7703.2 ms | -0.3% (within noise) |
| throughput | 647 tps | 649 tps | +0.3% |

The cache was scoped to make `update_visual_perception` cheap — the chunk-bucket
walk and `Vec<Entity>` allocation are now skipped on most ticks for stationary
or slow-moving agents, plus the per-tick `previous = visible.clone()` is gone
(swapped via a `Local<Vec<Entity>>`). The targeted system loses ~197 ms over
5000 ticks, ~77% of its prior cost.

`FixedMain` doesn't move much on this 12-human config because
`update_visual_perception` is only ~3% of total tick time — `arbitrate_every_tick`
and `react_to_danger` dominate and aren't touched here. At 100-human stress the
absolute `update_visual_perception` saving scales roughly linearly with agent
count and should make a visible dent in `FixedMain`.

The downstream consumers (`react_to_danger`, `write_perceptions_to_mind`,
`perceive_other_agents`) are unchanged by this PR — they iterate
`VisibleObjects.entities`, not the spatial index. Their ms numbers move only
within run-to-run noise.

## Per-system table

Sorted by absolute Δ ms (pre minus post). Systems within ±10 ms of single-run
noise omitted.

| system | pre ms | post ms | Δ ms | Δ % |
|---|---:|---:|---:|---:|
| `update_visual_perception` | 257.5 | 60.0 | -197.5 | -76.7 |
| `update_rational_planning` | 1112.0 | 1055.3 | -56.7 | -5.1 |
| `arbitrate_every_tick` | 886.9 | 997.2 | +110.3 | +12.4 |
| `write_perceptions_to_mind` | 661.7 | 684.7 | +23.0 | +3.5 |
| `react_to_danger` | 1432.8 | 1457.4 | +24.6 | +1.7 |
| `perceive_other_agents` | 61.2 | 70.0 | +8.8 | +14.4 |

Everything below `update_visual_perception` is run-to-run noise — none of these
systems read the spatial index or `PerceptionCache`. Single-run variance on the
hot brain/perception systems is consistently ±50–100 ms across repeated
captures of an unchanged binary.

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
