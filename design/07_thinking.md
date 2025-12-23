# Layer 6B: The Thinking System (Three Brains Architecture)

How agents make decisions. Not a single unified system, but **competing neural subsystems** that fight for control.

## Core Philosophy

Real brains aren't rational optimizers. They're messy coalitions of older and newer systems:
- The survival instincts that kept our ancestors alive
- The emotional associations that guide us toward good/away from bad
- The rational planning that lets us build cathedrals

These systems **compete**. Sometimes you KNOW you shouldn't eat the cake, but you do anyway. Sometimes fear overrides logic. Sometimes you stick to a plan despite discomfort.

**Rimworld's Limitation**: One unified utility function. Predictable, rational-ish behavior.

**Our Approach**: Three brains proposing actions, personality and state determining who wins.

---

## The Three Brains

```
┌─────────────────────────────────────────────────────────────────┐
│                        ARBITRATION                               │
│         (personality + stress + state determines winner)         │
└─────────┬─────────────────────┬─────────────────────┬───────────┘
          │                     │                     │
  ┌───────▼───────┐     ┌───────▼───────┐     ┌───────▼───────┐
  │   SURVIVAL    │     │   EMOTIONAL   │     │   RATIONAL    │
  │     BRAIN     │     │     BRAIN     │     │     BRAIN     │
  │               │     │               │     │               │
  │ • Reactive    │     │ • Associative │     │ • Planning    │
  │ • Immediate   │     │ • Memory-based│     │ • Multi-step  │
  │ • No planning │     │ • Feeling     │     │ • Social rules│
  │               │     │               │     │               │
  │ "HUNGRY! EAT!"│     │ "Bob = fear,  │     │ "I should ask │
  │ "DANGER! RUN!"│     │  avoid Bob"   │     │  before taking│
  │ "TIRED! SLEEP"│     │               │     │  Bob's food"  │
  └───────────────┘     └───────────────┘     └───────────────┘
```

### Survival Brain (Reptilian / System 0)

The oldest system. Fast, reactive, no planning. Responds to **immediate physical state**.

**Inputs:**
- Physical needs (hunger, energy, pain)
- Immediate threats (danger in vision)
- Basic drives at critical levels

**Outputs:**
- Simple, immediate actions
- No consideration of consequences
- No social awareness

