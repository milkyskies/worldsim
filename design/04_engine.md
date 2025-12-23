# Layer 6: Advanced Cognition (The Engine)

## 6A. Subjective Reality (The "Mind's Eye")
Agents do not access the Game State. They access a flawed copy.

### Structure
```
SubjectiveReality {
  observations: [Entity_Snapshot] (What I see NOW)
  working_memory: [Event_Log] (What I saw recently - decays fast)
  long_term_beliefs: [Fact_Map] ("Door is locked", "Grimnar is coward")
  theory_of_mind: {Agent_ID: Estimated_Personality}
}
```
**Emergence**:
- **Surprise**: Occurs when `Action.Result != Mental_Simulation.Result`.
- **Deception**: Manipulation is simply feeding false data into another's Subjective Reality.

## 6B. Goal-Oriented Planning (GOAP + Personality)
Agents filter goals through their Traits.

### Logic
1.  **Impetus**: Drive (Hunger) > Threshold.
2.  **Goal**: `State: Satiated`.
3.  **Plan Generation**:
    - *Option A*: `Buy Food` (Cost: Cheap).
    - *Option B*: `Steal Food` (Cost: Social Risk).
4.  **Personality Weighting**:
    - **Honest Agent**: `Steal_Cost *= 100` (Will starve before stealing).
    - **Pragmatic Agent**: `Steal_Cost *= 1` (Stealing is just another tool).
5.  **Mental Simulation**:
    - Agent runs the plan in `SubjectiveReality`.
    - "If I steal, Guard (Model: Lazy) won't catch me." (Risk Assessment).

---

# Layer 7: The Physics of Interaction

## 7A. Affordance System (Ontology)
We do not code `Stab()`. We code Physics.

**Objects grant Affordances:**
- `Dagger`: Grants `Lethal_Force`, `Concealable`.
- `Rock`: Grants `Blunt_Force`.

**Context synthesizes Meaning:**
- `Action`: Apply `Lethal_Force` (Dagger) to Target.
- `Context`: Target is `Father`.
- **Resulting Event**: **"Parricide"** (Extreme Horror Tag).
- `Context`: Target is `Enemy`.
- **Resulting Event**: **"Combat"** (Neutral Tag).

## 7B. Action Granularity (The Stream)
Actions are not atomic. They are streams of events.
1.  **Tick 0**: Start `Action: Eat`.
2.  **Tick 30**: Event `Sound: Explosion`.
3.  **Interrupt Check**: `Survival_Brain` overrides.
4.  **Reaction**: Cancel `Eat`, Start `Flee`.
5.  **Outcome**: Food dropped, Agent survived.

## 7C. Outcome Prediction (The Variance Cone)
Skill determines precision, not permission.
- **Master**: Intention `Hit Head` ± 1° variance.
- **Novice**: Intention `Hit Head` ± 45° variance.
- **Dunning-Kruger Effect**: Novices often have high `Confidence` despite low `Skill`, leading to attempted actions that result in **Tragic Accidents**.

---

# Layer 7D: The Event Object (The Atom of History)
Every significant change in the world is an Event.

```rust
struct Event {
    id: UUID,
    tick: u64,
    verb: ActionType, // Attack, Give, Eat
    actor: AgentId,
    targets: [AgentId | ObjectId],
    location: GridCell,
    witnesses: [AgentId], // Who saw it happen (Subjective Reality)
    
    // Semantic Data
    tags: [Violent, Romantic, Generous, Taboo],
    intensity: 0-100, // How "memorable" is it?
    cause_event_id: Option<UUID> // Causal chain (Revenge for Event X)
}
```

# Layer 7E: Engine Gaps Filled

## Theory of Mind Updates
How agents learn about others.
- **Observation**: "I saw Grimnar (Actor) Attack (Verb) the Child (Target)."
- **Update**:
  - `SubjectiveReality.Models[Grimnar].Aggression += 5`
  - `SubjectiveReality.Models[Grimnar].Compassion -= 10`
- **Inference**: "Grimnar is dangerous."

## Pain & Decision Weighting
Pain isn't just a stat; it destroys rationality.
```rust
fn calculate_plan_score(plan) {
    let base_score = plan.utility;
    
    // Pain Filter
    if State.Pain > 50 {
        // High pain focuses mind only on immediate relief
        if !plan.soothes_pain() {
            base_score *= 0.1; // Ignore long-term goals
        }
    }
    
    return base_score;
}
```
