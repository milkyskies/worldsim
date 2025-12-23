# Phase 10: Politics (The Warlord)

> **Behavior Goal**: Agents form hierarchies, demand tribute, and compete for power.

---

## Dependencies

```
Phase 9: Economy
└── Contracts (vassalage is a contract)
└── Debt (tribute creates debt-like obligations)
└── Property (territory as property)

Phase 6: Social
└── Relationships (power balance dimension)
└── Theory of mind (who is stronger?)
└── Respect (who is feared?)

Phase 4: Bodies
└── Combat capability (strength matters)

Phase 2: Personality
└── Status drive (want to dominate)
└── Agreeableness (low = more ruthless)
```

---

## New Systems

### Society: The Protection Racket
*Reference: [03_society.md](../03_society.md) Layer 8E*

- **Warlord emergence**:
  - High strength + high aggression agent
  - Offers "protection" for "tribute" (food, goods)
  - Uses violence to enforce
- **Vassal chains**:
  - Weak agents serve warlords
  - Vassals tax even weaker agents
  - Hierarchical power structure

### Society: Territory
*Reference: [03_society.md](../03_society.md) Layer 8E*

- **Borders = enforcement range**
  - Not fixed lines, zones of control
  - Where warlord can effectively collect tribute
- **Territory claims**:
  - Claim vs Reality (claim may exceed actual control)
  - Rebels exist within claimed territory

### Society: Civil War
*Reference: [03_society.md](../03_society.md) Layer 8E*

- **Occurs when vassal > liege** (in perceived power)
- **Power calculation**: strength + allies + weapons + supporters
- **Succession crisis**: when warlord dies, vassals compete

### Society: Factions
*Reference: [03_society.md](../03_society.md) Social Dynamics*

- **People with similar beliefs cluster**
- **Shared enemies create alliances**
- **Leadership emerges** (respect + success)
- **Faction identity** becomes personal identity

### Society: Status Hierarchies
*Reference: [03_society.md](../03_society.md) Social Dynamics*

- **Respect aggregates into reputation**
- **Status affects**:
  - Resource access
  - Mating opportunities
  - Whose beliefs spread faster
  - Negotiation power

### UI: Kingdom Map
*Reference: [05_ui.md](../05_ui.md) Layer 9A*

- **Visualize territory claims**
- **Color regions** by warlord's effective tax range
- **"Paper Map" mode** - shows claims, not reality
  - Player sees blue territory
  - Units there might attack (rebels)
- **Claim vs Reality** visible in debug/spy mode

---

## Test Scenario: "The Kingdom"

1. Strongest agent demands food from neighbors
2. Weak neighbors comply (tribute)
3. "Protection racket" zone forms
4. Warlord gains territory visualization
5. Eventually, strong vassal rebels
6. Civil war until new hierarchy

---

## What We're NOT Adding Yet

- ❌ Formal laws (emerge from culture, Phase 11)
- ❌ Legitimacy through time (becomes accepted)
- ❌ Military organization (future depth)

---

**Previous**: [Phase 9: Economy](10_phase9_economy.md)  
**Next**: [Phase 11: Culture](12_phase11_culture.md)
