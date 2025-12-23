# Phase 1: Survival (The Survivor)

> **Behavior Goal**: Agent seeks food when hungry to avoid death.

---

## Dependencies

```
Phase 0: Foundation
└── Tick system (hunger increases per tick)
└── Tilemap (agent moves on tiles)
```

---

## New Systems

### Biology: Basic Needs
*Reference: [02_psychology.md](../02_psychology.md) Layer 5 (State)*

- **Hunger** (0-100) - increases each tick, death at 100
- **Energy** (0-100) - depletes with movement, restored by sleep
- **State affects behavior** - high hunger → seek food urgently

### Agent: Inventory
*Reference: [01_biology.md](../01_biology.md) Layer 10*

- **Item storage** - list of items agent carries
- **Pickup/drop** - collect items from world
- **Consumption** - eat food, reduce hunger

### World: Resources
*Reference: [06_world.md](../06_world.md) Layer 10B*

- **Resource nodes** - apple trees, berry bushes
- **Harvesting** - interact to get food items
- **Finite resources** - trees deplete, respawn over time

### Engine: Simple Perception
*Reference: [04_engine.md](../04_engine.md) Layer 6A (partial)*

- **Vision range** - agent sees nearby tiles
- **Object detection** - can see food sources

### Engine: Simple Planning
*Reference: [04_engine.md](../04_engine.md) Layer 6B (partial)*

- **Need-based goals** - hungry → goal: eat
- **Find nearest** - locate closest food source
- **Move toward** - pathfind to target

### Engine: Affordances (Basic)
*Reference: [04_engine.md](../04_engine.md) Layer 7A*

- **Objects afford actions** - tree affords "pick fruit", food affords "eat"
- **Context-aware** - only show valid actions

### World: Light (Basic)
*Reference: [06_world.md](../06_world.md) Layer 10C.3*

- **Day/night cycle** - tied to game time
- **Vision reduction** - harder to see at night
- **Behavior effect** - agents prefer to act during day

---

## Test Scenario: "The Starving Man"

1. Spawn agent with hunger at 0
2. Watch hunger increase over time
3. When hunger > 50, agent should seek food
4. Agent finds tree, picks apple, eats, hunger drops
5. If no food available, agent dies at hunger 100

---

## What We're NOT Adding Yet

- ❌ Personality (Phase 2)
- ❌ Memory (Phase 3)
- ❌ Body parts/injuries (Phase 4)
- ❌ Skills (Phase 5)
- ❌ Relationships (Phase 6)

---

**Previous**: [Phase 0: Foundation](01_phase0_foundation.md)  
**Next**: [Phase 2: Personality](03_phase2_personality.md)
