# Knowledge System Architecture

This document describes the unified knowledge and memory system for agents in the simulation.

## Overview

The knowledge system replaces multiple fragmented components (Memory, Beliefs, MindGraph) with a **unified triple-store** that models all agent knowledge. This enables:

- Realistic belief formation from experience
- Uncertainty-aware planning
- Social knowledge transfer
- Emergent learning and innovation

---

## Core Philosophy

### Agents Don't Know Truth - They Have Beliefs

```
ECS (Ground Truth)          MindGraph (Agent Belief)
─────────────────           ────────────────────────
Tree has 5 apples    →      "I think tree has 3 apples" (saw it 1 min ago)
Bob is at (10, 20)   →      "I believe Bob is at (5, 5)" (outdated)
Berries are poison   →      "I think berries are food" (wrong!)
```

Agents act on **beliefs**, not truth. Beliefs can be:
- Correct or incorrect
- Confident or uncertain
- Recent or outdated
- Personal or communicated

### Knowledge Has Provenance

Every piece of knowledge tracks:
- **Source**: How did I learn this?
- **Confidence**: How sure am I?
- **Timestamp**: When did I learn this?
- **Evidence**: What experiences support this?

---

## Triple Store Architecture

All knowledge is stored as **triples**: `(Subject, Predicate, Object)`

```rust
pub struct Triple {
    pub subject: Node,
    pub predicate: Predicate,
    pub object: Value,
    pub meta: Metadata,
}
```

### Node Types (Subjects)

```rust
pub enum Node {
    Entity(Entity),              // Specific thing: Tree42, Bob
    Concept(Concept),            // Abstract: Food, Danger, AppleTree
    Tile((i32, i32)),           // Location
    Event(u64),                  // Remembered event
    Self_,                       // The agent's self-reference
    Action(ActionType),          // An action type
}
```

### Predicates (Relationships)

```rust
pub enum Predicate {
    // ─── Classification ───
    IsA,                // (Apple, IsA, Food)
    HasTrait,           // (Wolf, HasTrait, Dangerous)

    // ─── Spatial ───
    LocatedAt,          // (Tree42, LocatedAt, Tile(5,3))
    Contains,           // (Tree42, Contains, Apple(3))

    // ─── Action Semantics ───
    Affords,            // (AppleTree, Affords, Harvest)
    Produces,           // (AppleTree, Produces, Apple)
    Consumes,           // (Eat, Consumes, Food)
    Satisfies,          // (Eat, Satisfies, Hunger)
    Requires,           // (Harvest, Requires, AtLocation)

    // ─── Temporal ───
    RegenerationRate,   // (AppleTree, RegenerationRate, 10.0)
    LastObserved,       // (Tree42, LastObserved, 50000)

    // ─── Episodic Memory ───
    Actor,              // (Event42, Actor, Bob)
    Action,             // (Event42, Action, Attack)
    Target,             // (Event42, Target, Self_)
    Result,             // (Event42, Result, Damaged)
    Timestamp,          // (Event42, Timestamp, 1000)
    FeltEmotion,        // (Event42, FeltEmotion, Fear(0.8))

    // ─── Social ───
    Relationship,       // (Self_, Relationship, Bob) → Attitude(0.7)
    TrustsFor,          // (Bob, TrustsFor, FoodKnowledge)

    // ─── Emotional ───
    TriggersEmotion,    // (Wolf, TriggersEmotion, Fear(0.6))
}
```

### Values (Objects)

```rust
pub enum Value {
    Int(i32),
    Float(f32),
    Concept(Concept),
    Entity(Entity),
    Tile((i32, i32)),
    Action(ActionType),
    Emotion(EmotionType, f32),
    Item(Concept, u32),           // (Apple, 5) - quantity
    Attitude(f32),                // -1.0 to 1.0 (hate to love)
}
```

---

## Memory Types

Knowledge is categorized by **MemoryType**, which determines decay rate and query priority.

```rust
pub enum MemoryType {
    /// Universal truths (laws of physics, logic)
    /// Never decays. Shared across all agents.
    Intrinsic,

    /// Knowledge taught by culture/parents
    /// Slow decay. Agents start with this.
    Cultural,

    /// Personal beliefs inferred from experience
    /// Medium decay. "Bob is hostile", "Trees have apples"
    Semantic,

    /// Specific remembered events
    /// Faster decay (unless emotionally significant)
    Episodic,

    /// Skills and procedural knowledge
    /// Very slow decay. "I can forge swords"
    Procedural,

    /// Current perceptions
    /// Instant decay when out of sight
    Perception,
}
```

### Decay Rates

