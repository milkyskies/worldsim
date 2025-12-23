# Layer 8: Species & Cognitive Architecture

This document defines how different species of agents differ in their cognitive capabilities, traits, and behaviors. The goal is an **extensible system** where adding new species (wolves, birds, fish, etc.) is straightforward.

---

## Core Principle: Shared Architecture, Different Parameters

All agents use the same underlying systems:
- **Three Brains**: Survival, Emotional, Rational
- **MindGraph**: Knowledge representation
- **Inventory**: Items carried
- **AgentState**: Hunger, Energy, etc.

Species differ in **parameters** and **weights**, not architecture. This enables emergent behavior differences without code duplication.

**No hardcoded behaviors!** If a deer flees from humans, it's because:
1. Deer has knowledge: `(Person, HasTrait, Dangerous)`
2. Deer sees a Person → Emotional brain triggers Fear
3. Survival brain's fear response → Flee action

This is emergent from knowledge, not `if species == deer && sees_human { run() }`.

---

## Species Profile

Each species is defined by a `SpeciesProfile` component:

```rust
#[derive(Component, Clone)]
struct SpeciesProfile {
    // === Identity ===
    species: Species,           // Enum: Human, Deer, Wolf, etc.
    
    // === Cognitive Parameters (continuous, not tiers!) ===
    max_plan_depth: usize,      // Max steps in a plan (1 = reactive, 10 = strategic)
    memory_capacity: usize,     // Max triples in MindGraph before aggressive decay
    memory_decay_rate: f32,     // How fast memories fade (0.0 = perfect, 1.0 = instant)
    
    // === Brain Power Base Weights (sum to 1.0) ===
    survival_weight: f32,       // Base influence of survival brain
    emotional_weight: f32,      // Base influence of emotional brain  
    rational_weight: f32,       // Base influence of rational brain
    
    // === Physical ===
    base_speed: f32,            // Movement speed multiplier
    vision_range: f32,          // How far can see
    
    // === Diet (determines what's edible) ===
    diet: Diet,                 // Herbivore, Carnivore, Omnivore
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Species {
    Human,
    Deer,
    Wolf,
    Rabbit,
    // ... more as needed
}

#[derive(Clone, Copy)]
enum Diet {
    Herbivore,  // Plants only
    Carnivore,  // Meat only
    Omnivore,   // Everything
}
```

---

## Cognitive Parameters (Continuous Scale)

Instead of discrete "tiers", each species has continuous values:

| Parameter | Low End | High End | Notes |
|-----------|---------|----------|-------|
| `max_plan_depth` | 1 (reactive) | 10+ (strategic) | Limits A* planner depth |
| `memory_capacity` | 50 | 10000 | Triples before forced decay |
| `memory_decay_rate` | 0.0 | 1.0 | 0 = never forgets, 1 = goldfish |
| `survival_weight` | 0.0 | 1.0 | Higher = more reactive |
| `rational_weight` | 0.0 | 1.0 | Higher = more planning |

Example spectrum:
```
Insect ─── Rabbit ─── Deer ─── Dog ─── Wolf ─── Chimp ─── Human
plan:1     plan:1     plan:2   plan:3  plan:4   plan:6    plan:10
```

---

## Knowledge-Driven Behavior

**Critical**: Behavior differences come from **knowledge**, not code.

### Innate Knowledge per Species

When a deer spawns, it gets this knowledge:
```
// What deer know is edible (via Ontology trait)
(Berry, HasTrait, Edible)     // From Ontology
(Grass, HasTrait, Edible)     // Deer-specific

// What deer know is dangerous
(Person, HasTrait, Dangerous)
(Wolf, HasTrait, Dangerous)

// What deer know to flee from
(Dangerous, TriggersEmotion, Fear(0.8))
```

When a wolf spawns:
```
// What wolves know is edible
(Meat, HasTrait, Edible)
(Deer, HasTrait, Prey)        // Deer can be hunted (yields Meat)

// What wolves know is dangerous
(Person, HasTrait, Dangerous)
(Bear, HasTrait, Dangerous)

// Wolves don't fear deer - they hunt them!
```

