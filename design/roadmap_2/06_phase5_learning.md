# Phase 5: Learning (The Apprentice)

> **Behavior Goal**: Agents improve at tasks through practice, and can teach others.

---

## Dependencies

```
Phase 4: Bodies
└── Body capabilities (physical limits on actions)
└── Action system (actions grant XP)

Phase 3: Memory
└── Skill memories (procedural, built through practice)
└── Events (learning events become memories)

Phase 2: Personality
└── Openness trait (affects learning rate)
```

---

## New Systems

### Psychology: Skill System
*Reference: [02_psychology.md](../02_psychology.md) Layer 4C*

- **Skills**: combat, smithing, farming, medicine, persuasion, etc.
- **Skill properties**:
  - Level (0-100)
  - XP accumulation
  - Decay rate (unused skills atrophy)
  - Specializations (branch into sub-skills)
- **Learning rates affected by**:
  - Age (young learn faster)
  - Personality (high Openness = faster)
  - Teacher quality (learning from master = huge bonus)
  - Related skills (transfer bonuses)

### Psychology: Knowledge
*Reference: [02_psychology.md](../02_psychology.md) Layer 4C*

- **Knowledge** stored as beliefs with "factual" tag
  - "Bronze = copper + tin" (recipe)
  - "Red berries are poisonous" (information)
- **Sources**: personal discovery, taught, read from books
- **Can be lost** - if no one remembers, knowledge disappears

### Engine: Skill Variance
*Reference: [04_engine.md](../04_engine.md) Layer 7C*

- **Skill determines precision, not permission**
  - Master archer: hit target ± 1° variance
  - Novice archer: hit target ± 45° variance
- **Dunning-Kruger effect**
  - Low skill + high confidence = tragic accidents
  - "I can definitely make that shot" → hits ally

### Society: Teaching
*Reference: [02_psychology.md](../02_psychology.md) Layer 4C*

- **Knowledge transfer** - skilled agent teaches less-skilled
- **Teacher bonus** - learning from expert is much faster than trial/error
- **Teaching requires**:
  - Shared language (Phase 11, for now assume shared)
  - Time spent together
  - Relationship quality (Phase 6, for now assume neutral)

### 🔄 Goals: Skill-Based
*EXTENDS: Phase 2/3 goals*

- **Mastery goals** from success memories
  - "I crafted something good" → Goal: "Master smithing"
- **Subgoal generation** - "Build house" → gather wood, craft planks, etc.

---

## Test Scenario: "The Apprentice"

1. Agent tries to fish (skill level 0)
2. Fails 90% of the time
3. Each attempt grants XP
4. Skill level increases
5. Eventually fails only 50% of time
6. Higher skilled agent can teach, accelerating learning

---

## What We're NOT Adding Yet

- ❌ Skill effects on relationship respect (Phase 6)
- ❌ Skill inheritance through genes (Phase 7)
- ❌ Written knowledge/books (Phase 11 with language)

---

**Previous**: [Phase 4: Bodies](05_phase4_bodies.md)  
**Next**: [Phase 6: Social](07_phase6_social.md)
