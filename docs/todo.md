# WorldSim TODO

## Memory System

### Remove Hardcoded Action→Emotion Mapping
**Location**: `src/agent/memory.rs:94-116`

**Problem**: Action categorization and emotional impact are hardcoded in a match statement, duplicating what's already in the ontology:
```rust
match action {
    ActionType::Wave | ActionType::Talk => concepts.push(Concept::SocialAction),
    // ... hardcoded mapping
}
let felt = if concepts.contains(&Concept::ViolentAction) {
    Some(EmotionType::Fear)  // hardcoded!
}
```

**Solution**:
1. Query the ontology for action category: `mind.query(Some(&Node::Action(action)), Some(Predicate::IsA), None)`
2. Add `TriggersEmotion` triples to ontology for action types:
   ```rust
   add(act(ActionType::Attack), TriggersEmotion, Value::Emotion(Fear, 0.8));
   add(act(ActionType::Wave), TriggersEmotion, Value::Emotion(Joy, 0.3));
   ```
3. Query ontology for emotional impact: `mind.query(Some(&Node::Action(action)), Some(Predicate::TriggersEmotion), None)`

This makes the system data-driven and extensible without code changes.

---

### Implement Memory Rehearsal
**Location**: `src/agent/knowledge.rs` or new `src/agent/memory_rehearsal.rs`

**Concept**: Memories that get accessed (queried by brains during decision-making) should be strengthened, simulating the psychological effect of rehearsal.

**Possible Implementation**:
1. Track "last accessed" timestamp on triples (or increment access count)
2. When a triple is returned from `query()`, update its timestamp or boost salience slightly
3. Frequently-accessed memories decay slower (effectively refreshed)

**Considerations**:
- Performance: Don't want query() to mutate on every call
- Could batch updates: track accessed triple IDs, update in a system
- Or: only rehearse during explicit "thinking" phases (brain planning)

---

### Perception Staleness for Non-Visible Entities
**Location**: `src/agent/memory.rs` decay system

**Problem**: We store `(Entity#X, LocatedAt, Tile)` for entities we've seen, but these beliefs persist forever even when the entity moves.

**Solution**:
- Track which entities are currently visible (from VisibleObjects)
- For perception triples about entities NOT in current VisibleObjects, accelerate decay
- Or: mark confidence as decreasing over time when not refreshed

---

## Performance

### Spatial Indexing for Perception
**Location**: `src/cognition/perception.rs:29-66`

**Problem**: O(n*m) all-pairs distance checks every frame.

**Solution**: Implement spatial partitioning (grid or quadtree) for O(log n) entity lookups.

---