| Memory Type | Half-Life | Notes |
|-------------|-----------|-------|
| Intrinsic | ∞ | Never forgets |
| Cultural | 1 hour | Core cultural knowledge persists |
| Semantic | 5 minutes | Beliefs fade without reinforcement |
| Episodic | 1 minute | Mundane events forgotten quickly |
| Episodic (intense) | 10 minutes | Emotional events last longer |
| Procedural | 30 minutes | Skills decay slowly |
| Perception | 1 second | Only while visible |

---

## Metadata Structure

Every triple carries metadata about its provenance:

```rust
pub struct Metadata {
    /// How did I learn this?
    pub source: Source,

    /// What kind of memory is this?
    pub memory_type: MemoryType,

    /// When did I learn this? (game time in ms)
    pub timestamp: u64,

    /// How confident am I? (0.0 to 1.0)
    pub confidence: f32,

    /// Who told me? (for communicated knowledge)
    pub informant: Option<Entity>,

    /// What events support this belief?
    pub evidence: Vec<u64>,

    /// How emotionally significant? (affects decay)
    pub salience: f32,
}

pub enum Source {
    Intrinsic,      // Laws of the universe
    Cultural,       // Taught by society
    Communicated,   // Someone told me
    Observed,       // I saw it happen (to others)
    Experienced,    // I did it / it happened to me
    Inferred,       // I deduced from patterns
}
```

---

## Unified MindGraph Component

```rust
#[derive(Component)]
pub struct MindGraph {
    /// Shared universal truths (read-only, Arc for cheap clone)
    pub ontology: Arc<Vec<Triple>>,

    /// All personal knowledge
    pub triples: Vec<Triple>,

    /// Indices for fast lookup
    index: MindGraphIndex,
}

struct MindGraphIndex {
    by_subject: HashMap<Node, Vec<usize>>,
    by_predicate: HashMap<Predicate, Vec<usize>>,
    by_memory_type: HashMap<MemoryType, Vec<usize>>,
}
```

### Working Memory (Separate Component)

Working memory is **attention**, not storage. It tracks what the agent is currently thinking about.

```rust
#[derive(Component)]
pub struct WorkingMemory {
    /// What I'm attending to (limited capacity ~7 items)
    pub focus: VecDeque<FocusItem>,
    pub capacity: usize,
}

pub struct FocusItem {
    pub target: Node,
    pub salience: f32,
    pub entered_at: u64,
}
```

---

## Knowledge Flow

```
                    ┌─────────────────┐
                    │   WORLD (ECS)   │
                    │ Ground Truth    │
                    └────────┬────────┘
                             │
                             ▼
              ┌──────────────────────────┐
              │   PERCEPTION SYSTEM      │
              │ ECS → Perception Triples │
              └──────────────┬───────────┘
                             │
                             ▼
              ┌──────────────────────────┐
              │   ACTION EXECUTION       │
              │ Results → Episodic       │
              └──────────────┬───────────┘
                             │
                             ▼
              ┌──────────────────────────┐
              │   CONSOLIDATION          │
              │ Episodic → Semantic      │
              │ (Pattern Recognition)    │
              └──────────────┬───────────┘
                             │
                             ▼
              ┌──────────────────────────┐
              │   PLANNING (GOAP)        │
              │ Query Semantic + Cultural│
              │ Build Probabilistic Plan │
              └──────────────────────────┘
```

---

## Belief Formation

Beliefs are NOT hardcoded. They emerge from experience through **consolidation**.

### The Process

1. **Event Occurs**: Bob attacks agent
2. **Episodic Memory**: Store event triples
   ```
   (Event42, Actor, Bob)
   (Event42, Action, Attack)
   (Event42, Target, Self_)
   (Event42, FeltEmotion, Fear(0.9))
   ```
3. **Pattern Recognition**: Consolidation system runs periodically
   - "Bob has attacked me 3 times"
   - "Each time I felt fear"
4. **Belief Formed**: Semantic triple created
   ```
   (Bob, HasTrait, Hostile)
   confidence: 0.85
   evidence: [Event42, Event43, Event44]
   source: Inferred
   ```

### Confidence Calculation

Belief confidence is NOT based on simple counts. It uses weighted evidence:

```rust
confidence = f(
    supporting_evidence_weight,
    contradicting_evidence_weight,
    emotional_intensity,
    recency,
    source_trust
)
```

**Event Weight Formula:**
```
weight = (0.2 + intensity * 0.8) × (0.3 + recency * 0.7)

Where:
  intensity = emotional intensity of event (0-1)
  recency = (0.5)^(age / half_life)
```

**Examples:**

