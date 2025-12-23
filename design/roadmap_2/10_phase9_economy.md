# Phase 9: Economy (The Trader)

> **Behavior Goal**: Agents exchange goods based on perceived value, creating emergent markets.

---

## Dependencies

```
Phase 8: Stress
└── Willpower (resisting bad deals under pressure)
└── Stress (unfair deals cause stress)

Phase 6: Social
└── Relationships (trust affects deals)
└── Theory of mind (predicting trade partner behavior)

Phase 5: Learning
└── Skills (crafted goods, perceived value)
└── Knowledge (recipes, resource locations)

Phase 1: Survival
└── Inventory (goods to trade)
└── Resources (raw materials)
```

---

## New Systems

### Society: Debt System
*Reference: [03_society.md](../03_society.md) Layer 8C*

- **Agent tracks favors owed**
- **Debt types**:
  - Small Favor - consumable, expires
  - Life Debt - permanent, transferable to kin
  - Blood Debt - negative ("you owe me revenge")
- **Leverage** - can command actions from debtors
- **Tragedy** - forced to act against principles to pay debts

### Society: Contracts
*Reference: [03_society.md](../03_society.md) Layer 8C-2*

- **Formal agreements with conditions**
- **Contract properties**:
  - Parties (employer, worker)
  - Obligation (action, target, schedule)
  - Payment (amount, interval)
  - Breach tracking
- **Breach & Enforcement**:
  - System checks if obligations met
  - High breach → casus belli (just cause to fire/punish)
  - Unpaid → worker casus belli (just cause to quit/steal)

### Economy: Barter
*Reference: [03_society.md](../03_society.md) Layer 8E*

- **No global prices** - value is subjective
- **Value = labor time saved**
  - Smith trades hoe (10h to make) for potatoes (100h to grow himself)
- **Trade when mutually beneficial**
- **Trust affects deals** - won't trade with distrusted agents

### Economy: Property
*Reference: Implied by debt/contracts*

- **Ownership tracking** - who owns what
- **Theft** - taking without permission (affected by agreeableness)
- **Inheritance** - property passes to family on death

### World: Resource Growth
*Reference: [06_world.md](../06_world.md) Layer 10B*

- **Flora lifecycle**: seed → sprout → sapling → mature → old → dead
- **Seasonal resources** - fruit only in autumn
- **Regeneration** - forests expand if conditions right
- **Finite resources** - ore veins deplete

### World: Material Properties
*Reference: [06_world.md](../06_world.md) Layer 10B*

- **Wood**: flammable, floats, rots
- **Stone**: heavy, fireproof, durable
- **Iron**: requires smelting, high durability
- **Affects affordances** - what you can do with material

### 🔄 Society: Fairness Drive
*EXTENDS: Phase 2 drives*

- **Agents compare their deals to peers**
- **Inequity** - if my ratio worse than peer's, stress increases
- **Reaction** - demand better terms or quit
- **Creates pressure** for standardized contracts

---

## Test Scenario: "The Favor"

1. Agent A saves Agent B's life → Life Debt created
2. Agent A asks Agent B for something illegal
3. Agent B complies (debt > morals) OR refuses (high integrity)
4. If refuses, relationship damaged, debt remains

---

## Test Scenario: "The Job"

1. Agent A hires Agent B (dawn-to-dusk, 5 gold/day)
2. Agent B shows up at dawn daily
3. One day, Agent B doesn't show → breach count up
4. After 3 breaches, Agent A fires Agent B

---

## What We're NOT Adding Yet

- ❌ Currency (gold as agreed token) - emerges naturally later
- ❌ Markets (physical locations) - future
- ❌ Inflation mechanics - emerges from currency

---

**Previous**: [Phase 8: Stress](09_phase8_stress.md)  
**Next**: [Phase 10: Politics](11_phase10_politics.md)
