# Performance Optimizations for Knowledge System

This document describes performance strategies for the unified MindGraph system with 100-1000+ agents.

---

## Performance Targets

| Metric | Target | Why |
|--------|--------|-----|
| Frame budget (60fps) | 16.6ms | Smooth gameplay |
| Knowledge queries per agent | <1ms | 100 agents = 100ms total |
| Memory per agent | <10KB | 1000 agents = 10MB total |
| Consolidation cost | <0.1ms/agent | Staggered, not every frame |
| Planning cost | <2ms/agent | Staggered, not every frame |

---

## Optimization 1: HashMap Indexing

### Problem
Current MindGraph queries are O(n) - scan all triples every time:

```rust
pub fn query(&self, subject: Option<&Node>, ...) -> Vec<&Triple> {
    self.triples.iter().filter(...).collect()  // O(n) scan
}
```

**Cost per query**: 100 triples × 7ns = 700ns
**Cost per agent**: 12 queries × 700ns = 8.4μs
**Cost for 100 agents**: 840μs (0.84ms) ✓ acceptable but could be better
**Cost for 1000 agents**: 8.4ms ✗ too expensive!

### Solution: Indexed Lookups

```rust
pub struct MindGraph {
    triples: Vec<Triple>,

    // Indices for O(1) lookup
    by_subject: HashMap<Node, Vec<usize>>,
    by_predicate: HashMap<Predicate, Vec<usize>>,
    by_subject_pred: HashMap<(Node, Predicate), usize>,
    by_memory_type: HashMap<MemoryType, Vec<usize>>,
}

impl MindGraph {
    /// O(1) lookup for functional predicates
    pub fn get(&self, subject: &Node, predicate: Predicate) -> Option<&Value> {
        self.by_subject_pred.get(&(*subject, predicate))
            .map(|&idx| &self.triples[idx].object)
    }

    /// O(1) + small iteration for multi-valued predicates
    pub fn query_subject(&self, subject: &Node) -> impl Iterator<Item = &Triple> {
        self.by_subject.get(subject)
            .map(|indices| indices.iter().map(|&i| &self.triples[i]))
            .into_iter()
            .flatten()
    }
}
```

**New cost per query**: 1 hash lookup (30ns) + ~5 comparisons (35ns) = 65ns
**Cost per agent**: 12 queries × 65ns = 780ns
**Cost for 100 agents**: 78μs (0.078ms) ✓✓ great!
**Cost for 1000 agents**: 780μs (0.78ms) ✓ acceptable!

**Index maintenance cost**:
- Insert: 1 vec push + 3-4 hashmap inserts = ~200ns (negligible)
- Rebuild: Only when bulk removing triples (decay)

### Implementation

```rust
impl MindGraph {
    pub fn assert(&mut self, triple: Triple) {
        let idx = self.triples.len();

        // Remove old triple if functional predicate
        if is_functional(triple.predicate) {
            if let Some(&old_idx) = self.by_subject_pred.get(&(triple.subject.clone(), triple.predicate)) {
                self.remove_from_indices(old_idx);
                self.triples.swap_remove(old_idx);
                // Update indices if swapped
                if old_idx < self.triples.len() {
                    self.update_indices_after_swap(old_idx);
                }
            }
        }

        // Add new triple
        self.triples.push(triple.clone());
        self.add_to_indices(idx, &triple);
    }

    fn add_to_indices(&mut self, idx: usize, triple: &Triple) {
        self.by_subject.entry(triple.subject.clone())
            .or_default()
            .push(idx);

        self.by_predicate.entry(triple.predicate)
            .or_default()
            .push(idx);

        if is_functional(triple.predicate) {
            self.by_subject_pred.insert((triple.subject.clone(), triple.predicate), idx);
        }

        self.by_memory_type.entry(triple.meta.memory_type)
            .or_default()
            .push(idx);
    }

    pub fn rebuild_indices(&mut self) {
        self.by_subject.clear();
        self.by_predicate.clear();
        self.by_subject_pred.clear();
        self.by_memory_type.clear();

        for (idx, triple) in self.triples.iter().enumerate() {
            self.add_to_indices(idx, triple);
        }
    }
}
```

---

## Optimization 2: Shared Ontology (Arc)

### Problem
Each agent copies the entire ontology (50+ triples):

```rust
// OLD: Copy 50 triples per agent
let mut mind = MindGraph::default();
copy_ontology_to_mind(&world_graph, &mut mind);
// 50 triples × 88 bytes = 4.4 KB per agent
// 1000 agents = 4.4 MB wasted!
```

### Solution: Arc for Zero-Copy Sharing

```rust
#[derive(Resource)]
pub struct Ontology(pub Arc<Vec<Triple>>);

pub struct MindGraph {
    pub ontology: Arc<Vec<Triple>>,  // Shared!
    pub triples: Vec<Triple>,         // Personal only
}

// Arc clone is just pointer + refcount increment
let mind = MindGraph {
    ontology: ontology.0.clone(),  // ~16 bytes, not 4.4 KB!
    triples: Vec::new(),
};
```

