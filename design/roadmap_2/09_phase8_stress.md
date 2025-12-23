# Phase 8: Stress (The Breakdown)

> **Behavior Goal**: Agents crack under pressure, revealing their true nature and sometimes losing control entirely.

---

## Dependencies

```
Phase 7: Family
└── Genetics (willpower partially genetic)
└── Phenotypes (stress response threshold)

Phase 6: Social  
└── Relationships (social pressure to maintain mask)
└── Theory of mind (others observing your behavior)

Phase 3: Memory
└── Memories (suppressed desires, traumas)
└── Emotions (stress interacts with emotions)

Phase 2: Personality
└── All traits (mask = hiding true traits)
└── Drives (suppressed drives build stress)
```

---

## New Systems

### Psychology: Willpower
*Reference: [02_psychology.md](../02_psychology.md) Layer 8G*

- **Willpower = mental HP**
- **Formula**: (Conscientiousness × 0.5) + (100 - Neuroticism × 0.5) + (Energy × 0.2)
- **Uses**:
  - Resist temptation (don't steal despite hunger)
  - Force unpleasant actions (work when tired)
  - Maintain the Mask (see below)
  - Resist "The Snap" (see below)
- **Depletes** when resisting urges, **restores** with rest

### Psychology: The Mask
*Reference: [02_psychology.md](../02_psychology.md) Layer 8A*

- **True Self** - the calculated personality (e.g., Aggression: 80)
- **Mask** - the persona projected for social role (e.g., "Noble Knight" Aggression: 10)
- **Mask Dissonance** - distance between true self and mask
- **Strain Equation**: leakage_chance = (dissonance × 0.2) + (stress × 0.01)
- **The Slip** - when mask fails, micro-action reveals true nature
  - A sneer, cruel joke, moment of genuine emotion
  - Observant witnesses notice

### Psychology: Stress Hydro-System
*Reference: [02_psychology.md](../02_psychology.md) Layer 8B*

- **Stress is hydraulic pressure** - builds up, must be released
- **Input (builds stress)**:
  - Suppression (vetoing survival brain for social brain)
  - Uncertainty (unknown environment, low information)
  - Mask maintenance (high dissonance = constant stress gain)
- **Output (releases stress)**:
  - Indulgence (eating, sleeping, satisfying drives)
  - Vices (alcohol, gambling - fast relief, long-term cost)
  - Venting (screaming, violence - transfers stress to others)

### Psychology: The Snap
*Reference: [02_psychology.md](../02_psychology.md) Layer 8B*

- **When Stress > Willpower**: agent enters Fugue State
- **Fugue State**:
  - Rational brain disabled
  - Social brain disabled
  - Survival brain takes over
  - Execute highest immediate stress relief regardless of consequences
- **Examples**: loyal soldier deserts, polite person screams, disciplined person binges

---

## Test Scenario: "The Hangry Man"
1. Agent's Hunger increases.
2. Moderate Hunger: Agreeableness trait modifier applied (x0.3).
3. Agent interacts with neutral event.
4. Due to lowered Agreeableness, agent reacts negatively/aggressively.
5. High Hunger: Anxiety/Stress builds up due to unmet Survival Need.

---

## Test Scenario: "The Breakdown"

1. Agent is overworked (energy depleted)
2. Agent is hungry (need unsatisfied)
3. Agent is insulted (stress from social conflict)
4. Stress exceeds willpower
5. Agent enters Fugue State
6. Agent abandons post and flees to sleep/eat/be alone

---

## Test Scenario: "The Spy"

1. Agent with high aggression has mask "peaceful diplomat"
2. High dissonance drains willpower constantly
3. Stress accumulates over days
4. Eventually slips - snarls at someone in anger
5. Witness notices, suspects true nature

---

## What We're NOT Adding Yet

- ❌ Vices system (alcohol, gambling) - future depth
- ❌ Stress transfer through venting (Phase 11 social dynamics)

---

**Previous**: [Phase 7: Family](08_phase7_family.md)  
**Next**: [Phase 9: Economy](10_phase9_economy.md)
