# Phase 2: Personality (The Individual)

> **Behavior Goal**: Two agents in the same situation make different choices based on who they are.

---

## Dependencies

```
Phase 1: Survival
└── Basic needs (personality affects how urgently needs are pursued)
└── Simple planning (personality weights action costs)
└── Affordances (personality enables/blocks certain actions)
```

---

## New Systems

### Psychology: Big 5 Personality Traits
*Reference: [02_psychology.md](../02_psychology.md) Layer 3B*

- **Openness** (0-100) - curiosity vs traditionalism
- **Conscientiousness** (0-100) - discipline vs spontaneity  
- **Extraversion** (0-100) - social energy vs solitude
- **Agreeableness** (0-100) - compassion vs self-interest
- **Neuroticism** (0-100) - anxiety vs emotional stability

For now: **randomly assigned at spawn** (genetics come in Phase 7)

### Psychology: Drives
*Reference: [02_psychology.md](../02_psychology.md) Layer 3C*

- **Curiosity** - explore, try new things
- **Social** - seek companionship
- **Status** - gain respect, power
- **Security** - avoid risk, hoard resources
- **Autonomy** - resist control

Drive strengths derived from personality traits.

### Psychology: Simple Goals
*Reference: [02_psychology.md](../02_psychology.md) Layer 4E (partial)*

- **Drive-based goals** - high curiosity → "explore new area"
- **Need-based goals** - hungry → "find food" (from Phase 1)
- **Goal priority** - drives compete with needs

### 🔄 Engine: Personality-Weighted Planning
*EXTENDS: Phase 1 simple planning*

*Reference: [04_engine.md](../04_engine.md) Layer 6B*

- **Action cost modifiers** based on personality:
  - High Agreeableness: stealing cost × 10
  - Low Agreeableness: stealing cost × 1
  - High Neuroticism: risky actions cost more
  - High Openness: unfamiliar actions cost less

---

## Test Scenario: "The Choice"

1. Spawn two agents: one High Agreeableness, one Low Agreeableness
2. Both are starving, only one food source, owned by someone else
3. Low Agreeable agent steals the food
4. High Agreeable agent waits or searches elsewhere
5. Same situation, predictably different outcomes

---

## What We're NOT Adding Yet

- ❌ Trait calculation from genes (Phase 7)  
- ❌ Trait changes from experiences (Phase 3)
- ❌ The Mask/social camouflage (Phase 8)

---

**Previous**: [Phase 1: Survival](02_phase1_survival.md)  
**Next**: [Phase 3: Memory](04_phase3_memory.md)
