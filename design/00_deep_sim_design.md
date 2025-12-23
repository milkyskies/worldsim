# Deep Simulation System Design

## Core Philosophy

A simulation where behavior emerges from layered systems rather than scripted responses. Everything from genes to civilization arises from interactions between lower-level systems.

**Key Design Principles:**
1.  **Nothing is scripted** - all behavior emerges from systems
2.  **Traits are calculated, not assigned** - genes + experiences → who you are
3.  **Memory is unreliable** - everyone remembers events differently; history is a consensus of lies.
4.  **Beliefs update slowly** - confirmation bias is real
5.  **Personality can change** - trauma, joy, and the "Snap" reshape people
6.  **Bodies matter** - chronic pain affects personality, injuries affect capability
7.  **Society emerges** - culture, factions, and history arise from individuals
8.  **Knowledge can be lost** - civilizations rise and fall based on what people remember
9.  **Everything feeds back** - outcomes create memories create beliefs create decisions create outcomes
10. **Structures are Hallucinations** - Kingdoms and Religions exist only because agents believe they do.

---

## System Architecture

The system is split into 5 interlocking domains:

### [1. BIOLOGY](design/01_biology.md) (The Body)
Physical systems that define "What the agent IS."
- **Layer 1: Genetics** (Genes → Phenotypes)
- **Layer 2: Phenotypes** (Stats derived from genes + environment)
- **Layer 3A: Body Parts** (Limbs, organs, injuries, health)

### [2. PSYCHOLOGY](design/02_psychology.md) (The Mind)
Systems that define "What the agent THINKS."
- **Personality** (Traits, Drives, The Mask)
- **Memory Systems**:
  - Working Memory (immediate awareness)
  - Episodic Memory (specific events)
  - Semantic Memory (beliefs/knowledge)
  - Procedural Memory (skills)
- **Cognition** (Goals, Emotional Associations)
- **State** (Stress, Needs, Object Psychometry)

### [3. SOCIETY](design/03_society.md) (The World)
Systems that emerge between agents.
- **Relationships** (Trust, Respect, Debt)
- **Emergent Systems** (Culture, Language, Family)
- **Macro-Structures** (Politics, Economy, Information Fidelity)

### [4. ENGINE](design/04_engine.md) (The Loop)
The code logic that runs the simulation.
- **Cognition Engine** (Subjective Reality, GOAP)
- **Physics of Interaction** (Affordances, Action Granularity, Outcome Prediction)

### [5. UI](design/05_ui.md) (The Lens)
How the player interacts with the data.
- **Dynamic Visualization** (Kingdom Maps, Gossip Clouds, Social Graphs)

### [6. WORLD](design/06_world.md) (The Stage)
The physical environment agents inhabit.
- **Terrain & Geography** (Tiles, Biomes, Elevation)
- **Resources & Materials** (Wood, Stone, Food, Crafting)
- **Structures** (Buildings, Construction)
- **Time & Environment** (Day/Night, Seasons, Weather)
- **Spatial Systems** (Vision, Hearing, Pathfinding)


---

## Architecture Flow

```
GENES (inherited)
  ↓
  ├─→ PHYSICAL PHENOTYPES (Body)
  │         ↓
  │   PHYSICAL CAPABILITIES
  │
  └─→ PSYCHOLOGICAL PHENOTYPES (Mind)
            ↓
      ┌─────┴─────┐
      ↓           ↓
  PERSONALITY   DRIVE STRENGTHS
      ↓           ↓
      ┌───────────┴────────────┬──────────┬──────────┐
      ↓                        ↓          ↓          ↓
   EPISODIC                 SEMANTIC  PROCEDURAL  RELATIONSHIPS
   MEMORY ──────────────→  MEMORY    MEMORY
   (events)                (beliefs)  (skills)
      ↓                        ↓          ↓          ↓
      └────────────────────────┴──────────┴──────────┘
                            ↓
                         GOALS
                            ↓
        DECISIONS (Subjective Reality)
             ↓
    ACTIONS (Affordance Checks)
             ↓
    OUTCOMES (Events) ──────────┐
      ↓                         │
    SOCIETY & CULTURE <─────────┘
      (Emergent Feedback)
```

## Minimum Viable Complexity / Roadmap
1. Personality → Decisions → Actions (like Rimworld but deeper)
2. Add memories that shape personality
3. Add genetic predispositions
4. Add body part tracking
5. Add skills and knowledge transmission
6. Add reproduction and family
7. Add culture and language