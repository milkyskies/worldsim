# Layer 1: Genetics

## Gene System
- Each person has a genome (collection of gene values)
- Genes are inherited from parents with potential mutations
- Genes don't directly create traits - they influence phenotypes through expression
- Multiple genes can affect one phenotype (polygenic traits)
- One gene can affect multiple phenotypes (pleiotropy)

## Gene → Phenotype Process
```
Genes → (influenced by environment) → Phenotypes → Traits
```

Example genes:
- `muscle_fiber_type`, `metabolism_rate`, `growth_hormone_production`
- `baseline_serotonin`, `stress_hormone_sensitivity`, `dopamine_receptors`
- `immune_system_strength`, `pain_receptor_density`

---

# Layer 2: Phenotypes

Phenotypes are measurable biological/psychological characteristics that result from gene expression + environment.

## Physical Phenotypes
Calculated from relevant genes + environmental modifiers:
- **Strength potential**: muscle fiber genes + nutrition + training
- **Endurance**: cardiovascular genes + conditioning
- **Height**: growth genes + childhood nutrition
- **Metabolism rate**: metabolic genes + activity level
- **Immune system**: immune genes + disease exposure
- **Pain tolerance**: pain receptor genes + experiences
- **Lifespan potential**: longevity genes + lifestyle
- **Facial features**: appearance genes (affects social interactions)

## Psychological Phenotypes
Calculated from relevant genes + environmental modifiers:
- **Baseline anxiety level**: stress response genes + childhood experiences
- **Reward sensitivity**: dopamine system genes + reinforcement history
- **Stress response threshold**: cortisol genes + trauma exposure
- **Processing speed**: neural efficiency genes + stimulation
- **Memory capacity**: hippocampus genes + practice
- **Aggression tendency**: testosterone/serotonin genes + social learning
- **Empathy capacity**: mirror neuron genes + attachment experiences

## Environmental Influence on Phenotypes
Phenotypes are ranges, not fixed values:
- Same genes + good nutrition = stronger outcome
- Same genes + trauma = higher anxiety outcome
- Phenotypes can shift slowly over lifetime based on experiences

---

# Layer 3A: Body Parts System

Each person has a hierarchy of body parts, each tracked individually.

## Body Part Structure
```
Body {
  head: {
    skull, brain, left_eye, right_eye, nose, jaw, tongue
  },
  torso: {
    spine, heart, lungs, liver, stomach, intestines, ribs
  },
  left_arm: {
    upper_arm, elbow, forearm, 
    left_hand: {palm, thumb, 4 fingers}
  },
  right_arm: {...},
  left_leg: {
    thigh, knee, shin,
    left_foot: {ankle, 5 toes}
  },
  right_leg: {...}
}
```

## Body Part Properties
Each part tracks:
- `max_hp`: from constitution phenotype + part type
- `current_hp`: current health
- `injuries[]`: active injuries (cut, bruise, fracture, burn, infection, missing)
- `scar_tissue`: accumulated permanent damage
- `pain_level`: 0-100, affects decisions
- `function_rate`: 0-100%, how well it works

## Injury System
**Injury types and effects:**
- **Cuts/bruises**: heal over time, cause pain
- **Fractures**: require immobilization, longer healing
- **Burns**: painful, can scar, risk of infection
- **Infections**: spread to connected body parts, can be fatal
- **Missing parts**: permanent disability, phantom pain

**Injury propagation:**
- Spine damage → paralysis below injury point
- Brain damage → cognitive impairment, personality changes
- Heart/lung damage → reduced stamina phenotype
- Missing hand → can't use two-handed tools
- Infected wound → spreads through bloodstream

**Healing:**
- Healing rate: constitution phenotype + age + nutrition
- Some injuries heal fully, others leave scars (reduced max_hp)
- Scars accumulate, reducing body part effectiveness over time
- Missing parts don't regenerate

## Physical Capabilities
Derived from body part function rates:
- Walking speed: leg function
- Manipulation: hand function
- Vision: eye function
- Stamina: heart/lung function
- Combat ability: arm/leg function + pain level

---

# Layer 10: Biology Gaps Filled

## Lifecycle Stages
- **Child (0-12)**: 
  - High `Neuroplasticity` (Learning bonus).
  - Low `Strength` cap.
  - Dependent on `Parent` for resources.
- **Adult (13-50)**:
  - Peak physical stats.
  - Full reproductive capability.
- **Elder (50+)**:
  - `Constitution` degrades over time.
  - Gained `Wisdom` (XP bonus to others).
  - Risk of `Natural Death` (Heart failure check each year).

## Inventory & Capacity
- **Carrying Capacity**: `Strength * 10kg`.
- **Encumbrance**: Exceeding capacity reduces `Movement Speed` and increases `Fatigue`.
- **Hands**: 2 slots. Required for tool use. (e.g., Bow requires 2 hands).
