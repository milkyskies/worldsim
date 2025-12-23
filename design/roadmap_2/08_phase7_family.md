# Phase 7: Family (The Parent)

> **Behavior Goal**: Agents reproduce, children inherit traits from parents, families form across generations.

---

## Dependencies

```
Phase 6: Social
└── Relationships (reproduction requires high affection)
└── Family relationship type

Phase 5: Learning
└── Skills (can be taught parent → child)
└── Knowledge (inherited through teaching)

Phase 3: Memory
└── Childhood memories (formative, high intensity)

Phase 2: Personality
└── Traits (now derived from genes, not random)
└── Drives (reproduction drive)
```

---

## New Systems

### Biology: Genetics
*Reference: [01_biology.md](../01_biology.md) Layer 1*

- **Genome** - collection of gene values per agent
- **Gene types**:
  - Physical: muscle fiber, metabolism, height potential
  - Psychological: baseline anxiety, dopamine receptors, stress sensitivity
- **Inheritance** - child gets mix of parent genes + mutation
- **Polygenic traits** - multiple genes affect one phenotype
- **Pleiotropy** - one gene affects multiple phenotypes

### Biology: Phenotypes
*Reference: [01_biology.md](../01_biology.md) Layer 2*

- **Phenotypes = genes + environment**
- **Physical**: strength, endurance, height, metabolism
- **Psychological**: baseline anxiety, reward sensitivity, aggression tendency
- **Phenotypes shift slowly** based on experiences

### 🔄 Psychology: Calculated Traits
*EXTENDS: Phase 2 personality - NOW DERIVED FROM GENES*

*Reference: [02_psychology.md](../02_psychology.md) Layer 8G*

- **Traits calculated, not assigned**:
  - Neuroticism = anxiety_gene × 0.6 + trauma_count × 4.0
  - Extraversion = dopamine_gene × 0.7 + positive_social_memories × 0.3
  - Openness = processing_speed_gene × 0.5 + experience_diversity × 0.5

### Society: Reproduction
*Reference: [03_society.md](../03_society.md) Reproduction & Family*

- **Requirements**: compatible pair, high affection, reproduction drive
- **Child creation**:
  - Genes combined from parents with mutations
  - Strong initial relationships with parents/siblings
  - Childhood defined by parenting + events

### Biology: Lifecycle Stages
*Reference: [01_biology.md](../01_biology.md) Layer 10*

- **Child (0-12)**: high neuroplasticity (learning bonus), low strength cap, dependent on parent
- **Adult (13-50)**: peak stats, can reproduce
- **Elder (50+)**: constitution degrades, wisdom bonus (teaching), risk of natural death

### Society: Parenting Styles
*Reference: [03_society.md](../03_society.md) Childhood*

- **Harsh** → child neuroticism increases
- **Nurturing** → child agreeableness increases
- **Neglectful** → child trust decreases, self-reliance up
- **Childhood trauma** has outsized personality impact

### Society: Inheritance  
*Reference: [03_society.md](../03_society.md) Generational*

- **Resources** passed to children
- **Status** somewhat inherited
- **Knowledge/skills** taught
- **Family reputation** affects children's standing
- **Surname/lineage** tracking

---

## Test Scenario: "The Dynasty"

1. Two agents with high affection reproduce
2. Child inherits mix of parent genes
3. Child's personality emerges from genes + parenting
4. Child learns skills from parents (teaching bonus)
5. Parents die, child inherits possessions
6. Child reproduces, grandchild continues lineage

---

## What We're NOT Adding Yet

- ❌ Marriage contracts (Phase 9)
- ❌ Family feuds across generations (Phase 11 with culture)
- ❌ Adoption, step-parents (future)

---

**Previous**: [Phase 6: Social](07_phase6_social.md)  
**Next**: [Phase 8: Stress](09_phase8_stress.md)
