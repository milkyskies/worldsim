# Vertical-Slice Roadmap (v2)

A behavior-milestone approach to building the simulation. Each phase adds a **new emergent behavior** by slicing vertically through multiple systems.

---

## Philosophy

**Old Roadmap**: Finish ALL of biology → ALL of psychology → ALL of cognition...  
**New Roadmap**: Build SIMPLE versions of everything → DEEPEN as behaviors require

This means:
- ✅ Emergent behavior appears EARLY
- ✅ Each phase is independently testable  
- ⚠️ Some systems are REVISITED (marked with 🔄 in each phase)

---

## Dependency Legend

Each phase header shows:
- **Requires**: Phases that MUST be done first
- **New Systems**: First introduction of these systems
- **Extends**: Systems from earlier phases that get deeper (🔄)

---

## Phase Overview

| Phase | Milestone | Key Behavior |
|-------|-----------|--------------|
| [0](01_phase0_foundation.md) | The Clock | Time passes |
| [1](02_phase1_survival.md) | The Survivor | Agent eats to not die |
| [2](03_phase2_personality.md) | The Individual | Agents act differently |
| [3](04_phase3_memory.md) | The Witness | Agents remember events |
| [4](05_phase4_bodies.md) | The Wounded | Injuries affect capability |
| [5](06_phase5_learning.md) | The Apprentice | Agents improve over time |
| [6](07_phase6_social.md) | The Friend | Agents form relationships |
| [7](08_phase7_family.md) | The Parent | Agents reproduce, genes matter |
| [8](09_phase8_stress.md) | The Breakdown | Agents crack under pressure |
| [9](10_phase9_economy.md) | The Trader | Agents exchange value |
| [10](11_phase10_politics.md) | The Warlord | Agents organize hierarchies |
| [11](12_phase11_culture.md) | The Tribe | Groups develop identity |
| [12](13_phase12_emergence.md) | The Civilization | Everything collides |

---

## Feature Coverage Checklist

Every feature from the design docs mapped to its phase:

### From 01_biology.md
- [x] Genetics (Layer 1) → Phase 7
- [x] Phenotypes (Layer 2) → Phase 7  
- [x] Body Parts (Layer 3A) → Phase 4
- [x] Injuries → Phase 4
- [x] Lifecycle Stages → Phase 7
- [x] Inventory & Capacity → Phase 1

### From 02_psychology.md
- [x] Personality Traits (Layer 3B) → Phase 2
- [x] Drive Strengths (Layer 3C) → Phase 2
- [x] Beliefs (Layer 4A) → Phase 3
- [x] Memories (Layer 4B) → Phase 3
- [x] Skills (Layer 4C) → Phase 5
- [x] Goals (Layer 4E) → Phase 2, extended Phase 5
- [x] State (Layer 5) → Phase 1, extended throughout
- [x] The Mask (Layer 8A) → Phase 8
- [x] Stress System (Layer 8B) → Phase 8
- [x] Object Psychometry (Layer 8F) → Phase 12
- [x] Willpower (Layer 8G) → Phase 8
- [x] Rigidity (Layer 8G) → Phase 12

### From 03_society.md
- [x] Relationships (Layer 4D) → Phase 6
- [x] Culture → Phase 11
- [x] Language → Phase 11
- [x] Reproduction & Family → Phase 7
- [x] Gossip → Phase 11
- [x] Faction Formation → Phase 10
- [x] Status Hierarchies → Phase 10
- [x] Debt System (Layer 8C) → Phase 9
- [x] Contracts (Layer 8C-2) → Phase 9
- [x] Information Fidelity (Layer 8D) → Phase 11
- [x] Politics (Layer 8E) → Phase 10
- [x] Economy (Layer 8E) → Phase 9

### From 04_engine.md
- [x] Subjective Reality (Layer 6A) → Phase 3
- [x] GOAP Planning (Layer 6B) → Phase 2
- [x] Affordances (Layer 7A) → Phase 1
- [x] Action Granularity (Layer 7B) → Phase 4
- [x] Outcome Prediction (Layer 7C) → Phase 5
- [x] Event Object (Layer 7D) → Phase 3
- [x] Theory of Mind (Layer 7E) → Phase 6

### From 05_ui.md
- [x] Kingdom Map (Layer 9A) → Phase 10
- [x] Gossip Cloud (Layer 9B) → Phase 11
- [x] Social Graph (Layer 9C) → Phase 6

### From 06_world.md
- [x] Tile Grid → Phase 0
- [x] Elevation & Slopes → Phase 4
- [x] Resources → Phase 1
- [x] Flora Growth → Phase 9
- [x] Material Properties → Phase 9
- [x] Water System → Phase 4
- [x] Temperature → Phase 4
- [x] Light System → Phase 1

---

## How to Read Each Phase

Each phase file contains:
1. **Behavior Goal** - What emergent behavior this enables
2. **Dependencies** - What phases must come before
3. **New Systems** - First-time implementations
4. **Extended Systems** (🔄) - Revisiting earlier systems
5. **Test Scenario** - Concrete proof the phase works
6. **Design Doc References** - Links back to design docs
