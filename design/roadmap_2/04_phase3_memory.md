# Phase 3: Memory (The Witness)

> **Behavior Goal**: Agents remember events and those memories influence future decisions.

---

## Dependencies

```
Phase 2: Personality
└── Personality traits (affect memory formation intensity)
└── Goals (memories can create new goals)

Phase 0: Foundation  
└── Tick system (memory decay over time)
```

---

## New Systems

### Engine: Event Objects
*Reference: [04_engine.md](../04_engine.md) Layer 7D*

- **Every significant action becomes an Event**
  - Actor, targets, location, timestamp
  - Witnesses (who saw it happen)
  - Tags (violent, generous, taboo, etc.)
  - Intensity (how memorable, 0-100)
  - Cause chain (this event caused by previous event)

### Psychology: Memory System
*Reference: [02_psychology.md](../02_psychology.md) Layer 4B*

- **Memory formation** - high intensity events create memories
- **Memory structure**:
  - Event reference
  - Emotional valence (positive/negative/mixed)
  - Decay rate (intense memories decay slower)
- **Memory types**:
  - Episodic: "I saw John steal bread"
  - Skill: "I know how to fish" (extended in Phase 5)

### Psychology: Beliefs (Basic)
*Reference: [02_psychology.md](../02_psychology.md) Layer 4A*

- **Pattern extraction** - multiple similar memories → belief
  - "Orcs attacked me 3 times" → Belief: "Orcs are dangerous"
- **Belief strength** - based on memory count and intensity
- **Confirmation bias** - strong beliefs filter new information

### Engine: Subjective Reality
*Reference: [04_engine.md](../04_engine.md) Layer 6A*

- **Agents don't see game state** - they see their subjective copy
- **Working memory** - recent observations, decays fast
- **Long-term beliefs** - persist, may be wrong
- **Surprise** - when reality differs from expectation

### Psychology: Emotions (Basic)
*Reference: [02_psychology.md](../02_psychology.md) Layer 5*

- **Active emotions** - fear, anger, joy, grief
- **Triggered by events** - violence → fear, loss → grief
- **Decay toward baseline** - personality determines baseline
- **Affect decisions** - fear increases risk aversion temporarily

### 🔄 Psychology: Trait Shifts
*EXTENDS: Phase 2 personality*

- **Memories can shift traits over time**
  - Trauma → neuroticism increases
  - Repeated success → confidence increases
- **Slow change** - traits stabilize in adulthood

### 🔄 Goals: Memory-Based
*EXTENDS: Phase 2 goals*

- **Goals from memories**
  - "Father was killed" → Goal: "Avenge father"
  - "I succeeded at smithing" → Goal: "Become master smith"

### Psychology: Emotional Associations (NEW)
*Reference: [02_psychology.md](../02_psychology.md) Layer 5B*

- **Tags map to emotional responses** - "Wolf" → (Fear, 0.8)
- **Multiple emotions per tag** - "Social" → [(Joy, 0.3), (Fear, 0.1)] for introverts
- **Sources**:
  - Genetic (from phenotypes)
  - Cultural (from parents/society)
  - Personal (from experiences)
- **Personality filters emotional interpretation**

---

## Test Scenario: "The Witness"

1. Agent A punches Agent B
2. Agent C is nearby and witnesses the event
3. Inspect Agent C's memory list - contains the violence event
4. Agent C forms belief: "Agent A is violent"
5. Later, Agent C avoids Agent A or refuses to trade with them

---

## What We're NOT Adding Yet

- ❌ Skills/skill memories (Phase 5)
- ❌ Relationship tracking (Phase 6)
- ❌ Gossip/telling others (Phase 11)
- ❌ False memory from gossip (Phase 11)

---

**Previous**: [Phase 2: Personality](03_phase2_personality.md)  
**Next**: [Phase 4: Bodies](05_phase4_bodies.md)