| Scenario | Weight | Resulting Confidence |
|----------|--------|---------------------|
| 1 traumatic attack (intensity=0.95, recent) | 0.96 | ~74% "hostile" |
| 3 mild annoyances (intensity=0.3 each) | 0.45 × 3 | ~70% "annoying" |
| 1 attack + 2 gifts | 0.92 vs 0.88 | ~51% uncertain |

### One-Shot Learning

Traumatic events can form beliefs immediately:

```rust
if event.emotional_intensity > 0.8 {
    // Skip waiting for patterns
    // Immediately form belief with high confidence
    form_belief_from_single_event(event);
}
```

---

## Cultural Knowledge

Agents don't start blank. They inherit knowledge from their culture.

### What Cultures Provide

```rust
// All cultures know:
(Eat, Satisfies, Hunger)
(Sleep, Satisfies, Fatigue)
(Apple, IsA, Food)

// Farmer culture also knows:
(AppleTree, Produces, Apple)
(AppleTree, RegenerationRate, 10.0)
(WheatField, Produces, Wheat)

// Hunter culture knows instead:
(Deer, Produces, Meat)
(Wolf, HasTrait, Dangerous)
// Does NOT know about farming!
```

### Cultural Knowledge Properties

- Source: `Source::Cultural`
- Confidence: 0.7-0.8 (trusted but not absolute)
- Can be overridden by personal experience
- Decays slowly if never reinforced

---

## Social Knowledge Transfer

Agents can share knowledge through communication.

### Sharing Process

```rust
// Alice tells Bob: "The big tree has apples"
bob.mind.assert(Triple {
    subject: Node::Entity(big_tree),
    predicate: Predicate::Contains,
    object: Value::Item(Apple, 5),
    meta: Metadata {
        source: Source::Communicated,
        informant: Some(alice),
        confidence: bob.trust_for(alice, FoodKnowledge) * 0.8,
        ..
    }
});
```

### Trust Modifies Confidence

```
received_confidence = source_confidence × trust(informant, domain)

Examples:
- Trusted friend tells me about food: 0.8 × 0.9 = 0.72 confidence
- Stranger tells me about danger: 0.8 × 0.3 = 0.24 confidence
- Known liar tells me anything: 0.8 × 0.1 = 0.08 confidence
```

### Gossip and False Beliefs

Low-trust information can still be stored, but with low confidence. If multiple sources say the same thing, confidence increases:

```
P(belief) = 1 - ∏(1 - P(source_i))

3 strangers say "Bob is dangerous":
P = 1 - (1-0.24)³ = 1 - 0.44 = 0.56 (now more believable!)
```

---

## Uncertainty and Planning

The planning system operates on **probabilistic beliefs**, not binary facts.

### BeliefState

```rust
pub struct BeliefState {
    /// Each fact has a probability
    pub beliefs: HashMap<Fact, f32>,
}
```

### Building BeliefState from MindGraph

```rust
fn build_belief_state(mind: &MindGraph, current_time: u64) -> BeliefState {
    // For each relevant belief:
    // 1. Get base confidence from triple
    // 2. Apply time decay (old observations less reliable)
    // 3. Apply regeneration knowledge (empty tree might have regrown)

    // Example: Tree observed empty 2 minutes ago
    let age = current_time - observation_time;
    let regen_rate = mind.get(tree, RegenerationRate);
    let expected_items = age / regen_rate;
    let p_has_items = 1.0 - (-expected_items).exp();
}
```

### Expected Value Planning

GOAP considers probability of success:

```
Expected Cost = Base Cost / P(Success)

Example - Two trees:
  Tree A: 85% likely has apples, distance 10
    Cost = 15 / 0.85 = 17.6

  Tree B: 70% likely has apples, distance 5
    Cost = 10 / 0.70 = 14.3 ← Better choice!
```

### Replanning on Failure

When an action fails, beliefs are updated and agent replans:

```rust
// Plan: Harvest Tree A
// Reality: Tree A is empty!

// Update belief:
mind.assert((TreeA, Contains, Apple(0)), confidence=1.0, timestamp=now);

// Replan with new information
let new_plan = goap_plan(updated_belief_state, goal);
```

---

## Innovation and Exploration

When agents have no known solution, they can **experiment**.

### Exploration Mode Triggers

```rust
if goal.unsatisfied && !known_path_exists {
    let explore_chance = desperation * 0.5 + curiosity * 0.5;
    if random() < explore_chance {
        mode = PlanningMode::Exploration;
    }
}
```

### Experimental Actions

In exploration mode, agents generate "try X" actions for unknown objects:

```rust
// See unknown plant
generate_actions = [
    "Examine plant" (learn type),
    "Try harvest plant" (might get something),
    "Try eat plant" (risky but might work),
]
```

### Learning from Experiments

