# Phase 12: Emergence (The Civilization)

> **Behavior Goal**: All systems collide to create complex, emergent drama across generations.

---

## Dependencies

```
ALL PREVIOUS PHASES
└── Everything built so far
└── This phase adds final refinements and polish
```

---

## New Systems

### Psychology: Object Psychometry (Totems)
*Reference: [02_psychology.md](../02_psychology.md) Layer 8F*

- **Objects hold emotional resonance**
- **History log** - items track owners and critical events
- **Psychic imprint** - high-intensity memories stick to nearby items
- **Effects**:
  - Sword that killed 100 → intimidation bonus
  - Father's sword → sentimental value >> material value
  - Agent refuses to sell despite need
- **Totems** - objects become symbols of beliefs/values

### Psychology: Rigidity & Routines
*Reference: [02_psychology.md](../02_psychology.md) Layer 8G*

- **New trait: Rigidity (0-100)**
  - Derived from: low Openness + high Neuroticism + sensitivity gene
- **Routine mechanic**:
  - Agent checks if current action = yesterday's action at this time
  - Match → stress decreases (soothing)
  - Mismatch → stress increases proportional to rigidity
- **High rigidity agents** self-enforce strict schedules
- **Meltdown** if routines disrupted ("the chair was moved!")

### Psychology: Personality Projection (Boss Effect)
*Reference: [02_psychology.md](../02_psychology.md) Layer 8G*

- **High status agents reshape environment to match traits**
- **Mechanism**: dominant agents create rules matching their psychology
- **Example**: High rigidity boss creates strict contracts for everyone
  - Not for efficiency, but to lower own stress
- **Conflict**: Chaotic employee under rigid boss = mutual stress

### Society: Ethnicity (Founder Effect)
*Reference: [03_society.md](../03_society.md) Layer 8E*

- **Visual history of lineage** (not a "race" stat)
- **Drift**: isolated groups homogenize cosmetic traits
  - "The Crimson Folk" all have red eyes because founder did
- **Racism emerges**: pattern recognition error in belief system
  - "Red eyes = thieves" (false generalization)

### Society: Civilization Rise & Fall
*Reference: [03_society.md](../03_society.md) Historical Emergence*

- **No explicit civilization object**
- **Emerges from**: population density + shared beliefs + trade + tech level
- **Rises through**: resource management + knowledge accumulation + cooperation
- **Falls through**: resource depletion + internal conflict + external pressure + knowledge loss
- **Successor civilizations** can rediscover lost knowledge through ruins

### Society: Conflict Escalation
*Reference: [03_society.md](../03_society.md) Social Dynamics*

- **Personal grudge → family feud → tribal war**
- **Dehumanization** through belief formation
- **Revenge cycles** - violence creates trauma → desire for revenge
- **Peace** through relationship building or exhaustion

---

## Final Integration Tests

### "The 1000 Year Run"
1. Spawn 100+ agents
2. Run simulation for 1000 game-years
3. Civilizations should rise and fall
4. Knowledge should be gained and lost
5. Dynasties should form and end
6. No crashes, playable framerate

### "The Heirloom"
1. Agent starves but refuses to sell Father's Sword
2. Only sells when stress maxed OR father-belief weakened
3. New owner gains the sword's reputation

### "The Rigid Boss"
1. High-rigidity agent becomes faction leader
2. Creates strict schedules for everyone
3. Chaotic employees accumulate stress
4. Eventually rebel or leave

---

## Optimization (for scale)

*Reference: Implied by long-term simulation*

- **LOD (Level of Detail)** - distant agents use simplified AI
- **History culling** - forget old, low-importance events
- **Spatial culling** - only fully simulate agents near focus
- **Batch processing** - similar agents processed together
- **Lazy evaluation** - only compute when needed

---

## What's COMPLETE

At this point, all major systems from design docs are implemented:

| Design Doc | Coverage |
|------------|----------|
| 01_biology.md | ✅ Genetics, Phenotypes, Body Parts, Injuries, Lifecycle |
| 02_psychology.md | ✅ Personality, Drives, Beliefs, Memories, Skills, Goals, State, Mask, Stress, Willpower, Rigidity, Totems |
| 03_society.md | ✅ Relationships, Culture, Language, Family, Gossip, Factions, Status, Debt, Contracts, Politics, Economy, History |
| 04_engine.md | ✅ Subjective Reality, GOAP, Affordances, Actions, Events, Theory of Mind |
| 05_ui.md | ✅ Kingdom Map, Gossip Clouds, Social Graph |
| 06_world.md | ✅ Tiles, Elevation, Resources, Materials, Environment |

---

**Previous**: [Phase 11: Culture](12_phase11_culture.md)  
**Back to**: [Index](00_index.md)
