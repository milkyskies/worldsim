# Layer 3B: Personality Traits

Personality is NOT assigned - it's calculated from phenotypes + experiences.

## Trait Calculation
```
Personality_Trait = f(
  relevant_phenotypes,
  accumulated_experiences,
  current_age
)
```

## Core Personality Dimensions
Each is a spectrum (0-100):
- **Openness**: Curiosity vs Traditionalism
- **Conscientiousness**: Discipline vs Spontaneity
- **Extraversion**: Social energy vs Solitude preference
- **Agreeableness**: Compassion vs Self-interest
- **Neuroticism**: Emotional stability vs Anxiety

## Additional Traits
- **Bravery**: Calculated from anxiety phenotype + combat experiences
- **Honor**: Calculated from conscientiousness + cultural upbringing
- **Idealism**: Calculated from belief patterns + disappointments

## Trait Dynamics
- Start heavily weighted by phenotypes (genetic predisposition)
- Shift over time based on emotionally intense memories
- Stabilize somewhat in adulthood but never fully lock
- Can change dramatically from trauma or major life events

**Example:**
```
Bravery = 
  base_from_phenotype(low fear_response) +
  experience_modifier(survived_battles, saw_friends_die) +
  age_modifier(young = reckless, old = cautious)
```

---

# Layer 3C: Drive Strengths

Psychological drives that create ongoing motivation for behavior.

## Survival Needs (deplete over time, must be satisfied)
- Hunger, thirst, sleep, pain avoidance
- These are tracked in STATE, not here

## Psychological Drives (baseline strength from phenotypes)
- **Curiosity**: Drive to explore, learn, try new things
- **Social connection**: Need for companionship, belonging
- **Status/dominance**: Need for respect, power, hierarchy climbing
- **Autonomy**: Desire for independence, resisting control
- **Reproduction**: Sex drive, desire for children
- **Security**: Need for safety, predictability, risk avoidance

## Drive Strength Calculation
```
Drive_Strength = f(relevant_phenotypes, personality_traits)

Examples:
- Curiosity = openness phenotype × 0.8 + processing_speed phenotype × 0.2
- Social = extraversion trait × reward_sensitivity phenotype
- Status = low agreeableness + high confidence
```

## How Drives Influence Decisions
- High curiosity → explores dangerous areas even when irrational
- High status drive → takes risks for glory
- Low social drive → avoids social gatherings
- High security drive → hoards resources, avoids change

---

# Layer 4A: Beliefs

Beliefs are generalizations extracted from memory patterns, not assigned.

## Belief Structure
```
Belief {
  proposition: "orcs are dangerous" / "stealing is wrong" / "leader X is trustworthy"
  strength: 0-100
  supporting_memories: [memory_ids]
  contradicting_memories: [memory_ids]
  source: personal_experience / taught_by_parent / cultural_norm
  cultural_norm: boolean (is this shared by >60% of social group?)
}
```

## Belief Formation
- System periodically scans memories for patterns
- Multiple memories supporting same idea → create/strengthen belief
- Strength = f(number_of_memories, emotional_intensity, recency)

## Belief Updates
- New contradicting memories → reduce strength
- BUT: existing strong beliefs create confirmation bias (filter what becomes memory)
- High-Openness personalities update beliefs faster
- Beliefs from trusted sources harder to change

## Belief Influence
- Strong beliefs override weak personality traits in decisions
- Beliefs filter perception (notice things that confirm beliefs)
- Beliefs drive arguments with those who hold contradictory beliefs
- Clustered beliefs form worldviews/ideologies

---

# Layer 4B: Memory Systems

Memory in psychology consists of multiple systems that work together.

## Psychological Memory Model

**Working Memory** (immediate awareness, seconds):
- Buffer of raw events you're currently processing
- Limited capacity (~7 items)
- What you're actively thinking about RIGHT NOW
- → Implemented as `WorkingMemory` component

**Long-Term Memory** (permanent storage, unlimited):
1. **Episodic Memory** - Specific events with context
   - "I saw Bob wave at me yesterday at the market"
   - → Implemented as `Memory` component
2. **Semantic Memory** - General knowledge and beliefs
   - "Wolves are dangerous", "Bob is friendly"
   - → Implemented as `Beliefs` component
3. **Procedural Memory** - Skills and how-to knowledge
   - "How to forge a sword", "How to track animals"
   - → Implemented as `Skills` component (future)

## Episodic Memory Structure
```
Memory {
  event_type: saw_violence / learned_skill / betrayed_by_friend / achievement
  participants: [person_ids]
  emotional_intensity: 0-100
  emotional_valence: positive / negative / mixed
  timestamp: game_tick
  decay_rate: f(intensity, reinforcement_count)
  tags: [relevant_concepts for querying]
}
```

