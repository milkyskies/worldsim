# Layer 4D: Relationships

Each relationship is a separate tracked object between two people.

## Relationship Structure
```
Relationship {
  person_A: id
  person_B: id
  trust: 0-100
  respect: 0-100
  affection: 0-100
  power_balance: -100 to +100 (who has leverage)
  shared_memories: [memory_ids]
  relationship_type: family / friend / rival / lover / acquaintance
}
```

## Relationship Dynamics
- **Trust**: builds slowly through reliable behavior, destroyed quickly by betrayal
- **Respect**: based on competence, skill, accomplishments
- **Affection**: emotional bond, friendship or romance
- **Power balance**: who needs whom more, who has authority

## Relationship Updates
- Every interaction creates potential memory for both parties
- Memory's emotional valence affects relationship values
- Positive interactions: increase trust/affection slowly
- Negative interactions: decrease trust/affection quickly (asymmetric)
- Observed behavior affects relationship (saw them help someone → respect up)

## Relationship Types
- **Family**: high initial affection/trust, can deteriorate but hard to sever
- **Friend**: mutual affection + shared positive memories
- **Rival**: competition for status/resources, mixture of respect and antagonism
- **Lover**: high affection + reproduction drive, can become family
- **Mentor/student**: respect-based, knowledge transfer

## Social Influence
- Trusted people's beliefs carry more weight
- High-affection people get priority in decisions
- Respected people become leaders
- Power imbalance affects negotiation outcomes

---

# Emergent Systems

## Culture System

Culture is NOT a separate object - it emerges from shared beliefs + social enforcement.

**Cultural Formation:**
- Geographic cluster of people in contact
- Beliefs spread through teaching and gossip
- Beliefs shared by >60% of group = cultural norm
- Violating cultural norms = larger social penalties

**Cultural Mechanics:**
- **Taboos**: beliefs with high emotional intensity ("don't eat pork")
- **Laws**: beliefs enforced by leadership with punishment ("theft = exile")
- **Rituals**: repeated actions reinforcing beliefs (funeral rites)
- **Values**: which personality traits earn respect (warrior culture values bravery)
- **Technology level**: sum of knowledge possessed by group members
- **Traditions**: behaviors that started randomly, gained meaning over time

**Cultural Evolution:**
- Successful groups (survive better) → their beliefs spread to neighbors
- Failed groups (die out) → their beliefs disappear
- Cultural drift: beliefs mutate slightly as passed person-to-person
- Cultural splitting: isolated groups develop divergent beliefs
- Cultural exchange: trade/travel spreads beliefs between groups

**No explicit culture object needed.** Just track:
- Belief overlap between geographic clusters
- Social network density (who interacts with whom)
- Shared language (enables belief transmission)

## Language System

**Simple implementation:**
```
Language {
  id: unique_id
  speakers: [person_ids]
  mutual_intelligibility: {other_language_id: 0-100%}
}
```

**Language mechanics:**
- Born into group → learn that language (knowledge)
- Communication only works between shared language speakers
- Languages diverge over time when groups separate geographically
- Learning new language = skill that improves with exposure time
- Bilingual individuals enable cultural exchange

**Language effects:**
- **Trade**: requires common language
- **Cultural transmission**: beliefs spread within language groups more easily
- **Conflict**: language barrier makes dehumanization easier
- **Identity**: shared language creates group cohesion
- **Knowledge preservation**: language death = potential knowledge loss

## Reproduction & Family System

**Reproduction mechanics:**
```
if compatible_pair AND 
   relationship.affection > threshold AND 
   reproduction_drive satisfied AND
   decide_to_reproduce:
  create_child()
```

**Child creation:**
```
Child {
  genes: combine_parent_genes(parent1, parent2, mutation_rate)
  phenotypes: calculated_at_birth + childhood_environment_modifiers
  family_bonds: strong initial relationships with parents/siblings
  childhood_memories: shaped by parenting + events + culture
}
```

