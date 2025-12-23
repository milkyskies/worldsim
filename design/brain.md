┌─────────────────────────────────────────────────────────────────────┐
│                           AGENT                                     │
│  ┌──────────┐  ┌──────────┐  ┌─────────────┐  ┌──────────────────┐ │
│  │  Needs   │  │ Emotions │  │ Personality │  │ Memory/Beliefs   │ │
│  │(hunger,  │  │(fear,    │  │(neuroticism,│  │(spatial, episodic│ │
│  │ energy)  │  │ joy)     │  │ extraversion│  │ relationships)   │ │
│  └────┬─────┘  └────┬─────┘  └──────┬──────┘  └────────┬─────────┘ │
│       │             │               │                   │           │
│       ▼             ▼               ▼                   ▼           │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    URGENCY GENERATORS                         │  │
│  │  Each Need/Emotion → Urgency (0-1) × Personality Sensitivity  │  │
│  │                                                                │  │
│  │  hunger_urgency = curve(hunger) × hunger_sensitivity          │  │
│  │  fear_urgency   = fear.intensity × fear_reactivity            │  │
│  │  social_urgency = social_drive × extraversion                 │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    GOAL GENERATOR                             │  │
│  │  High urgency signals become GOALS with priorities            │  │
│  │                                                                │  │
│  │  hunger_urgency=0.7  →  Goal { SatisfyHunger, priority=0.7 }  │  │
│  │  fear_urgency=0.9    →  Goal { GetSafe, priority=0.9 }        │  │
│  │  social_urgency=0.3  →  Goal { Socialize, priority=0.3 }      │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    GOAP PLANNER                               │  │
│  │  Pick highest priority goal. Find action sequence.           │  │
│  │                                                                │  │
│  │  Goal: SatisfyHunger                                          │  │
│  │  Plan: [WalkTo(AppleTree), Harvest, Eat]                      │  │
│  │                                                                │  │
│  │  Uses: WorldState (facts), ActionRegistry (templates)         │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    EXECUTOR                                   │  │
│  │  Execute current plan step. Update CurrentActivity.           │  │
│  │  Report completion → advance plan index.                      │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