## Memory Formation
- Events above significance threshold create memories
- Emotional intensity calculated from:
  - Unexpectedness (violated predictions)
  - Personal stakes (affects survival, status, relationships)
  - Trait sensitivity (high neuroticism = more intense negative memories)
- High intensity = slower decay

## Memory Formation & Storage

**Episodic** (`Memory` component):
- Specific events with emotional context
- "I saw John steal bread" (who, what, when, where, how I felt)
- Auto-forgets old memories (keeps last ~10 significant events)
- Decays over time unless reinforced

**Semantic** (`Beliefs` component):
- General knowledge extracted from episodic patterns
- "Elves are untrustworthy" (from multiple negative elf encounters)
- Forms slowly from repeated experiences
- More resistant to change (confirmation bias)

**Procedural** (`Skills` component - future):
- How-to knowledge and muscle memory
- "I know how to forge swords" (built through practice)
- Improves with repetition
- Decays slowly when unused

## Memory Retrieval
When making decisions, query relevant memories:
```
relevant_memories = filter(all_memories,
  where: tags_match(decision_context) AND
         not_fully_decayed(current_time)
)
```
Weight by: intensity × recency × relevance

## Memory Effects
- Form beliefs (patterns extracted)
- Shift personality (traumatic memories increase neuroticism)
- Define relationships (remember what others did)
- Guide decisions (remember outcomes of past choices)
- Can be reinforced (repeated experiences strengthen)
- Can be contradicted (new experiences weaken)
- Decay over time (forgotten below retrieval threshold)

---

# Layer 4C: Skills & Knowledge

Skills are learned abilities, separate from traits.

## Skill Structure
```
Skill {
  name: combat / smithing / farming / medicine / persuasion
  level: 0-100
  experience: accumulated practice points
  learning_rate: f(phenotypes, age, teacher_quality)
  decay_rate: unused skills atrophy slowly
  specializations: [] (can branch into sub-skills)
}
```

## Skill Learning
```
on_action_performed(skill_type):
  xp_gain = base_xp × learning_rate × difficulty
  skill.experience += xp_gain
  if experience > threshold:
    skill.level += 1
```

## Learning Rate Modifiers
- **Phenotypes**: processing_speed phenotype = faster learning
- **Age**: young = faster learning, old = slower but retained better
- **Teacher**: learning from skilled person = huge bonus
- **Personality**: high Openness = tries new techniques, learns faster
- **Related skills**: existing skills provide transfer bonuses

## Skills vs Knowledge
- **Skills**: practical ability (can swing sword well, can craft tools)
- **Knowledge**: information (knows bronze recipe, knows medicinal herbs)

Knowledge stored as beliefs with "factual" tag:
- Learned from experience (discovered through trial and error)
- Taught by others (creates belief with source attribution)
- Read from books (requires literacy skill)
- Can be lost if no one remembers it

