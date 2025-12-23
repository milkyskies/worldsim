# Scenario: The Rigid Boss (Emergent Employment)

## The Setup
Two agents with opposing personalities trying to cooperate.

### Agent A: "The Boss" (Grimnar)
- **Role**: Owner of the Sawmill.
- **Traits**: 
  - `Rigidity: 90` (Needs routine).
  - `Stauts Drive: 80` (Wants to be respected).
- **Goal**: Produce Planks (to gain Status).

### Agent B: "The Employee" (Flick)
- **Role**: Unemployed (High Hunger).
- **Traits**:
  - `Rigidity: 10` (Chaotic, hates rules).
  - `Autonomy Drive: 80` (Hates being told what to do).
- **Goal**: Get Food (Survival).

---

## The Engagement (Day 1)

1.  **The Deal**:
    - Grimnar needs labor. Flick needs food.
    - Grimnar proposes **Contract A**: "Chop wood from Dawn(6) to Dusk(18). Pay: 5 Gold."
    - *Why Dawn-to-Dusk?* Grimnar projects his need for order onto the contract.
    - Flick accepts (Hunger overrides Autonomy).

2.  **The Friction**:
    - **Day 2, 08:30 AM**: Flick arrives late. "Whatever, I'm here now."
    - **Grimnar's Reaction**:
        - `Stress` spikes (Pattern Violation).
        - `Relationship[Flick].Respect` drops ("He makes me feel unsafe").
        - Grimnar scolds Flick (Interaction: `Demean`, Tag: `Dominance`).
    - **Flick's Reaction**:
        - `State.Stress` spikes (Autonomy violation).
        - `Relationship[Grimnar].Affection` drops.

3.  **The Breaking Point (Day 5)**:
    - Flick is late again.
    - Grimnar fires him (Breach of Contract). "I'd rather lose money than deal with this chaos."
    - Flick steals a saw on the way out (Revenge for `Demean` memory).

---

## The Outcome (Emergence)
- **Grimnar**: Hires a new worker with higher `Conscientiousness`.
- **Flick**: Becomes a thief (low barrier to entry, high autonomy).
- **The Sawmill**: Becomes a place of strict order ("The 9-to-5 Shop") because only rigid people survive there.
