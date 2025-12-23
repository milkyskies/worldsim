# Phase 11: Culture (The Tribe)

> **Behavior Goal**: Groups develop distinct identities through shared beliefs, language, and customs.

---

## Dependencies

```
Phase 10: Politics
└── Factions (cultural groups)
└── Territory (geographic isolation)

Phase 7: Family
└── Generational transmission
└── Parenting (beliefs passed to children)

Phase 6: Social
└── Relationships (who talks to whom)
└── Trust (whose info is believed)

Phase 5: Learning
└── Teaching (knowledge transmission)
└── Knowledge (can be lost)

Phase 3: Memory
└── Beliefs (shared beliefs = culture)
└── Events (become legends)
```

---

## New Systems

### Society: Language
*Reference: [03_society.md](../03_society.md) Language System*

- **Languages** - each has speakers, mutual intelligibility scores
- **Mechanics**:
  - Born into group → learn that language
  - Communication requires shared language
  - Learning new language = skill with exposure
- **Divergence** - separated groups develop different languages
- **Effects**:
  - Trade requires common language
  - Beliefs spread easier within language group
  - Barrier makes dehumanization easier
  - Language death = knowledge loss

### Society: Culture (Emergent)
*Reference: [03_society.md](../03_society.md) Culture System*

- **NOT a separate object** - emerges from shared beliefs
- **Cultural norm** = belief held by >60% of geographic group
- **Mechanics**:
  - **Taboos**: high-intensity beliefs ("don't eat pork")
  - **Laws**: beliefs enforced with punishment ("theft = exile")
  - **Rituals**: repeated reinforcing actions (funeral rites)
  - **Values**: which traits earn respect (warrior culture = bravery)
- **Cultural evolution**:
  - Successful groups spread beliefs to neighbors
  - Failed groups → beliefs die
  - Drift as beliefs mutate person-to-person

### Society: Gossip & Information Fidelity
*Reference: [03_society.md](../03_society.md) Layer 8D*

- **Information rots as it travels**
- **Message = fact + fidelity (0-1) + mutation count**
- **Transmission**: new_fidelity = old × sender_skill × receiver_intelligence
- **Hallucination**: when fidelity < 0.5, gaps filled with receiver's biases
  - "King is sick" → "King was poisoned" (if receiver hates queen)

### 🔄 Psychology: False Memory
*EXTENDS: Phase 3 memory*

*Reference: [02_psychology.md](../02_psychology.md) Layer 8G*

- **Low-fidelity gossip becomes memory**
- **Stored as if true** even if distorted
- **Biases twist content** - hated subjects become villains

### Society: Legends
*Reference: [03_society.md](../03_society.md) Historical Emergence*

- **Event → Story transformation**
- **Multiple witnesses** with different personality biases
- **Stories mutate** through gossip
- **Legend formed** when high-importance + high-respect person + many witnesses
- **Embellishment increases** over time

### Society: History
*Reference: [03_society.md](../03_society.md) Historical Emergence*

- **History = sum of memories + artifacts**
- **Oral tradition** - old tell stories to young
- **Bias colors stories** based on teller personality
- **Written records** - if literacy exists, preserve events
- **Victor writes history**

### UI: Gossip Clouds
*Reference: [05_ui.md](../05_ui.md) Layer 9B*

- **Floating text over settlements**
- **Visual coding**:
  - Bold/crisp = high fidelity facts
  - Faded/wobbly = low fidelity rumors
- **Player can verify** - pay spies to check rumors

---

## Test Scenario: "The Telephone Game"

1. Agent A tells B "I saw a wolf"
2. B tells C (fidelity drops, mutation occurs)
3. C hears "I saw a monster"
4. C tells D "There's a dragon!"
5. D panics, spreads further distorted news

---

## Test Scenario: "The Taboo"

1. Group of agents share belief "eating pork is wrong"
2. New agent joins, eats pork
3. Social penalties from group
4. Agent either conforms or leaves
5. Isolated groups develop different taboos

---

## What We're NOT Adding Yet

- ❌ Religion as formal structure (emerges from beliefs + rituals)
- ❌ Written language mechanics (just existence for now)

---

**Previous**: [Phase 10: Politics](11_phase10_politics.md)  
**Next**: [Phase 12: Emergence](13_phase12_emergence.md)
