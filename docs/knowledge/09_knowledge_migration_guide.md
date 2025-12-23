# Knowledge System Migration Guide

This document tracks the migration from fragmented memory/knowledge systems to the unified MindGraph triple-store.

## Overview

**Goal:** Replace multiple separate components with a single unified knowledge representation that enables realistic belief formation, uncertainty-aware planning, and emergent learning.

---

## Component Migration Map

### BEFORE: Fragmented Systems

```
┌─────────────────────────────────────────────────────┐
│ CURRENT ARCHITECTURE (TO BE REMOVED)               │
├─────────────────────────────────────────────────────┤
│                                                     │
│  Memory Component (src/agent/memory.rs)            │
│  ├─ episodic: VecDeque<EpisodicMemory>            │
│  └─ methods: memories_about(), memories_with_tag() │
│                                                     │
│  WorkingMemory Component                            │
│  ├─ buffer: VecDeque<WorkingMemoryItem>           │
│  └─ Stores raw GameEvent references                │
│                                                     │
│  Beliefs Component (src/agent/beliefs.rs)          │
│  ├─ ??? (need to check implementation)             │
│  └─ Separate from episodic memory                  │
│                                                     │
│  WorldGraph Resource (src/agent/knowledge.rs)      │
│  ├─ triples: Vec<Triple>                           │
│  ├─ Stores ontology + entity facts                 │
│  └─ Query method                                   │
│                                                     │
│  Current MindGraph Component                        │
│  ├─ triples: Vec<Triple>                           │
│  ├─ No indexing (O(n) queries)                     │
│  ├─ No memory type distinction                     │
│  └─ Ontology copied per agent                      │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### AFTER: Unified System

```
┌─────────────────────────────────────────────────────┐
│ NEW ARCHITECTURE (UNIFIED)                          │
├─────────────────────────────────────────────────────┤
│                                                     │
│  Ontology Resource (shared via Arc)                │
│  ├─ Arc<Vec<Triple>>                               │
│  └─ Read-only, shared across all agents            │
│                                                     │
│  MindGraph Component (unified knowledge)           │
│  ├─ ontology: Arc<Vec<Triple>> (shared)           │
│  ├─ triples: Vec<Triple> (personal knowledge)     │
│  │   ├─ Episodic (Event nodes)                    │
│  │   ├─ Semantic (beliefs)                        │
│  │   ├─ Procedural (skills)                       │
│  │   └─ Perception (current observations)         │
│  ├─ index: MindGraphIndex (O(1) lookups)          │
│  │   ├─ by_subject: HashMap                       │
│  │   ├─ by_predicate: HashMap                     │
│  │   └─ by_memory_type: HashMap                   │
│  └─ Methods: query(), get(), assert(), etc.       │
│                                                     │
│  WorkingMemory Component (attention only)          │
│  ├─ focus: VecDeque<FocusItem>                    │
│  ├─ capacity: usize (~7)                          │
│  └─ Tracks what agent is attending to             │
│                                                     │
└─────────────────────────────────────────────────────┘
```

---

## File-by-File Migration

### 1. `src/agent/memory.rs` → DELETE (after migration)

**Current Contents:**
- `EpisodicMemory` struct
- `Memory` component with `VecDeque<EpisodicMemory>`
- `WorkingMemory` component with event buffer
- `process_perception()` system
- `process_working_memory()` system
- `decay_stale_knowledge()` system

**Migration:**
- [ ] EpisodicMemory → Event node triples in MindGraph
- [ ] Memory.episodic → MindGraph triples with MemoryType::Episodic
- [ ] WorkingMemory → Refactor to attention system (keep component, rewrite internals)
- [ ] process_perception() → Update to write Event triples
- [ ] process_working_memory() → Update to consolidation system
- [ ] decay_stale_knowledge() → Move to new decay system with MemoryType-based half-lives

**Delete After:**
- EpisodicMemory struct
- Memory component
- Old query methods (memories_about, memories_with_concept)

**Keep & Update:**
- WorkingMemory component (but rewrite as FocusItem system)

---

### 2. `src/agent/beliefs.rs` → DELETE (if exists)

**Need to check:** Does this file exist? What does it contain?

**Expected Migration:**
- [ ] Beliefs component → MindGraph triples with MemoryType::Semantic
- [ ] Belief queries → MindGraph.query() with semantic filter
- [ ] Belief formation → Consolidation system

**Delete:** Entire component if it exists separately from MindGraph.

---

### 3. `src/agent/knowledge.rs` → REFACTOR (major changes)

**Current:**
- WorldGraph resource
- MindGraph component (basic)
- setup_ontology() function
- copy_ontology_to_mind() function
- update_knowledge_from_perception() system

**Changes:**

#### Add New Types:
```rust
// Add to existing enums
pub enum MemoryType {
    Intrinsic,
    Cultural,
    Semantic,
    Episodic,
    Procedural,
    Perception,
}