### How Fear → Flee Works

1. Deer perceives Person entity
2. Perception system adds to MindGraph: `(Entity_42, IsA, Person)`
3. Emotional brain queries: "Does Person have associations?"
4. Finds: `(Person, HasTrait, Dangerous)` → `(Dangerous, TriggersEmotion, Fear)`
5. Adds Fear emotion to EmotionalState
6. Survival brain sees high Fear → proposes Flee action
7. Deer runs away!

**No hardcoding needed.** Change knowledge → change behavior.

---

## Example Species Profiles

### Deer 🦌
```rust
SpeciesProfile {
    species: Species::Deer,
    
    max_plan_depth: 2,
    memory_capacity: 100,
    memory_decay_rate: 0.3,
    
    survival_weight: 0.70,
    emotional_weight: 0.20,
    rational_weight: 0.10,
    
    base_speed: 1.2,
    vision_range: 80.0,
    diet: Diet::Herbivore,
}
```

**Innate Knowledge:**
- Berry, Grass → Edible
- Person, Wolf → Dangerous → Fear
- BerryBush → Produces Berry

### Wolf 🐺 (Future)
```rust
SpeciesProfile {
    species: Species::Wolf,
    
    max_plan_depth: 4,
    memory_capacity: 500,
    memory_decay_rate: 0.1,
    
    survival_weight: 0.40,
    emotional_weight: 0.35,
    rational_weight: 0.25,
    
    base_speed: 1.4,
    vision_range: 120.0,
    diet: Diet::Carnivore,
}
```

**Innate Knowledge:**
- Deer, Rabbit → Prey (huntable for meat)
- Person, Bear → Dangerous
- Pack members → Friendly

### Human 🧑
```rust
SpeciesProfile {
    species: Species::Human,
    
    max_plan_depth: 10,
    memory_capacity: 10000,
    memory_decay_rate: 0.01,
    
    survival_weight: 0.33,
    emotional_weight: 0.33,
    rational_weight: 0.34,
    
    base_speed: 1.0,
    vision_range: 100.0,
    diet: Diet::Omnivore,
}
```

**Knowledge:** Comes from Culture system, not innate.

---

## Trait Systems

Different species have different personality models:

### All Animals
```rust
// Simple 2-trait model for basic animals
boldness: f32,      // 0 = fearful, 1 = reckless
aggression: f32,    // 0 = passive, 1 = aggressive
```

### Pack Animals (wolves, dogs)
```rust
// Adds social traits
sociability: f32,   // 0 = loner, 1 = pack-oriented
dominance: f32,     // 0 = follower, 1 = alpha
```

### Humans
```rust
// Full Big Five (existing system)
openness, conscientiousness, extraversion, agreeableness, neuroticism
```

---

## Implementation Plan

### Phase 1: SpeciesProfile Component ✅ (Now)
- [x] Create `SpeciesProfile` component
- [x] Create `Species` and `Diet` enums
- [x] Add to spawners (deer, human)
- [x] Add deer innate knowledge: `(Person, HasTrait, Dangerous)`

### Phase 2: Apply Parameters
- [ ] Limit planner depth by `max_plan_depth`
- [ ] Apply brain weight defaults from profile
- [ ] Use `memory_capacity` for MindGraph limits

### Phase 3: Trait Refactor
- [ ] Create trait enums (Universal, Pack, Human)
- [ ] Deer uses Universal traits only
- [ ] Human continues using Big Five

### Future
- [ ] Add more species (wolf, rabbit, bird)
- [ ] Predator-prey dynamics
- [ ] Pack coordination

---

## Emergent Behaviors We Want

| Scenario | How It Works |
|----------|--------------|
| Deer flees human | Knowledge: Person→Dangerous→Fear→Flee |
| Wolf hunts deer | Knowledge: Deer→Prey, hunger triggers hunting |
| Human tames wolf | Repeated positive interactions change wolf's knowledge of that human |
| Deer learns new danger | After being attacked, adds (Attacker, HasTrait, Dangerous) |
| Herd behavior | Deer seeks proximity to other Deer (social drive) |