```
Success: "I harvested unknown tree, got coconut!"
  → Store event
  → Partial belief: (Tree, Produces, Coconut) conf=0.5
  → Try eating coconut...

Success: "Eating coconut reduced hunger!"
  → Belief: (Coconut, IsA, Food) conf=0.7
  → Strengthen: (Tree, Produces, Coconut) conf=0.7

Failure: "Eating red berry made me sick!"
  → Traumatic event (intensity=0.8)
  → ONE-SHOT belief: (RedBerry, HasTrait, Poisonous) conf=0.9
  → Won't try again!
```

---

## Comparison to Other Games

### Dwarf Fortress
- Complex needs and personality
- Memories affect mood
- NO unified knowledge graph
- NO probabilistic planning
- NO social knowledge transfer

### RimWorld
- Simplified DF model
- Traits affect behavior
- NO learning from experience
- NO belief formation

### The Sims
- Needs-based behavior
- Relationships
- NO episodic memory
- NO inference or learning

### This System
- Unified knowledge representation
- Probabilistic beliefs with provenance
- Learning from experience (consolidation)
- Cultural and social knowledge transfer
- Uncertainty-aware planning
- Innovation through exploration

**To our knowledge, no shipped game has implemented all of these together.**

---

## Implementation Phases

### Phase 1: Refactor MindGraph
- Add MemoryType to Metadata
- Add new predicates
- Add indexing
- Remove separate Memory/Beliefs components

### Phase 2: Episodic Memory as Triples
- Events create Event node triples
- Store actor, action, target, result, emotion
- Link to working memory attention

### Phase 3: Consolidation System
- Pattern recognition in episodic memory
- Belief formation with weighted evidence
- One-shot learning for trauma

### Phase 4: Cultural Knowledge
- Define culture knowledge sets
- Spawn agents with cultural knowledge
- Trust system for knowledge transfer

### Phase 5: Probabilistic Planning
- BeliefState from MindGraph
- Expected value in GOAP
- Replan on failure

### Phase 6: Exploration/Innovation
- Detect "stuck" states
- Generate experimental actions
- Learn from results

---

## Example Scenarios

### Scenario: Finding Food on Unknown Island

```
1. Agent spawns, hungry
2. Has cultural knowledge: (Eat, Satisfies, Hunger), (Food, IsA, Edible)
3. Does NOT know what local plants are food
4. Sees: Palm tree, bush, rocks
5. No known path to food → Exploration mode
6. Tries: Examine palm tree
   → Learns: (palm123, IsA, CoconutPalm)
7. Tries: Harvest palm tree
   → Gets coconut! Event stored.
   → Partial belief: (CoconutPalm, Produces, Coconut)
8. Has coconut, doesn't know if edible
9. Desperate (hunger=80) → Tries: Eat coconut
   → Success! Hunger reduced.
   → Event stored with Joy emotion
   → Belief formed: (Coconut, IsA, Food)
10. Next time hungry: KNOWS coconuts are food, palms have coconuts
    → Plans directly: Find palm → Harvest → Eat
```

### Scenario: Learning That Bob is Hostile

```
1. Bob attacks agent (intensity=0.7)
   → Event stored: (Event1, Actor, Bob), (Event1, Action, Attack), ...
   → Felt: Fear(0.7)
   → Single event weight: 0.76
   → Belief: (Bob, HasTrait, Hostile) conf=0.62

2. Bob attacks again (intensity=0.8)
   → Event stored
   → Combined weight: 0.76 + 0.84 = 1.6
   → Belief updated: conf=0.79

3. Bob gives gift (intensity=0.4)
   → Event stored: positive
   → Contradicting weight: 0.52
   → Belief reduced: conf=0.68 (more uncertain now)

4. Agent's behavior:
   → conf > 0.6: Avoids Bob, defensive posture
   → conf > 0.8: Flees on sight
   → conf < 0.5: Uncertain, watches carefully
```

### Scenario: Tree Regeneration Knowledge

```
1. Agent harvests tree, gets 3 apples
   → Belief: (Tree42, Contains, Apple(3))

2. Returns 1 minute later, tree empty
   → Belief updated: (Tree42, Contains, Apple(0))

3. Returns 2 minutes later, tree has 2 apples!
   → Event: Expected 0, found 2
   → Inference triggered: "Trees regrow apples"

4. Pattern recognized after 2-3 observations:
   → Belief: (AppleTree, RegenerationRate, ~10.0)

5. Now when planning:
   → Sees tree was empty 2 min ago
   → Knows regen rate
   → Calculates: P(has apples) = 1 - e^(-12) ≈ 0.99
   → Confidently goes to "empty" tree
```
