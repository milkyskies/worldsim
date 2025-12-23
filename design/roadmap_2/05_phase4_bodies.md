# Phase 4: Bodies (The Wounded)

> **Behavior Goal**: Physical injuries affect what agents can do and how they think.

---

## Dependencies

```
Phase 3: Memory
└── Memories (injuries create traumatic memories)
└── Emotions (pain triggers emotional responses)
└── Trait shifts (chronic pain affects personality)

Phase 1: Survival
└── Basic needs (body parts affect capabilities)
```

---

## New Systems

### Biology: Body Part Hierarchy
*Reference: [01_biology.md](../01_biology.md) Layer 3A*

- **Body structure** - head, torso, arms, legs, hands, feet
- **Each part tracks**:
  - Current HP / Max HP
  - Active injuries
  - Pain level
  - Function rate (0-100%)

### Biology: Injury System
*Reference: [01_biology.md](../01_biology.md) Layer 3A*

- **Injury types**: cuts, bruises, fractures, burns, infections, missing
- **Injury effects**:
  - Leg injury → reduced movement speed
  - Arm injury → can't use two-handed tools
  - Eye injury → reduced vision
  - Brain damage → cognitive impairment
- **Injury propagation**:
  - Infections spread through bloodstream
  - Spine damage → paralysis below injury
- **Healing**:
  - Slow over time, faster with rest/nutrition
  - Scars reduce max HP permanently
  - Missing parts don't regenerate

### 🔄 Engine: Action Interruption
*EXTENDS: Phase 1 simple planning*

*Reference: [04_engine.md](../04_engine.md) Layer 7B*

- **Actions are interruptible** - not atomic
- **Pain interrupt** - high pain derails current action
- **Survival brain override** - flee when threatened mid-action
- **Action progress** - track partial completion

### 🔄 Psychology: Pain Affects Decisions
*EXTENDS: Phase 2 personality-weighted planning*

*Reference: [04_engine.md](../04_engine.md) Layer 7E*

- **Pain destroys rationality** - high pain = only seek relief
- **Long-term goals ignored** when pain > 50
- **Chronic pain** affects personality over time (neuroticism up)

### World: Elevation & Terrain
*Reference: [06_world.md](../06_world.md) Layer 10A*

- **Tile elevation** - hills, valleys
- **Slope movement cost** - uphill costs more energy/fatigue
- **Cliffs** - impassable without climbing skill
- **Occlusion** - hills block line of sight

### World: Environmental Hazards
*Reference: [06_world.md](../06_world.md) Layer 10C*

- **Water** - rivers with flow, can drown
- **Temperature** - too hot/cold causes damage over time
- **Fire** - burns, spreads

---

## Test Scenario: "The Cripple"

1. Agent takes damage to left leg
2. Movement speed drops by 50%
3. Agent experiences pain, mood drops
4. Agent prioritizes resting to heal
5. After healing, scar remains (max HP reduced)

---

## Test Scenario: "The Tired Sleeper"

1. Agent exerts energy walking/working.
2. Energy drops below threshold.
3. Sleep becomes highest utility action (overriding minor drives).
4. Sleep restores energy over time.
5. If prevented from sleeping, fatigue penalty accumulates.

---

## What We're NOT Adding Yet

- ❌ Combat system (comes with skills, Phase 5)
- ❌ Medicine skill (Phase 5)
- ❌ Prosthetics/treatment (future)

---

**Previous**: [Phase 3: Memory](04_phase3_memory.md)  
**Next**: [Phase 5: Learning](06_phase5_learning.md)
