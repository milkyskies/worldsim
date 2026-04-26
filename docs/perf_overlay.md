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

Parents that hide multiple phases are split into sub-buckets so the F3
overlay and `--perf` table can show where the time actually goes. `skills`
and `biology` only hold two systems each and stay flat.

| bucket | sub-bucket | systems |
| --- | --- | --- |
| `perception` | `visual` | `update_visual_perception`, `write_perceptions_to_mind` |
| `perception` | `sensory` | body / water / grass / temperature / hearing perception, danger reaction |
| `perception` | `social` | social perception, recognition, shared-experience ToM |
| `memory` | `wm_tick` | working-memory processing, decay, belief updater |
| `memory` | `consolidation` | WM → MindGraph consolidation |
| `memory` | `mindgraph_drain` | draining pending triple mutations |
| `brain` | `urgency` | CNS urgency generation |
| `brain` | `planning` | GOAP A* rational planner |
| `brain` | `arbitration` | survival / emotional / rational arbitration |
| `brain` | `history` | brain history bookkeeping |
| `action` | `execution` | action start / tick / apply effects |
| `action` | `world_mutation` | labor accumulation, `becomes`, emitted effects |
| `psyche` | `emotions` | emotion decay, mood, stress, event reactions |
| `psyche` | `relationships` | relationship update + decay |
| `psyche` | `social_drives` | flocking social decay, greeting acknowledgments |
| `psyche` | `territoriality` | territoriality drive update |
| `communication` | `lifecycle` | conversation initiate, continuation eval |
| `communication` | `turn` | intent selection, speaker ToM, receive, emit |
| `skills` | *(flat)* | skill progression, skill decay |
| `biology` | *(flat)* | metabolism, wakefulness, freshness decay |

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
- **New sub-bucket:** add a variant to `PerfSubBucket`, extend the
  `label`/`parent`/`index`/`ALL` entries, bump the `sub_buckets:
  [BucketData; N]` array length in `PerfTracker`, and tag the relevant
  systems with both `.in_set(PerfBucket::X)` and
  `.in_set(PerfSubBucket::Y)`.
- **Assigning systems:** add `.in_set(crate::core::PerfBucket::X)` to
  the relevant `add_systems(FixedUpdate, …)` call. Existing `.after()`
  constraints across buckets are preserved — Bevy resolves orderings by
  system identity, not tuple membership.
- **Tuning the rolling window:** `core::perf::DEFAULT_WINDOW` (currently
  120 ticks = ~2 seconds at 60 tps).

## Complementary profilers

The overlay answers "which logical bucket is eating my tick budget?" —
it stops at the sub-bucket boundary. Two external tools cover the layers
underneath:

- **[Tracy](https://github.com/wolfpld/tracy) + the `profile-tracy` Cargo
  feature.** Build with
  `cargo build --release --no-default-features --features profile-tracy`
  and launch the native Tracy viewer — it connects over TCP and shows
  every Bevy system as a named span with sampled callstacks inside,
  live. Strict superset of the overlay's data plus function-level
  detail. Reach for this when a sub-bucket is hot and you need to know
  which function inside it is the culprit.

  **`--no-default-features` is mandatory.** The default `fast-link`
  feature enables `bevy/dynamic_linking`, which produces a `bevy_dylib`
  that statically swallows `tracy-client-sys` symbols without
  re-exporting them. Building tracy on top then fails to link with
  `Undefined symbols ... ____tracy_emit_zone_begin_alloc`. Drop the
  default features when profiling and you trade fast incremental builds
  for a working tracy binary.

  **Viewer version must match the Rust client's wire protocol exactly.**
  Bevy 0.18 bundles `tracy-client-sys 0.28.0`, which pins Tracy to
  **0.13.1** — see `~/.cargo/registry/src/**/tracy-client-sys-*/tracy/common/TracyVersion.hpp`.
  Later Tracy patch releases (0.13.2+) and any `*-git` AUR package bump
  the protocol and will reject the connection with an
  "incompatible protocol version" error. The AUR `tracy` package rolls
  forward without honoring this, so the reliable path is to build the
  matching tag from source:

  ```bash
  git clone --branch v0.13.1 --depth 1 https://github.com/wolfpld/tracy ~/src/tracy-0.13.1
  cd ~/src/tracy-0.13.1
  for dir in profiler capture csvexport; do
    cmake -B "$dir/build" -S "$dir" -DCMAKE_BUILD_TYPE=Release
    cmake --build "$dir/build" --parallel
  done
  # Symlink onto PATH so `tracy-capture` / `tracy-csvexport` just work:
  mkdir -p ~/.local/bin
  ln -sf ~/src/tracy-0.13.1/profiler/build/tracy-profiler    ~/.local/bin/
  ln -sf ~/src/tracy-0.13.1/capture/build/tracy-capture      ~/.local/bin/
  ln -sf ~/src/tracy-0.13.1/csvexport/build/tracy-csvexport  ~/.local/bin/
  ```

  **macOS gotcha.** If the cmake build fails with
  `fatal error: 'limits' file not found`, the Apple Command Line Tools
  install is missing libc++ headers in the path clang searches by
  default. Export
  `CPLUS_INCLUDE_PATH=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/c++/v1`
  before re-running `cmake --build` (and before the worldsim build with
  `--features profile-tracy`, since `tracy-client-sys` compiles its C++
  client as part of the cargo build).

  After a Bevy bump, re-check `TracyVersion.hpp` and rebuild against
  the matching tag.
- **[samply](https://github.com/mstange/samply).** Install with `cargo
  install samply`, then `samply record cargo run --release --
  --headless --game-defaults --seed 42 --ticks 5000`. Opens a Firefox
  Profiler tab with an interactive flamegraph + call tree. No code
  changes, no Bevy feature flag, no native viewer. Pure function-level —
  it doesn't know what a "Bevy system" is, but it'll tell you `HashMap::
  insert` is 30% of the sample. Good for quick "what's hot" sanity
  checks and for sharing profile traces (browser-viewable URL).

Rule of thumb: overlay → Tracy → samply, in that order of specificity.
Most questions stop at the overlay.