// Add new predicates
pub enum Predicate {
    // ... existing ...
    Affords,
    Produces,
    Consumes,
    Satisfies,
    Requires,
    RegenerationRate,
    LastObserved,
    Actor,
    Action,
    Target,
    Result,
    FeltEmotion,
    Relationship,
    TrustsFor,
}

// Add to Value
pub enum Value {
    // ... existing ...
    Attitude(f32),
}
```

#### Update Metadata:
```rust
pub struct Metadata {
    pub source: Source,
    pub memory_type: MemoryType,  // NEW
    pub timestamp: u64,
    pub confidence: f32,
    pub informant: Option<Entity>,
    pub evidence: Vec<u64>,  // NEW - event IDs
    pub salience: f32,       // NEW - emotional significance
}
```

#### Add MindGraph Indexing:
```rust
#[derive(Default, Clone)]
pub struct MindGraphIndex {
    by_subject: HashMap<Node, Vec<usize>>,
    by_predicate: HashMap<Predicate, Vec<usize>>,
    by_subject_pred: HashMap<(Node, Predicate), usize>,
    by_memory_type: HashMap<MemoryType, Vec<usize>>,
}

impl MindGraph {
    pub fn rebuild_indices(&mut self) { ... }

    // Optimized query using indices
    pub fn get(&self, subject: &Node, predicate: Predicate) -> Option<&Value> { ... }
}
```

#### Add Shared Ontology:
```rust
#[derive(Resource)]
pub struct Ontology(pub Arc<Vec<Triple>>);

pub struct MindGraph {
    pub ontology: Arc<Vec<Triple>>,  // NEW - shared
    pub triples: Vec<Triple>,
    index: MindGraphIndex,           // NEW
}
```

#### Delete:
- [ ] WorldGraph resource and all methods
- [ ] Old copy_ontology_to_mind() - replace with Arc clone

---

### 4. `src/world/spawner.rs` → UPDATE

**Current:**
- spawn_person() copies ontology from WorldGraph
- No cultural knowledge differentiation

**Changes:**
- [ ] Use shared Ontology resource (Arc clone)
- [ ] Add cultural knowledge based on agent's culture
- [ ] Remove WorldGraph assertions

**Before:**
```rust
let mut mind = MindGraph::default();
copy_ontology_to_mind(&world_graph, &mut mind);
world_graph.assert_entity(entity, Predicate::IsA, Value::Concept(Concept::Person));
```

**After:**
```rust
let mind = MindGraph {
    ontology: ontology.0.clone(),  // Arc clone (cheap!)
    triples: create_cultural_knowledge(culture),
    index: MindGraphIndex::default(),
};
// No WorldGraph assertion - use ECS marker component instead
```

---

### 5. `src/agent/spawning.rs` → DELETE

**Status:** User says this file was deleted already. Confirm and remove references.

**Check for:**
- [ ] Any imports of this module
- [ ] Any systems registered from this module
- [ ] Old spawn_agent() calls

---

### 6. `src/cognition/perception.rs` → UPDATE

**Current:**
- update_knowledge_from_perception() queries WorldGraph

**Changes:**
- [ ] Remove WorldGraph query
- [ ] Query ECS marker components instead (Person, AppleTree, etc.)
- [ ] Continue writing perception triples to MindGraph

**Before:**
```rust
let facts = world_graph.query(Some(&Node::Entity(entity)), None, None);
for fact in facts {
    mind.assert(perceived_fact);
}
```

**After:**
```rust
// Check ECS components directly
if entities.get(entity).has::<Person>() {
    mind.assert(Triple::new(
        Node::Entity(entity),
        Predicate::IsA,
        Value::Concept(Concept::Person),
    ));
}
```

---

### 7. `src/agent/brains/rational.rs` → UPDATE

**Current:**
- Generates actions from hardcoded templates
- Checks Affordance component

**Changes:**
- [ ] Query MindGraph for affordances instead
- [ ] Generate actions dynamically from beliefs
- [ ] Use BeliefState for probabilistic preconditions

**Before:**
```rust
for entity in visible {
    if let Ok(affordance) = affordances.get(entity) {
        if affordance.action_type == Harvest {
            // generate action
        }
    }
}
```

**After:**
```rust
for entity in visible {
    let entity_types = mind.all_types(&Node::Entity(entity));
    for concept in entity_types {
        let afforded = mind.query(
            Some(&Node::Concept(concept)),
            Some(Predicate::Affords),
            None
        );
        for triple in afforded {
            // generate action from belief
        }
    }
}
```

---

### 8. `src/agent/brains/planner.rs` → UPDATE

**Current:**
- Binary WorldState (HashSet<Fact>)
- Deterministic planning

**Changes:**
- [ ] Add BeliefState struct (HashMap<Fact, f32>)
- [ ] Probabilistic precondition checking
- [ ] Expected value cost calculation

**New Code:**
```rust
pub struct BeliefState {
    pub beliefs: HashMap<Fact, f32>,
}