**Childhood as formative period:**
- Children learn beliefs from parents (high trust = easy adoption)
- Parenting style affects personality development:
  - Harsh → increases neuroticism, might increase discipline or rebellion
  - Nurturing → increases agreeableness, secure attachment style
  - Neglectful → reduces trust, increases self-reliance or anxiety
- Childhood trauma = outsized impact (high emotional intensity memories)
- Skills learned from parents through teaching system
- Cultural norms absorbed from community

**Family bonds:**
- Strong baseline relationships (parent-child: trust+affection high)
- Siblings: affection baseline, can become rivals or allies
- Bonds can deteriorate (abusive parent → child hates them)
- Family reputation affects children's social standing
- Inheritance: resources, status, knowledge passed down

**Generational effects:**
- Genetic trait propagation (strong parents → strong children more likely)
- Genetic diversity through mutation and mixing
- Cultural transmission (beliefs pass reliably parent→child)
- Family feuds (grudges inherited, "my father's killer's son")
- Dynasties (families accumulate power/knowledge over generations)
- Surname/lineage tracking (family identity persists)

## Social Dynamics (Emergent)

**From relationships + culture:**

**Gossip system:**
- Person A witnesses event involving person B
- Person A creates memory
- Person A tells person C (if relationship.trust > threshold)
- Person C creates memory with source = A
- Trust in A affects belief strength in the story
- Stories mutate as they spread (details lost/embellished)

**Faction formation:**
- People with similar beliefs naturally cluster
- Shared enemies create alliances
- Shared goals create cooperation
- Leadership emerges (high respect + charisma + success)
- Factional identity becomes part of personal identity

**Status hierarchies:**
- Respect aggregates into reputation scores
- High reputation = higher status
- Status affects resource access
- Status affects mating opportunities
- Status affects whose beliefs spread faster

**Conflict escalation:**
- Personal grudge → family feud → tribal war
- Dehumanization through belief formation ("they're monsters")
- Revenge cycles (violence creates trauma → desire for revenge)
- Peace through relationship building or exhaustion

## Historical Emergence

History is NOT stored separately - it's the sum of everyone's memories + artifacts.

**Event → Story transformation:**
- Multiple people witness event, create memories
- Details differ based on personality bias (anxious person remembers it as scarier)
- Story spreads through gossip, mutates in transmission
- Important events with many witnesses create strong cultural memories
- Emotionally intense events become legends

**Legend formation:**
```
if event.importance > threshold AND 
   person.respect > threshold AND
   multiple_witnesses:
  
  Legend {
    core_event: (what actually happened)
    told_versions: [] (variations based on teller's personality)
    embellishment_factor: (increases over time)
    cultural_meaning: (what the culture believes it represents)
  }
```