**Characteristics:**
- Ignores ownership ("that's Bob's food" - doesn't care)
- Ignores social rules ("stealing is wrong" - doesn't understand)
- Cannot plan ahead ("I'll be hungry later" - only sees NOW)
- Very fast response time

```rust
fn survival_brain(needs: &Needs, threats: &[Entity]) -> Option<BrainProposal> {
    // Immediate threat? FLEE
    if let Some(threat) = threats.first() {
        return Some(BrainProposal {
            action: Action::FleeFrom(*threat),
            urgency: 100.0,
        });
    }

    // Starving? EAT ANYTHING
    if needs.hunger > 70.0 {
        return Some(BrainProposal {
            action: Action::EatNearest,  // Ignores ownership!
            urgency: needs.hunger,
        });
    }

    // Exhausted? SLEEP NOW
    if needs.energy < 15.0 {
        return Some(BrainProposal {
            action: Action::SleepHere,  // Ignores if safe!
            urgency: 100.0 - needs.energy,
        });
    }

    None  // No urgent survival needs
}
```

### Emotional Brain (Limbic / System 1)

Driven by **emotional associations** built from experience. Doesn't plan, but has memory.

**Inputs:**
- Emotional associations (from EmotionalProfile)
- Current emotions (fear, joy, anger, etc.)
- Visible entities and their tags

**Outputs:**
- Approach things with positive associations
- Avoid things with negative associations
- React to emotional triggers

**Characteristics:**
- Uses learned associations ("wolves hurt me before")
- Responds to feelings, not logic
- Can be wrong (phobias, irrational attachments)
- Medium response time

```rust
fn emotional_brain(
    emotions: &EmotionalState,
    associations: &EmotionalProfile,
    visible: &VisibleObjects,
) -> Option<BrainProposal> {
    let mut strongest_response: Option<BrainProposal> = None;
    let mut strongest_urgency = 0.0;

    for entity in &visible.entities {
        let feelings = associations.get_feelings_for(entity);

        // Strong fear? AVOID
        if feelings.fear > strongest_urgency && feelings.fear > 0.3 {
            strongest_response = Some(BrainProposal {
                action: Action::AvoidEntity(*entity),
                urgency: feelings.fear * 80.0,
            });
            strongest_urgency = feelings.fear;
        }

        // Strong positive feeling? APPROACH
        if feelings.joy > strongest_urgency && feelings.joy > 0.3 {
            strongest_response = Some(BrainProposal {
                action: Action::ApproachEntity(*entity),
                urgency: feelings.joy * 50.0,
            });
            strongest_urgency = feelings.joy;
        }

        // Anger? might propose attack...
    }

    // Also respond to current emotional state
    if emotions.get_intensity(EmotionType::Fear) > 0.7 {
        // General anxiety - seek safety
        return Some(BrainProposal {
            action: Action::SeekSafety,
            urgency: emotions.get_intensity(EmotionType::Fear) * 90.0,
        });
    }

    strongest_response
}
```

### Rational Brain (Neocortex / System 2)

The planner. Can think multiple steps ahead, consider consequences, follow social rules.

**Inputs:**
- Current world state
- Goals (from drives, memories, beliefs)
- Available actions and their effects
- Personality (affects action costs)

**Outputs:**
- Multi-step plans (GOAP)
- Socially appropriate actions
- Long-term goal pursuit

**Characteristics:**
- Considers ownership ("that's Bob's food")
- Follows social rules ("stealing is wrong")
- Plans ahead ("I'll need food for the trip")
- Weighs costs by personality
- Slow, effortful processing

```rust
fn rational_brain(
    state: &mut RationalState,
    world: &WorldState,
    goals: &Goals,
    personality: &Personality,
    actions: &[ActionTemplate],
) -> Option<BrainProposal> {
    // If we have a valid plan, continue it
    if let Some(plan) = &state.current_plan {
        if let Some(next_action) = plan.get(state.plan_index) {
            // Verify preconditions still met
            if next_action.preconditions_met(world) {
                return Some(BrainProposal {
                    action: next_action.clone(),
                    urgency: 30.0,  // Moderate - can be overridden
                });
            } else {
                // Plan invalidated, need to replan
                state.current_plan = None;
            }
        }
    }

    // No plan or plan complete - pick highest priority goal and plan
    if let Some(goal) = goals.highest_priority() {
        let plan = goap_plan(world, goal, actions, personality);
        if let Some(plan) = plan {
            state.current_plan = Some(plan.clone());
            state.plan_index = 0;
            return Some(BrainProposal {
                action: plan[0].clone(),
                urgency: goal.priority * 0.5,
            });
        }
    }

    // No goals, no plan - maybe wander?
    Some(BrainProposal {
        action: Action::Wander,
        urgency: 10.0,
    })
}
```

---

## Arbitration: Who Wins?

Each brain proposes an action with urgency. **Arbitration** decides who gets control.

### Brain Power Calculation

```rust
fn calculate_brain_powers(agent: &Agent) -> (f32, f32, f32) {
    // SURVIVAL POWER
    // - Increases with critical needs
    // - Increases with immediate danger
    // - Exponential curve (kicks in HARD when critical)
    let survival_power =
        (agent.needs.hunger / 100.0).powf(2.0) * 100.0 +
        (agent.needs.pain / 100.0).powf(2.0) * 100.0 +
        ((100.0 - agent.needs.energy) / 100.0).powf(3.0) * 80.0 +
        if agent.sees_threat { 50.0 } else { 0.0 };

    // EMOTIONAL POWER
    // - Increases with emotional intensity
    // - Increases with neuroticism (personality)
    // - Increases with stress
    let emotional_intensity = agent.emotions.total_intensity();
    let emotional_power =
        emotional_intensity * 50.0 *
        (0.5 + agent.personality.neuroticism * 0.5) *
        (1.0 + agent.stress / 200.0);

    // RATIONAL POWER
    // - Baseline from conscientiousness
    // - Decreases with stress (can't think straight)
    // - Decreases with decision fatigue
    // - Decreases with extreme needs
    let stress_penalty = agent.stress / 100.0;
    let needs_penalty = (agent.needs.hunger + agent.needs.pain) / 200.0;
    let rational_power =
        (30.0 + agent.personality.conscientiousness * 40.0) *
        (1.0 - stress_penalty * 0.5) *
        (1.0 - needs_penalty * 0.3);

    (survival_power, emotional_power, rational_power)
}
```

### Selection Methods

**Option A: Weighted Random**
```rust
fn arbitrate_weighted(proposals: &[BrainProposal], powers: (f32, f32, f32)) -> Action {
    let weights = [
        proposals[0].urgency * powers.0,
        proposals[1].urgency * powers.1,
        proposals[2].urgency * powers.2,
    ];
    let total: f32 = weights.iter().sum();
    let roll = random::<f32>() * total;

    // Pick based on roll
    // Adds unpredictability - sometimes emotional wins even when rational is "stronger"
}
```

**Option B: Winner Take All (with threshold)**
```rust
fn arbitrate_winner(proposals: &[BrainProposal], powers: (f32, f32, f32)) -> Action {
    let scores = [
        proposals[0].map(|p| p.urgency * powers.0).unwrap_or(0.0),
        proposals[1].map(|p| p.urgency * powers.1).unwrap_or(0.0),
        proposals[2].map(|p| p.urgency * powers.2).unwrap_or(0.0),
    ];

    // Highest score wins
    // More predictable, but still emergent from competing systems
}
```

**Recommendation**: Winner Take All for predictability, but with some noise/randomness added to scores to prevent robotic behavior.

---

## GOAP Planning (Rational Brain)

Goal-Oriented Action Planning for multi-step tasks.

### World State

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
enum Fact {
    // Location
    AtLocation(LocationId),
    NearEntity(EntityId),

    // Inventory
    HasItem(ItemType, u32),

    // Physical state
    IsHungry,
    IsTired,
    IsInPain,

    // World state
    EntityExists(EntityId),
    BuildingExists(BuildingType, LocationId),
}

type WorldState = HashSet<Fact>;
```

### Action Templates

```rust
struct ActionTemplate {
    id: ActionId,
    name: &'static str,

    // What must be true to do this
    preconditions: Vec<Fact>,

    // What becomes true after
    effects_add: Vec<Fact>,
    effects_remove: Vec<Fact>,

    // Cost calculation
    base_cost: f32,

    // Personality modifier - returns multiplier
    // High value = expensive for this personality
    personality_modifier: fn(&Personality) -> f32,
}

// Examples:
const ACTION_EAT: ActionTemplate = ActionTemplate {
    name: "Eat",
    preconditions: vec![Fact::HasItem(ItemType::Food, 1)],
    effects_add: vec![],
    effects_remove: vec![Fact::IsHungry],
    base_cost: 5.0,
    personality_modifier: |_| 1.0,
};

const ACTION_STEAL: ActionTemplate = ActionTemplate {
    name: "Steal",
    preconditions: vec![Fact::NearEntity(/* owner */)],
    effects_add: vec![Fact::HasItem(ItemType::Food, 1)],
    effects_remove: vec![],
    base_cost: 10.0,
    personality_modifier: |p| {
        // Agreeable people find stealing VERY costly
        // Disagreeable people find it cheap
        1.0 + (p.agreeableness * 10.0)
    },
};

const ACTION_HARVEST: ActionTemplate = ActionTemplate {
    name: "Harvest",
    preconditions: vec![Fact::NearEntity(/* harvestable */)],
    effects_add: vec![Fact::HasItem(ItemType::Food, 1)],
    effects_remove: vec![],
    base_cost: 15.0,
    personality_modifier: |_| 1.0,
};
```

### The Planner (A* in State Space)

```rust
fn goap_plan(
    current: &WorldState,
    goal: &[Fact],
    actions: &[ActionTemplate],
    personality: &Personality,
) -> Option<Vec<ActionTemplate>> {
    // A* search where:
    // - Nodes are world states
    // - Edges are actions
    // - Edge cost = base_cost * personality_modifier
    // - Heuristic = count of unmet goal facts

    let start = current.clone();
    let is_goal = |state: &WorldState| goal.iter().all(|f| state.contains(f));
    let heuristic = |state: &WorldState| {
        goal.iter().filter(|f| !state.contains(f)).count() as f32
    };

    astar(start, is_goal, heuristic, |state| {
        // Generate successors: apply each valid action
        actions.iter()
            .filter(|a| a.preconditions.iter().all(|p| state.contains(p)))
            .map(|a| {
                let mut next = state.clone();
                for fact in &a.effects_remove { next.remove(fact); }
                for fact in &a.effects_add { next.insert(fact.clone()); }
                let cost = a.base_cost * (a.personality_modifier)(personality);
                (next, a.clone(), cost)
            })
            .collect()
    })
}
```

### Plan Execution & Interruption

```rust
struct RationalState {
    current_plan: Option<Vec<ActionTemplate>>,
    plan_index: usize,
    current_goal: Option<Goal>,
}

impl RationalState {
    fn advance(&mut self) {
        self.plan_index += 1;
        if self.plan_index >= self.current_plan.as_ref().map(|p| p.len()).unwrap_or(0) {
            // Plan complete
            self.current_plan = None;
            self.current_goal = None;
        }
    }

    fn invalidate(&mut self) {
        // Something changed, need to replan
        self.current_plan = None;
        // Keep goal, will replan next tick
    }
}
```

**Key Insight**: The rational brain proposes the next step in its plan, but it might not win arbitration! The plan sits there, ready to resume when rational brain regains control.

---

## Goal Formation

Goals emerge from the agent's state, not manually assigned.

### Sources of Goals

```rust
enum GoalSource {
    Survival,   // From critical needs
    Emotional,  // From emotional state
    Drive,      // From personality drives
    Memory,     // From past experiences
    Belief,     // From formed beliefs
    Social,     // From relationships (Phase 6+)
}

struct Goal {
    desired_state: Vec<Fact>,
    priority: f32,
    source: GoalSource,
}

fn form_goals(
    needs: &Needs,
    drives: &Drives,
    emotions: &EmotionalState,
    memories: &Memory,
    beliefs: &Beliefs,
) -> Vec<Goal> {
    let mut goals = Vec::new();

    // SURVIVAL GOALS (high priority when critical)
    if needs.hunger > 50.0 {
        goals.push(Goal {
            desired_state: vec![Fact::HasItem(ItemType::Food, 1)],
            priority: needs.hunger * 1.5,
            source: GoalSource::Survival,
        });
    }

    // DRIVE GOALS (persistent, moderate priority)
    if drives.curiosity > 0.6 {
        goals.push(Goal {
            desired_state: vec![Fact::AtLocation(LocationId::Unexplored)],
            priority: drives.curiosity * 40.0,
            source: GoalSource::Drive,
        });
    }

    if drives.security > 0.7 {
        goals.push(Goal {
            desired_state: vec![Fact::HasItem(ItemType::Food, 10)], // Hoard!
            priority: drives.security * 30.0,
            source: GoalSource::Drive,
        });
    }

    // MEMORY-BASED GOALS
    // "Last winter I was cold" → Goal: Build shelter
    for memory in memories.significant_memories() {
        if memory.tags.contains(&Tag::Cold) && memory.valence < -0.5 {
            goals.push(Goal {
                desired_state: vec![Fact::BuildingExists(BuildingType::Shelter, LocationId::Here)],
                priority: 25.0,
                source: GoalSource::Memory,
            });
        }
    }

    // BELIEF-BASED GOALS
    // Belief: "Wolves are dangerous" + Saw wolf → Goal: Get weapon
    if beliefs.has_belief("wolves_dangerous") && saw_wolf_recently {
        goals.push(Goal {
            desired_state: vec![Fact::HasItem(ItemType::Weapon, 1)],
            priority: 35.0,
            source: GoalSource::Belief,
        });
    }

    goals.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap());
    goals
}
```

---

## Additional Systems

### Attention / Salience

Not everything in perception gets equal processing. Emotional associations affect what you notice.

```rust
fn calculate_salience(
    entity: Entity,
    associations: &EmotionalProfile,
    needs: &Needs,
    tags: &Tags,
) -> f32 {
    let mut salience = 1.0;  // Baseline

    // Emotional associations increase salience
    let feelings = associations.get_feelings_for(entity);
    salience += feelings.fear * 3.0;      // Threats are VERY salient
    salience += feelings.joy * 1.5;       // Liked things noticeable
    salience += feelings.anger * 2.0;     // Enemies noticeable

    // Needs increase salience of relevant things
    if tags.has(Tag::Food) {
        salience += needs.hunger / 50.0;  // Hungry? Notice food more
    }

    // Recent interaction increases salience
    // etc.

    salience
}

// In perception system:
fn filter_by_salience(visible: &[Entity], agent: &Agent) -> Vec<Entity> {
    let mut with_salience: Vec<_> = visible.iter()
        .map(|e| (*e, calculate_salience(*e, &agent.associations, &agent.needs)))
        .collect();

    with_salience.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Only process top N most salient entities
    with_salience.into_iter()
        .take(ATTENTION_LIMIT)
        .map(|(e, _)| e)
        .collect()
}
```

### Momentum / Commitment

Once started, actions have resistance to interruption.

```rust
struct CurrentAction {
    action: Action,
    ticks_invested: u32,
    total_ticks: u32,
}

fn calculate_switching_cost(current: &CurrentAction) -> f32 {
    // More invested = higher cost to switch
    let progress = current.ticks_invested as f32 / current.total_ticks as f32;

    // Sunk cost curve - peaks at ~70% complete
    let sunk_cost = (progress * 2.0).min(1.0) * 20.0;

    sunk_cost
}

// In arbitration:
fn arbitrate_with_momentum(
    proposals: &[BrainProposal],
    powers: (f32, f32, f32),
    current: &Option<CurrentAction>,
) -> Action {
    let switching_cost = current.as_ref()
        .map(calculate_switching_cost)
        .unwrap_or(0.0);

    // Current action gets bonus equal to switching cost
    // New actions must overcome this bonus to win
    // ...
}
```

### Decision Fatigue

Rational brain takes effort. Under load, defaults to simpler systems.

```rust
struct CognitiveState {
    decisions_today: u32,
    last_complex_decision: u64,  // tick
}

fn rational_power_with_fatigue(
    base_power: f32,
    cognitive: &CognitiveState,
    current_tick: u64,
) -> f32 {
    // Each decision today reduces rational power slightly
    let fatigue_penalty = (cognitive.decisions_today as f32 * 0.02).min(0.3);

    // Recent complex decisions cause temporary reduction
    let recency_penalty = if current_tick - cognitive.last_complex_decision < 100 {
        0.1
    } else {
        0.0
    };

    base_power * (1.0 - fatigue_penalty - recency_penalty)
}
```

### Habits (Phase 3+)

Learned behaviors that bypass conscious planning.

```rust
struct Habit {
    trigger: HabitTrigger,
    action: Action,
    strength: f32,  // Built through repetition
}

enum HabitTrigger {
    TimeOfDay(u32),           // "Eat breakfast at 7am"
    Location(LocationId),      // "Pray when entering temple"
    AfterAction(ActionType),   // "Wash hands after eating"
    Emotion(EmotionType),      // "Eat when stressed"
}

// Habits are checked BEFORE the three brains
fn check_habits(
    habits: &[Habit],
    context: &HabitContext,
) -> Option<BrainProposal> {
    for habit in habits {
        if habit.trigger.matches(context) && habit.strength > 0.5 {
            return Some(BrainProposal {
                action: habit.action.clone(),
                urgency: habit.strength * 40.0,
            });
        }
    }
    None
}
```

---

## Emergent Behaviors

The Three Brains architecture produces emergent behaviors that feel realistic:

| Scenario | What Happens |
|----------|--------------|
| Kind agent gets very hungry | Survival brain overpowers rational → Steals food → Later feels guilt (emotional response to own action) |
| Traumatized agent sees trigger | Emotional brain spikes → Flees even if rationally safe → Can't "think straight" |
| Agent mid-task gets hungry | If hunger moderate: rational continues task. If hunger critical: survival interrupts → Eats → Resumes task |
| Neurotic agent in new place | Emotional brain has more baseline power → More reactive to unfamiliar things → Slower to explore |
| Conscientious agent with plan | Rational brain has more power → Sticks to plan despite minor discomforts |
| Agent under extreme stress | "The Snap" - survival brain takes over completely → Does whatever relieves stress NOW |
| Agent sees loved one in danger | Emotional brain → HELP! → Overrides rational "it's too dangerous" |

---

## Integration with Existing Systems

```
┌─────────────────────────────────────────────────────────────────┐
│                     EXISTING SYSTEMS                             │
├─────────────────────────────────────────────────────────────────┤
│  Personality ──────────┬──────────────────────────────────────► │
│  (OCEAN traits)        │                                        │
│                        ▼                                        │
│  Drives ◄───────── calculated from ─────────────────────────►   │
│  (curiosity, etc)      │                                        │
│                        │                                        │
│  Needs ────────────────┼────────► SURVIVAL BRAIN                │
│  (hunger, energy)      │                                        │
│                        │                                        │
│  Emotions ─────────────┼────────► EMOTIONAL BRAIN               │
│  (fear, joy, etc)      │                 ▲                      │
│                        │                 │                      │
│  EmotionalProfile ─────┴─────────────────┘                      │
│  (associations)                                                 │
│                                                                 │
│  Memory ───────────────┬────────► GOAL FORMATION                │
│  (episodic)            │                 │                      │
│                        │                 ▼                      │
│  Beliefs ──────────────┴────────► RATIONAL BRAIN                │
│  (semantic)                              │                      │
│                                          ▼                      │
│                                    GOAP PLANNER                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implementation Phases

**Phase 2 (Current):**
- Three Brains architecture
- Basic GOAP planner
- Personality-weighted costs
- Simple arbitration

**Phase 3 (Memory):**
- Memory-based goals ("I was cold" → Build shelter)
- Belief-based goals
- Habits from repeated actions

**Phase 5 (Learning):**
- Skill-based action costs
- Learning new action templates

**Phase 6 (Social):**
- Social Brain (4th brain?)
- Theory of Mind in planning
- Reputation considerations

**Phase 8 (Stress):**
- Decision fatigue
- "The Snap" mechanics
- Willpower resource

---

**Previous**: [06_world.md](06_world.md)
**Next**: [08_implementation_notes.md](08_implementation_notes.md)
