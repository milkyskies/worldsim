# Phase 0: Foundation (The Clock)

> **Behavior Goal**: Time passes in the simulation.

---

## Dependencies

```
None - this is the foundation
```

---

## New Systems

### Core: Tick System
*Reference: All design docs mention "tick" for time-based updates*

- **Tick counter** - increments each frame, the heartbeat of simulation
- **Game time** - converted from ticks (seconds, minutes, hours, days, seasons, years)
- **Simulation controls** - pause, resume, speed (1x/2x/10x)
- **Time display** - UI showing current game time

### World: Basic Tilemap
*Reference: [06_world.md](../06_world.md) Layer 10A*

- **Tile grid** - 2D array of tiles, 1 tile = 1 meter
- **Tile types** - grass, water (just walkable vs non-walkable)
- **Camera** - pan and zoom controls

---

## Test Scenario: "The Clock"

1. Start empty world
2. Watch time display advance (seconds → minutes → hours)
3. Pause - verify time stops
4. Change speed - verify time accelerates

---

**Next**: [Phase 1: Survival](02_phase1_survival.md)
