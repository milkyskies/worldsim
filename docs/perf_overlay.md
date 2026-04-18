# Performance overlay (F3)

A live, Minecraft-F3-style view of which simulation system is eating your
tick budget.

## Windowed mode

Press **F3** in the running game to toggle a floating panel in the
top-right corner. The panel shows:

- `tick avg` / `tick max` — total wall-clock time for one `FixedMain`
  cycle (the sim tick), averaged over the last 120 ticks.
- `fps` / `frame` — Bevy's own render-frame diagnostics for comparison.
- `window` — how many tick samples are currently in the rolling window.
- A sorted table, one row per *bucket* (logical group of systems). Heaviest
  bucket on top. Buckets using ≥25% of the tick are highlighted red.

Press F3 again to hide.

## Headless mode

Pass `--perf` to a headless run to collect the same measurements and
print them periodically. Output goes to stderr so it doesn't pollute the
`--report` JSON on stdout.

```sh
cargo run --release -- --headless --ticks 5000 --perf --perf-every 1000
```

`--perf-every N` controls the print interval in ticks (default 500). The
printed table looks like:

```
──── perf @ tick 1000 ────
tick avg:  1279.9µs   max:  2264.7µs   window: 120 samples
Σ bucket avg:  6621.7µs  (higher than tick avg → parallel execution)
system             avg µs     max µs   % tick
communication      1122.5     2104.1    87.7%
action             1085.2     2076.6    84.8%
psyche             1065.3     2137.9    83.2%
brain              1026.1     2016.7    80.2%
perception          972.0     1986.8    75.9%
skills              914.2     1929.1    71.4%
memory              322.4      587.6    25.2%
biology             114.0      223.5     8.9%
```

If you also pass `--report`, the final snapshot is appended to the JSON
output under a `perf_stats` field.

## Buckets

| bucket | systems |
| --- | --- |
| `perception` | visual / body / tile / temperature / hearing perception, social perception, recognition, theory-of-mind |
| `memory` | working-memory processing, consolidation, knowledge decay, belief updater, MindGraph mutation drain |
| `psyche` | emotions, relationships, flocking, greetings, territoriality |
| `skills` | skill progression, skill decay |
| `biology` | metabolism, wakefulness, freshness decay |
| `brain` | urgency generation, rational planning, arbitration, brain history |
| `communication` | conversation lifecycle and turn selection |
| `action` | action start / tick / apply, world labor accumulation, `becomes`, emitted effects |

## Reading the numbers honestly

Bucket timings are **wall-clock latency** (begin-marker → end-marker),
not CPU time. Bevy's scheduler runs systems from different buckets in
parallel whenever resource constraints allow, which means two things:

- **Summed percentages can exceed 100%.** A bucket at "80% of tick" is
  saying "my longest system happened to span 80% of the tick's wall
  clock" — not "I consumed 80% of the CPU." The `Σ bucket avg` line in
  the header makes this visible at a glance.
- **The ranking is still honest.** If `brain > perception > memory`,
  brain really is doing more work than perception, which really is doing
  more work than memory. That's the question the overlay is built to
  answer: *which bucket should I optimize first?*

When `Σ bucket avg ≈ tick avg`, the buckets are running serially (maybe
because of heavy `.after()` constraints). When `Σ bucket avg >> tick
avg`, the scheduler is giving you parallelism for free.

## Extending

- **New bucket:** add a variant to `core::perf::PerfBucket`, an entry to
  `PerfBucket::ALL`, a `bucket_markers!` line in `core::perf`, and a
  `.before(...)` / `.after(...)` pair in `PerfPlugin::build`.
- **Assigning systems:** add `.in_set(crate::core::PerfBucket::X)` to
  the relevant `add_systems(FixedUpdate, …)` call. Existing `.after()`
  constraints across buckets are preserved — Bevy resolves orderings by
  system identity, not tuple membership.
- **Tuning the rolling window:** `core::perf::DEFAULT_WINDOW` (currently
  120 ticks = ~2 seconds at 60 tps).