**Memory saved**:
- Before: 50 triples × 88 bytes × 1000 agents = 4.4 MB
- After: 50 triples × 88 bytes × 1 + 16 bytes × 1000 agents = 4.4 KB + 16 KB = 20.4 KB
- **Saved: 4.38 MB** (99.5% reduction!)

**Query changes**:
```rust
pub fn query(&self, ...) -> Vec<&Triple> {
    // Search both ontology and personal knowledge
    self.ontology.iter()
        .chain(self.triples.iter())
        .filter(...)
        .collect()
}
```

---

## Optimization 3: Staggered Updates

### Problem
Heavy operations run every frame for every agent:

```rust
fn consolidate_memories(agents: Query<&mut MindGraph>) {
    for mut mind in agents.iter_mut() {
        // Pattern recognition: 50μs per agent
        // 1000 agents × 50μs = 50ms ✗ 3 frames!
    }
}
```

### Solution: Process 10% Per Frame

```rust
fn consolidate_memories(
    mut agents: Query<(Entity, &mut MindGraph), With<Person>>,
    tick: Res<TickCount>,
) {
    for (entity, mut mind) in agents.iter_mut() {
        // Only process if entity.index() mod 10 == tick mod 10
        if entity.index() % 10 != (tick.0 % 10) as u32 {
            continue;
        }

        // Heavy consolidation here
        find_patterns(&mind);
        form_beliefs(&mut mind);
    }
}
```

**Result**:
- Each agent consolidates every 10 frames (every 167ms at 60fps)
- Cost per frame: 1000 agents / 10 × 50μs = 5ms ✓
- Imperceptible to player!

**Apply to**:
- Consolidation (pattern recognition)
- GOAP planning (new plans)
- Memory decay
- Knowledge inference

**Keep every frame**:
- Perception (must be real-time)
- Action execution
- Quick queries (indexed, cheap)

---

## Optimization 4: Memory Decay by Type

### Problem
All memories decay at same rate, or we scan everything:

```rust
fn decay_all(mind: &mut MindGraph, current_time: u64) {
    for triple in &mut mind.triples {
        // Check age, calculate decay...
        // O(n) scan every frame!
    }
}
```

### Solution: Type-Based Decay Schedules

```rust
fn decay_memories(
    mut agents: Query<&mut MindGraph>,
    time: Res<Time>,
) {
    let now = time.elapsed().as_millis() as u64;

    for mut mind in agents.iter_mut() {
        // Only decay episodic/perception (fast decay types)
        // Skip intrinsic/cultural/semantic (slow/never decay)

        let to_remove: Vec<usize> = mind.triples.iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let half_life = match t.meta.memory_type {
                    MemoryType::Intrinsic => return None,     // Never decay
                    MemoryType::Cultural => 3600_000.0,        // 1 hour
                    MemoryType::Semantic => 300_000.0,         // 5 min
                    MemoryType::Procedural => 1800_000.0,      // 30 min
                    MemoryType::Episodic => {
                        // Emotional memories last longer
                        if t.meta.salience > 0.8 {
                            600_000.0  // 10 min for intense
                        } else {
                            60_000.0   // 1 min for mundane
                        }
                    }
                    MemoryType::Perception => 1_000.0,         // 1 sec
                };

                let age = now.saturating_sub(t.meta.timestamp) as f32;
                let decay = (0.5_f32).powf(age / half_life);

                // Remove if decayed below threshold
                if decay < 0.01 {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Remove in reverse order to preserve indices
        for i in to_remove.into_iter().rev() {
            mind.triples.swap_remove(i);
        }

        // Rebuild indices after bulk removal
        if mind.triples.len() < mind.by_subject.values().map(|v| v.len()).sum() {
            mind.rebuild_indices();
        }
    }
}
```

**Run frequency**: Every 60 frames (once per second)
**Cost**: ~10μs per agent (scan episodic only, ~30 triples)
**Total**: 1000 agents × 10μs = 10ms (but only once per second!)

---

## Optimization 5: Lazy BeliefState Building

### Problem
Building full belief state for planning is expensive:

```rust
fn plan(mind: &MindGraph) {
    // Build entire belief state upfront
    let belief_state = build_belief_state(mind, now);  // 50μs

    // But might only query a few facts during planning
    let plan = goap_plan(belief_state, goal);
}
```

### Solution: Lazy Evaluation

```rust
pub struct LazyBeliefState<'a> {
    mind: &'a MindGraph,
    current_time: u64,
    cache: RefCell<HashMap<Fact, f32>>,
}

impl<'a> LazyBeliefState<'a> {
    pub fn confidence(&self, fact: &Fact) -> f32 {
        // Check cache first
        if let Some(&conf) = self.cache.borrow().get(fact) {
            return conf;
        }

        // Compute on demand
        let conf = compute_confidence(self.mind, fact, self.current_time);
        self.cache.borrow_mut().insert(fact.clone(), conf);
        conf
    }
}
```

**Before**: Compute all 50 possible facts = 50 × 1μs = 50μs
**After**: Compute only 3-5 facts needed for plan = 5 × 1μs = 5μs
**Speedup**: 10x for planning!

---

