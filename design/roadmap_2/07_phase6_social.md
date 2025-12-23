# Phase 6: Social (The Friend)

> **Behavior Goal**: Agents form relationships that affect how they treat each other.

---

## Dependencies

```
Phase 5: Learning
└── Skills (respected for competence)
└── Teaching (requires relationship)

Phase 3: Memory
└── Memories of interactions (build relationships)
└── Events (witnessed behavior affects relationship)
└── Beliefs (about other agents)

Phase 2: Personality
└── Drives (social drive, status drive)
└── Traits (extraversion, agreeableness)
```

---

## New Systems

### Society: Relationships
*Reference: [03_society.md](../03_society.md) Layer 4D*

- **Each pair of agents has a relationship object**
- **Relationship dimensions**:
  - Trust (0-100): builds slowly, destroyed quickly by betrayal
  - Respect (0-100): based on skill, competence
  - Affection (0-100): emotional bond
  - Power balance (-100 to +100): who has leverage
- **Relationship types**: family, friend, rival, lover, acquaintance

### Society: Relationship Dynamics
*Reference: [03_society.md](../03_society.md) Layer 4D*

- **Every interaction creates memory** for both parties
- **Positive interactions** - trust/affection increase slowly
- **Negative interactions** - trust/affection decrease quickly (asymmetric!)
- **Observed behavior** affects relationship
  - Saw them help someone → respect up
  - Saw them steal → trust down

### Engine: Theory of Mind
*Reference: [04_engine.md](../04_engine.md) Layer 7E*

- **Agents model other agents** in their subjective reality
- **Observation updates models**
  - "I saw Grimnar attack child" → Model[Grimnar].aggression += 5
- **Predictions based on models**
  - "Grimnar is dangerous, I should avoid him"
- **Models can be wrong** - limited information

### 🔄 Psychology: Social Influence on Beliefs
*EXTENDS: Phase 3 beliefs*

- **Trusted people's beliefs carry more weight**
- **Respected people become belief leaders**
- Hearing info from trusted source → stronger belief

### UI: Social Graph Lens (Basic)
*Reference: [05_ui.md](../05_ui.md) Layer 9C*

- **Toggle overlay** showing relationship lines
- Green = trust, Red = hatred
- Thickness = strength

---

## Test Scenario: "The Best Friend"

1. Two agents spawn near each other
2. They chat/interact positively over time
3. Affection and trust increase
4. They choose to sit together at meals
5. When one is in danger, the other helps
6. If one betrays the other, trust crashes

---

## Test Scenario: "Social Batteries"

1. Introvert (Low Social Drive) vs Extrovert (High Social Drive).
2. Both receive a social event (e.g., "Waved At").
3. Extrovert: Matches Drive -> Gains Satisfaction/Joy.
4. Introvert: Unsolicited interaction -> Gains mild Stress/Fear.
5. Introvert isolates to recover; Extrovert seeks more people.

---

## What We're NOT Adding Yet

- ❌ Family bonds (Phase 7)
- ❌ Debt/favors (Phase 9)
- ❌ Gossip spreading (Phase 11)
- ❌ Formal relationships like marriage (Phase 7)

---

**Previous**: [Phase 5: Learning](06_phase5_learning.md)  
**Next**: [Phase 7: Family](08_phase7_family.md)