**Historical memory:**
- Old people tell stories to young (oral tradition)
- Bias based on teller's personality colors the story
- Written records preserve events (if literacy exists)
- But who writes history matters (victor's perspective)
- Ruins and artifacts are physical evidence of past civilizations
- Archaeological interpretation depends on finder's beliefs

**Civilization rise/fall:**
- No explicit "civilization" object
- Emerges from: population density + shared beliefs + trade networks + technology level
- Rises through: successful resource management + knowledge accumulation + cooperation
- Falls through: resource depletion + internal conflict + external pressure + knowledge loss
- Successor civilizations can rediscover lost knowledge through ruins

**Nations as identity:**
- Shared language + culture + territory + history = national identity
- National identity becomes part of personal belief system
- Wars between nations are wars between belief systems
- National collapse = crisis of individual identity for members

---

# Layer 8C: The "Web of Debt" (Social Currency)
Society runs on leverage, not just "Friendship".

## Mechanic
- **Ledger**: Every agent tracks "Favors Owed" to them.
- **Types**:
    - *Small Favor*: Consumable (expires).
    - *Life Debt*: Permanent, transferable to kin.
    - *Blood Debt*: Negative debt (vendetta).
- **Leverage**: A weak agent can command a strong agent if they hold a Life Debt.
- **Tragedy**: Agents are forced to act against their Principles to pay Debts.

---

# Layer 8C-2: Contracts & Promises
Exteding Debt to support employment and recurring obligations.

## Contract Structure
A contract is a formal agreement with conditions, not just a static number.

```
Contract {
  id: UUID
  parties: [EmployerID, WorkerID]
  
  // The "Job"
  obligation_action: Chop_Wood / Guard_Location / Craft_Item
  obligation_target: Sawmill_ID / Castle_Gate / Forge_ID
  
  // The "Schedule" (Logic Query)
  schedule: {
    type: Daily
    start_hour: 6 (Dawn)
    end_hour: 18 (Dusk)
    days: [Mon, Tue, Wed, Thu, Fri]
  }
  
  // The "Pay"
  payment_amount: 5 Gold
  payment_interval: Per_Day / Per_Item_Produced
  
  // State
  breach_count: 0
  active: true
}
```

## Breach & Enforcement
Every simulated hour, the system checks active contracts:
1.  **Check**: Is it work hours? (`CurrentTime` inside `Schedule`)
2.  **Verify**: Is Worker performing `obligation_action` at `obligation_target`?
3.  **Result**:
    - **Yes**: Add to `pending_payment`.
    - **No**: Increment `breach_count`.
4.  **Consequence**:
    - If `breach_count > Tolerance`: Employer gains `Casus_Belli` (Just cause to fire/punish).
    - If `pending_payment` not paid: Worker gains `Casus_Belli` (Just cause to quit/steal).

## Emergent Employment
- **"9-to-5"**: Emerges because Employers want work done when Vision is high (Day).
- **"Weekend"**: Emerges if Religions mandate rest on Day 7 (Culture), so contracts exclude it.
- **"Slavery"**: A Contract where Payment = 0 and Breach_Consequence = Violence.

## Uniformity & Fairness (Emergent Standardization)
Agents have a need for **Fairness**, creating pressure for standardized contracts.
1.  **Comparison**: Agents compare `My_Ratio = Pay / Effort` vs `Peer_Ratio = Pay / Effort`.
2.  **Inequity**: If `My_Ratio < Peer_Ratio`, `Drive:Fairness` drops.
3.  **Reaction**: Agent demands matching terms or quits.
4.  **Result**: Employers use **Role Templates** (Standardized Contracts) to prevent workforce rebellion.


---

# Layer 8D: Information Fidelity (Rumors & Gossip)
Information rots as it travels.

## Mechanic
- **Message Object**: Contains `Fact` + `Fidelity` (0.0-1.0) + `Mutation_Count`.
- **Transmission**: `New_Fidelity = Old_Fidelity * Sender_Communication * Receiver_Intelligence`.
- **Hallucination**: When `Fidelity < Threshold`, the system fills gaps with data matching the Receiver's *Biases*.
    - "The King is sick" → (low fidelity) → "The King was poisoned" (if Receiver hates the Queen).

---

# Layer 8E: Emergent Macro-Structures (The Hallucination of State)
Kingdoms and Economies don't exist; they are aggregated behaviors.

**Politics (The Protection Racket):**
- **Warlord**: High Strength/Aggression agent offers "Protection" for "Tribute" (Food).
- **Vassal Chains**: Weak agents serve Warlords to tax even weaker agents.
- **Borders**: The visual radius where a Warlord can effectively enforce tax collection.
- **Civil War**: Occurs naturally when a Vassal calculates they have more power than their Liege.

**Economy (Time Utility):**
- No global prices. Value = Labor Time saved.
- **Barter**: Smith trades a Hoe (took 10h to make) for Potatoes (would take Smith 100h to grow).
- **Currency**: Emerges when agents agree on a token (Gold) to store Labor Value efficiently.
- **Inflation**: If Gold becomes common, Labor Value of Gold drops → Prices rise.

**Ethnicity (The Founder Effect):**
- Visual history of lineage, not a "Race" stat.
- **Drift**: Isolated groups homogenize cosmetic traits (e.g., "The Crimson Folk" all have red eyes because the founder did).
- **Racism**: Pattern recognition error (Belief System) where traits are linked to behaviors ("Red eyes = Thieves").
