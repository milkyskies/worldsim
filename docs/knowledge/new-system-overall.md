```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           PERCEPTION → MEMORY → KNOWLEDGE                    │
└─────────────────────────────────────────────────────────────────────────────┘

  1. PERCEPTION
     ┌──────────────────┐
     │ Agent sees stuff │ → GameEvent (Wave, Attack, Harvest, etc.)
     └──────────────────┘
              │
              ▼
  2. WORKING MEMORY (src/agent/memory.rs)
     ┌─────────────────────────────────────────────┐
     │ WorkingMemory: Short-term event buffer      │
     │ - process_perception(): Routes events       │
     │ - process_working_memory(): → MindGraph     │
     └─────────────────────────────────────────────┘
              │ Converts events to triples:
              │ (Event123, Actor, Bob)
              │ (Event123, Action, Wave)
              │ (Event123, FeltEmotion, Joy)
              ▼
  3. MINDGRAPH (src/agent/knowledge.rs)
     ┌─────────────────────────────────────────────────────────────────┐
     │ MindGraph: The agent's entire knowledge as semantic triples     │
     │                                                                 │
     │ Triples: (Subject, Predicate, Object) + Metadata               │
     │                                                                 │
     │ Examples:                                                       │
     │   (Tree, Contains, Apple:5)         ← World state              │
     │   (Bob, HasTrait, Friendly)         ← Belief about Bob         │
     │   (Event123, FeltEmotion, Joy)      ← Episodic memory          │
     │   (Apple, IsA, Food)                ← Semantic knowledge       │
     │                                                                 │
     │ De-duplicates identical triples automatically!                  │
     └─────────────────────────────────────────────────────────────────┘
              │
              ▼
  4. CONSOLIDATION (src/agent/consolidation.rs)
     ┌─────────────────────────────────────────────────────────────────┐
     │ consolidate_knowledge: Forms beliefs from patterns              │
     │                                                                 │
     │ - Runs every 30 ticks, staggered by entity ID                   │
     │ - Scans episodic events (Bob attacked me 3x)                    │
     │ - Forms semantic beliefs (Bob → HasTrait → Hostile)             │
     │ - Weighted by recency (recent events matter more)               │
     └─────────────────────────────────────────────────────────────────┘
              │
              ▼
  5. BELIEF STATE (src/agent/brains/belief_state.rs)
     ┌─────────────────────────────────────────────────────────────────┐
     │ BeliefState: Translates MindGraph → Planner Facts               │
     │                                                                 │
     │ estimate_probability(Fact::HasItem(Apple)) → 0.0 to 1.0        │
     │                                                                 │
     │ - Reads MindGraph to answer "What does agent believe?"          │
     │ - Used by the Rational Brain to plan actions                    │
     └─────────────────────────────────────────────────────────────────┘
              │
              ▼
  6. PLANNING (src/agent/brains/rational.rs)
     ┌─────────────────────────────────────────────────────────────────┐
     │ Rational Brain uses BeliefState to make decisions               │
     │                                                                 │
     │ "Do I have food?" → BeliefState → MindGraph → "No"             │
     │ "Where is food?"  → MindGraph → "Saw apples at Tree(42)"       │
     │ "Plan: Walk to Tree, Harvest, Eat"                              │
     └─────────────────────────────────────────────────────────────────┘
```

Key Files:
File	Purpose
memory.rs
WorkingMemory
 buffer + perception processing
knowledge.rs
MindGraph
 - semantic triple store
consolidation.rs
Forms beliefs from event patterns
belief_state.rs	Translates MindGraph to planner facts
exploration.rs	Proposes exploration goals when bored/starving