impl BeliefState {
    pub fn confidence(&self, fact: &Fact) -> f32 {
        *self.beliefs.get(fact).unwrap_or(&0.0)
    }
}

fn check_preconditions_prob(state: &BeliefState, preconditions: &[Fact]) -> f32 {
    preconditions.iter()
        .map(|pre| state.confidence(pre))
        .product()
}

fn effective_cost(action: &ActionTemplate, state: &BeliefState) -> f32 {
    let success_prob = check_preconditions_prob(state, &action.preconditions);
    if success_prob < 0.1 {
        return f32::INFINITY;
    }
    action.base_cost / success_prob
}
```

---

### 9. `src/agent/behavior.rs` → UPDATE

**Current:**
- perform_harvesting() and perform_eating() update inventories

**Changes:**
- [ ] After action completion, create Event triples in MindGraph
- [ ] Store actor, action, target, result, emotion, timestamp
- [ ] Update beliefs based on observations

**Add After Actions:**
```rust
// In perform_harvesting after success:
let event_id = generate_event_id();
mind.assert(Triple::new(
    Node::Event(event_id),
    Predicate::Action,
    Value::Action(ActionType::Harvest),
));
mind.assert(Triple::new(
    Node::Event(event_id),
    Predicate::Target,
    Value::Entity(tree),
));
mind.assert(Triple::new(
    Node::Event(event_id),
    Predicate::Result,
    Value::Item(Concept::Apple, amount),
));
// ... etc
```

---

## New Files to Create

### 1. `src/agent/consolidation.rs` (NEW)

**Purpose:** Pattern recognition and belief formation.

**Contains:**
- Pattern detection enums
- Weighted evidence calculation
- Bayesian confidence formulas
- Consolidation system

```rust
pub enum Pattern {
    RepeatedAction { actor: Entity, action: ActionType, count: usize, ... },
    ActionResult { action: ActionType, target_type: Concept, result: Concept, ... },
}

pub fn consolidate_memories(agents: Query<&mut MindGraph>, ...) { ... }
```

---

### 2. `src/agent/culture.rs` (NEW)

**Purpose:** Cultural knowledge definitions.

**Contains:**
- Culture enum/struct
- Knowledge set definitions
- create_cultural_knowledge() function

```rust
pub enum Culture {
    Farmer,
    Hunter,
    Nomad,
}

pub fn create_cultural_knowledge(culture: Culture) -> Vec<Triple> { ... }
```

---

### 3. `src/agent/brains/belief_state.rs` (NEW)

**Purpose:** Build probabilistic belief state for planning.

**Contains:**
- BeliefState struct
- build_belief_state() function
- Confidence estimation with regeneration
- Time decay calculations

```rust
pub fn build_belief_state(mind: &MindGraph, current_time: u64) -> BeliefState { ... }

pub fn estimate_confidence(
    mind: &MindGraph,
    entity: Entity,
    item: Concept,
    current_time: u64,
) -> f32 { ... }
```

---

### 4. `src/agent/exploration.rs` (NEW)

**Purpose:** Exploration mode and experimental actions.

**Contains:**
- PlanningMode enum
- select_planning_mode() function
- generate_exploration_actions()
- Learning from experiments

```rust
pub enum PlanningMode {
    Exploitation,
    Exploration,
}