## Optimization 6: Parallel Queries with Rayon

### Problem
Processing many agents sequentially:

```rust
for mut mind in agents.iter_mut() {
    // Process one by one
}
```

### Solution: Parallel Processing

```rust
use rayon::prelude::*;

fn consolidate_system(
    mut agents: Query<&mut MindGraph>,
) {
    // Bevy supports parallel queries
    agents.par_iter_mut().for_each(|mut mind| {
        // Runs on multiple threads!
        consolidate_memories(&mut mind);
    });
}
```

**Benefit**: 4-core CPU can process 4 agents simultaneously
**Speedup**: Near-linear with core count (with staggering, less critical)

**Caution**: Parallel writes need synchronization. Use for:
- ✓ Read-heavy: query, consolidation, planning
- ✗ Write-heavy: perception updates (potential conflicts)

---

## Memory Layout Optimization (Future)

### Arena Allocation
For even better cache locality:

```rust
use bumpalo::Bump;

pub struct MindGraph {
    arena: Bump,
    triples: Vec<&'arena Triple>,  // Pointers into arena
}
```

**Benefits**:
- Cache-friendly iteration (triples contiguous in memory)
- Fast allocation (bump pointer, no allocator overhead)
- Fast deallocation (reset arena in one go)

**Tradeoff**: More complex lifetime management

---

## Benchmarking Strategy

### Test Scenarios

1. **Query Performance**
   ```rust
   #[bench]
   fn bench_query_indexed(b: &mut Bencher) {
       let mind = create_mind_with_100_triples();
       b.iter(|| {
           mind.get(&Node::Self_, Predicate::Hunger)
       });
   }
   // Target: <100ns per query
   ```

2. **Consolidation Cost**
   ```rust
   #[bench]
   fn bench_consolidation(b: &mut Bencher) {
       let mut mind = create_mind_with_30_events();
       b.iter(|| {
           consolidate_memories(&mut mind)
       });
   }
   // Target: <50μs per agent
   ```

3. **Full Frame Budget**
   ```rust
   #[test]
   fn test_100_agents_frame_budget() {
       let mut world = create_world_with_100_agents();

       let start = Instant::now();

       // Run all systems
       perception_system();
       query_system();
       planning_system();  // Staggered
       consolidation_system();  // Staggered
       decay_system();

       let elapsed = start.elapsed();

       assert!(elapsed < Duration::from_micros(1000)); // <1ms
   }
   ```

### Profile Tools

```bash
# CPU profiling
cargo build --release
cargo flamegraph --bin worldsim

# Memory profiling
cargo install cargo-instruments
cargo instruments --release --template allocations

# Real-time inspection
cargo install cargo-profdata
# Use bevy_egui_inspector to see live stats
```

---

## Performance Checklist

Before merging knowledge system:

- [ ] All queries use indices (no linear scans)
- [ ] Ontology shared via Arc
- [ ] Heavy systems staggered (mod 10)
- [ ] Memory decay type-aware
- [ ] BeliefState lazy evaluation
- [ ] Parallel iteration where safe
- [ ] Benchmarks: <1ms per 100 agents
- [ ] Profile with 1000 agents: <60fps drop

---

## Expected Performance Profile

```
Frame time breakdown (1000 agents, 60fps):

Perception (every frame):           2ms
  └─ Vision + knowledge updates

Knowledge queries (indexed):      0.8ms
  └─ 1000 agents × 12 queries × 65ns

Planning (staggered, 100/frame):  1.5ms
  └─ 100 agents × 15μs GOAP

Consolidation (staggered, 100/frame): 0.5ms
  └─ 100 agents × 5μs pattern recognition

Decay (once per second):         amortized ~0.2ms
  └─ 1000 agents × 10μs / 60 frames

Other (physics, rendering):       8ms

Total knowledge system:           ~5ms / 16.6ms frame
Headroom:                         ~3ms for other systems

✓ Target achieved!
```

---

## Rust-Specific Advantages

Why this is faster in Rust than C#/scripting:

| Feature | Rust | C# (Unity) | Python/Lua |
|---------|------|------------|------------|
| HashMap lookup | 20-30ns | 50-100ns | 500-1000ns |
| Vec iteration | 1ns/elem | 5ns/elem | 100ns/elem |
| Memory layout | Tight, no GC | Looser, GC pauses | Very loose, GC |
| Zero-cost Arc | Yes | No (ref overhead) | N/A |
| SIMD auto-vectorization | Yes | Limited | No |
| Parallel iteration | rayon (safe!) | Jobs (complex) | GIL blocks |

**Overall speedup**: 5-20x over C#, 50-100x over scripting languages.

---

## Scaling Predictions

| Agents | Frame Time | Notes |
|--------|------------|-------|
| 100 | <1ms | Easy |
| 500 | ~3ms | Comfortable |
| 1000 | ~5ms | Target achieved |
| 5000 | ~20ms | Would need further staggering (mod 50 instead of mod 10) |
| 10000 | ~40ms | Possible with aggressive staggering + culling (only update nearby agents) |

**Conclusion**: System can handle 1000+ agents at 60fps with proposed optimizations.
