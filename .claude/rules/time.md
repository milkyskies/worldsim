# Simulation Time

**1 tick = 1 in-game second.** Canonical source: `GameTime::TICKS_PER_SECOND = 1` in `src/core/time.rs`.

Derived constants (already defined on `GameTime`, use them — don't recompute):

- 60 ticks = 1 game minute (`TICKS_PER_MINUTE`)
- 3,600 ticks = 1 game hour (`TICKS_PER_HOUR`)
- 86,400 ticks = 1 game day (`TICKS_PER_DAY`)

At the default 60 real ticks/second, 1 real second = 1 game minute (RimWorld-style compression). That real-time rate is orthogonal to the tick→game-second mapping and is controlled by `game_seconds_per_cycle` / `dt`; physics rates are expressed per-rate-unit where 1 rate-unit = 60 game-seconds.

## Consequences for tuning

- **Rates reported by `--why` and field-logger `:why` are per rate-unit (per-game-minute)**, not per tick or per game-second. A `-0.3` aerobic rate means "drains 0.3 points per game-minute" = "0.005 per game-second" = "5.5 game-hours to empty a 100 pool".
- **When setting physiological drain constants**, benchmark against real-world time-to-exhaustion (walk for hours, jog for hours, sprint for minutes) and convert to per-game-minute rate-units. Do not think in ticks.
- **When writing tests that need wall-clock durations**, express them as `N * GameTime::TICKS_PER_MINUTE` (or hour/day) — never hardcode `3600` or `86400`.