## Skill Effects
- Action success rate (high smithing = better weapons)
- Action speed (experienced farmer works faster)
- Unlock new actions (can't perform surgery without medicine skill)
- Social respect (master craftsman gains status)
- Teaching others (can transfer knowledge/skills)

---

# Layer 4E: Goals

Long-term motivations that persist across multiple decisions.

## Goal Structure
```
Goal {
  description: "become village chief" / "avenge father" / "master smithing"
  priority: 0-100
  formed_from: unsatisfied_drives + beliefs + memories
  progress: tracking toward completion
  subgoals: [] (steps toward main goal)
}
```

## Goal Formation
Goals emerge from:
- **Unsatisfied drives**: high status drive → "become leader"
- **Beliefs**: "family honor matters" → "avenge father's death"
- **Memories**: "I succeeded at crafting" → "become master smith"
- **Personality**: ambitious + high conscientiousness → long-term goals

## Goal Influence on Decisions
- Filter available actions (is this action relevant to goals?)
- Weight decisions (actions that advance goals weighted higher)
- Create persistence (continue pursuing goal across days/months)
- Can conflict (goal to be safe vs goal to avenge father)

## Goal Updates
- Progress tracking (completed subgoals increase motivation)
- Failure can abandon goal (repeated setbacks → give up)
- Success creates new goals (became chief → now defend village)
- Major events can create new urgent goals (loved one killed → revenge)

---

# Layer 5: State

Temporary modifiers separate from permanent traits.

## State Components
```
State {
  physical_needs: {
    hunger: 0-100 (increases over time)
    thirst: 0-100
    fatigue: 0-100
    pain: 0-100 (sum from body parts)
  },
  emotional_state: {
    current_mood: -100 to +100
    stress_level: 0-100
    active_emotions: [grief, anger, joy, fear]
  },
  social_context: {
    present_people: [person_ids]
    location_type: home / public / wilderness / combat
    ongoing_event: feast / battle / ritual
  },
  recent_events: [last N significant events]
}
```

## State Updates
- Physical needs degrade over time (hunger increases each tick)
- Emotional states decay toward baseline (defined by personality traits)
- Context changes based on location and who's nearby
- Recent events have temporary influence on decisions

## State Modifiers to Decisions
```
effective_trait = base_trait × state_modifier

Examples:
- High hunger → agreeableness × 0.3 (normally kind person becomes selfish)
- High stress → conscientiousness × 0.5 (normally disciplined person makes mistakes)
- Pain → rational thinking × 0.7 (hard to plan when hurting)
- In love → neuroticism × 0.6 (anxiety reduced temporarily)
```

---

# Layer 5B: Emotional Associations

Tags (concepts, entities, actions) mapped to emotional responses. This is how agents develop "feelings" about things.

## Structure
```
EmotionalAssociation {
  subject_tag: "Wolf" / "Violence" / "Apples" / Agent(45)
  emotions: [(EmotionType, intensity)]  // Can trigger multiple emotions!
  source: Genetic / Cultural / Personal
}

EmotionalProfile {
  associations: HashMap<Tag, Vec<EmotionalAssociation>>
}
```

## Sources of Associations

### Genetic (from phenotypes)
Born with predispositions. Cannot be unlearned, but can be overridden.
```
High fear_response gene → (Violence, Fear, 0.5)
Low fear_response gene → (Danger, Curiosity, 0.3)  // "Adrenaline junkie"
```

### Cultural (from parents/society)
Learned in childhood. Strong but can fade.
```
"Mom said wolves are dangerous" → (Wolf, Fear, 0.4, source=parent)
"Our tribe hates the Red Clan" → (RedClan, Anger, 0.6, source=culture)
```

### Personal (from experiences)
Formed from emotional memories. Reinforced or weakened over time.
```
Was attacked by wolf → (Wolf, Fear, 0.8, source=experience)
Ate apple and got sick → (Apple, Disgust, 0.5, source=experience)
```

## Mixed Emotions
Tags can trigger multiple emotions simultaneously:
```
Wolf → [(Fear, 0.6), (Anger, 0.2)]  // Scared AND angry
Social → [(Joy, 0.3), (Fear, 0.1)]  // Happy but also anxious (introvert)
```

## Subject Types
Associations can be attached to various levels of abstraction:
```rust
enum Subject {
    Tag(String),           // "Human", "Violence", "Food"
    Agent(Entity),         // Specific person (Bob)
    ItemType(ItemType),    // Category (Apples)
    Item(Entity),          // Specific item (Dad's specific sword)
    Action(String),        // "Wave", "Attack"
}
```

## Resolution Priority (Specific Overrides General)
When multiple associations apply, the **most specific** one takes precedence or combines.

**Priority Order:**
1. **Specific Entity** (Agent(Bob), Item(Excalibur))
2. **Specific Category** (ItemType(Sword))
3. **General Tag** (Tag("Weapon"), Tag("Human"))

**Example: "I hate Humans, but I like Bob"**
1. Event: Bob waves at me.
2. Bob has tags: `[Human, Male, Soldier]`.
3. Check `Subject::Agent(Bob)` → Found: **(Joy, 0.5)** ("I like Bob").
4. Check `Subject::Tag("Human")` → Found: **(Disgust, 0.4)** ("Ew, human").
5. **Result**: Specific (Joy) overrides General (Disgust) → I feel **Joy**.

*Alternative Logic: Mixed Feelings*
If we want mixed feelings, we can sum them:
Result = Joy(0.5) + Disgust(0.4) → Mixed state.

## How Associations Work
```
fn interpret_emotion(event, observer):
  tags = get_tags(event.subject)  // "Wave" has [Social, Friendly]
  
  for tag in tags:
    if observer.associations.contains(tag):
      apply_emotions(observer.associations[tag].emotions)
    else:
      apply_default(tag)  // Fallback from genetics
```

---

# Layer 8A: The "Mask" System (Social Camouflage)
People have a constant friction between who they *are* and who they *pretend to be*.

## Structure
- **True Self**: Calculated personality (e.g., Aggression: 80, Honesty: 20).
- **Mask**: The persona projected to fit a social role (e.g., "The Noble Knight" - Aggression: 10, Honesty: 90).
- **Leakage**: The probability of the particular mask failing.

## The "Strain" Equation
```
mask_dissonance = Distance(True_Self, Target_Mask)
current_stress = Agent.State.Stress (0-100)
leakage_chance = (mask_dissonance * 0.2) + (current_stress * 0.01)
```
- A psychopath pretending to be an empath accumulates huge Stress.
- **The Slip**: If `random() < leakage_chance`, they perform an "out of character" micro-action (a sneer, a cruel joke), revealing their true nature to observant witnesses.

---

# Layer 8B: The Stress Hydro-System
Stress is not just a mood; it is a hydraulic pressure system that builds up and *must* be released.

## Flow
1.  **Input (Compression)**:
    - **Suppression**: Vetoing the `Survival Brain` for the `Social Brain` (e.g., not eating the King's cake).
    - **Uncertainty**: Low `Information_Fidelity` environments (darkness, unknown enemies).
    - **Mask Maintenance**: High Dissonance = High Stress gain/tick.
2.  **Output (Relief Valves)**:
    - **Indulgence**: Executing a standard Drive action (eating, sleeping).
    - **Vices**: Alcohol/gambling (fast relief, long-term cost).
    - **Venting**: Screaming, violence (transfers Stress to others).

## The "Snap" (Breakdown)
When `Stress > Willpower`:
- The agent enters a **Fugue State**.
- The `Rational` and `Social` brains are disabled.
- The `Survival Brain` executes the action with the *highest immediate Stress relief* regardless of consequences (e.g., a loyal soldier deserting his post to sleep).

---

# Layer 8F: Object Psychometry (Totems)
Objects are not just stats; they hold emotional resonance.

## Mechanic
- **History Log**: Items track owners and critical events (used to kill King X).
- **Psychic Imprint**: High-intensity memories "stick" to nearby items.
- **Effect**:
    - **Reputation**: A sword that killed 100 men gives an implicit Intimidation bonus, even if stats are normal.

---

# Layer 8G: Gap Fixes & Refinements

## 1. Willpower (The Mental HP)
Defined as the resistance to Stress and Temptation.
- **Formula**: `Willpower = (Conscientiousness * 0.5) + (100 - Neuroticism * 0.5) + (Energy_State * 0.2)`
- **Usage**: Used to resist `The Snap` and to force `Unpleasant Actions` (like working when tired).

## 2. Genes → Personality Formulas
Explicit links between Layer 1 and Layer 3B.
- `Neuroticism = Baseline_Anxiety_Genotype * 0.6 + Trauma_Memory_Count * 4.0`
- `Extraversion = Dopamine_Receptor_Genotype * 0.7 + Positive_Social_Memories * 0.3`
- `Openness = Processing_Speed_Genotype * 0.5 + Diversity_of_Experiences * 0.5`

## 3. Gossip Hallucination → Memory
How low fidelity gossip becomes false memory.
```
on_receive_gossip(message):
  if message.fidelity < 0.5:
    // Hallucination: Fill gaps with Receiver's Biases
    if Receiver.hates(message.subject):
        message.content = twist_negative(message.content)
        message.tags.add("Malicious")
  
  create_memory(message) // Stored as if it were true
```

## 4. Routine & Consistency (Neurodiversity)
Some agents have a high biological need for Sameness.

**New Trait: `Rigidity` (0-100)**
- Derived from: `Low Openness` + `High Neuroticism` + `Sensory_Sensitivity_Gene`.
- **Low Rigidity**: Adaptive, ignores schedule changes.
- **High Rigidity**: Requires strict adherence to patterns.

**The Routine Mechanic**:
1.  **Pattern Check**: Agent checks `Memory` for `Action(Time - 24h)`.
2.  **Match**: If `Current_Action == Yesterday_Action` → `Stress -= 2` (Soothing).
3.  **Mismatch**: If `Current_Action != Yesterday_Action` → `Stress += (Rigidity * 0.1)`.
    - *Emergence*: High-rigidity agents will self-enforce strict schedules and may `Meltdown` if forced to deviate (e.g., "The chair was moved").

## 5. Personality Projection (The "Boss" Effect)
How internal psychology forces external social change.
- **Mechanism**: Agents with high `Status` or `Dominance` try to reshape their environment to match their Traits.
- **Example**: A `High Rigidity` Boss creates `Strict Contracts` for everyone, not for efficiency, but to lower his own Stress.
- **Conflict**: A `Chaotic` employee working for a `Rigid` Boss accumulates Stress vs. the Boss's projected environment.