pub fn generate_exploration_actions(
    mind: &MindGraph,
    visible: &VisibleObjects,
    goal: &Goal,
) -> Vec<ActionTemplate> { ... }
```

---

## Migration Checklist

### Phase 1: Foundation
- [ ] Add MemoryType, new Predicates, Value::Attitude to knowledge.rs
- [ ] Add MindGraphIndex struct and rebuild_indices()
- [ ] Optimize query methods to use indices
- [ ] Create Ontology resource with Arc
- [ ] Update MindGraph to reference shared ontology
- [ ] Update spawn_person to use shared ontology

### Phase 2: Episodic Migration
- [ ] Update behavior.rs to create Event triples on action completion
- [ ] Update process_perception to write Event triples
- [ ] Refactor WorkingMemory to FocusItem-based attention
- [ ] Verify old Memory component is no longer used
- [ ] Delete Memory component

### Phase 3: Consolidation
- [ ] Create consolidation.rs with pattern detection
- [ ] Implement weighted evidence calculation
- [ ] Implement Bayesian confidence formulas
- [ ] Add consolidation system to agent plugin
- [ ] Implement one-shot learning for trauma

### Phase 4: Cultural Knowledge
- [ ] Create culture.rs with Culture enum
- [ ] Define knowledge sets per culture
- [ ] Update spawner to assign cultures
- [ ] Add trust system for social transfer

### Phase 5: Probabilistic Planning
- [ ] Create belief_state.rs
- [ ] Implement build_belief_state with regeneration logic
- [ ] Update planner.rs with BeliefState
- [ ] Implement expected value cost calculation
- [ ] Add replan-on-failure system

### Phase 6: Exploration
- [ ] Create exploration.rs
- [ ] Implement exploration mode detection
- [ ] Generate experimental actions
- [ ] Add learning from experiment results

### Cleanup
- [ ] Delete old Memory component (src/agent/memory.rs - most of it)
- [ ] Delete Beliefs component if separate
- [ ] Delete WorldGraph resource and methods
- [ ] Remove copy_ontology_to_mind function
- [ ] Remove spawning.rs references (already deleted)
- [ ] Update all imports and references
- [ ] Remove Affordance component (replaced by knowledge)

### Performance
- [ ] Add staggered update systems (mod 10)
- [ ] Add memory decay system with MemoryType-based rates
- [ ] Profile before/after with 100 agents

---

## Testing Strategy

### Unit Tests
- [ ] Test MindGraph indexing (lookups return correct results)
- [ ] Test belief consolidation (patterns → beliefs)
- [ ] Test confidence calculation (weighted evidence)
- [ ] Test BeliefState building (regeneration math)
- [ ] Test expected value planning (cost calculation)

### Integration Tests
- [ ] Agent learns that tree produces apples (2-3 harvests)
- [ ] Agent learns Bob is hostile (3 attacks → belief)
- [ ] Agent plans to empty-but-regenerated tree
- [ ] Agent explores unknown environment
- [ ] Agent learns from failed experiment

### Performance Tests
- [ ] Benchmark query speed (indexed vs linear)
- [ ] Profile 100 agents with full knowledge system
- [ ] Verify <1ms per agent per frame

---

## Rollback Plan

If migration fails or introduces bugs:

1. **Git branches**: Create feature branch, keep main stable
2. **Feature flags**: Use Bevy conditions to toggle systems
3. **Parallel systems**: Keep old components alongside new temporarily
4. **Incremental**: Migrate one phase at a time, test each

---

## Success Criteria

✅ **Migration complete when:**
- [ ] No references to Memory component (except WorkingMemory)
- [ ] No references to Beliefs component
- [ ] No references to WorldGraph resource
- [ ] All old files deleted or refactored
- [ ] All tests passing
- [ ] Performance meets targets (<1ms/agent)
- [ ] Example scenarios working (food learning, Bob hostile, tree regen)

---

## Notes

- **Backward compatibility**: None. This is a breaking refactor.
- **Data migration**: No save files yet, so no migration needed.
- **Documentation**: Update CLAUDE.md and psychology docs after migration.
