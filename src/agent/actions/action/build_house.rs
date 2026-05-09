//! BuildHouse action. Two required materials (wood + stone), higher
//! labor cost than BuildLeanTo. Spawns a labor-gated construction site
//! that becomes a `House` after `HOUSE_LABOR_TICKS` ticks of `Construct`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, RuntimeOp, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::build::{
    HOUSE_LABOR_TICKS, HOUSE_STONE_REQUIRED, HOUSE_WOOD_REQUIRED,
};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::Focus, 0.4),
];

const HOUSE_REQUIREMENTS: &[(Concept, u32)] = &[
    (Concept::Wood, HOUSE_WOOD_REQUIRED),
    (Concept::Stone, HOUSE_STONE_REQUIRED),
];
const HOUSE_PROVIDES: &[Concept] = &[Concept::ShelterProviding, Concept::Safety];

const PLACEMENT_DURATION_TICKS: u32 = 60;

pub static BUILD_HOUSE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::BuildHouse,
    kind: ActionKind::Timed {
        duration_ticks: PLACEMENT_DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 10.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: false,
    start_log: Some("started building house"),
    complete_log: Some("placed house site"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[
        Pattern::SelfContains {
            concept: Concept::Wood,
            quantity: HOUSE_WOOD_REQUIRED,
        },
        Pattern::SelfContains {
            concept: Concept::Stone,
            quantity: HOUSE_STONE_REQUIRED,
        },
    ],
    plan_effects: &[EffectTemplate::SelfNearConcept(Concept::House)],
    plan_consumes: &[
        Pattern::SelfContains {
            concept: Concept::Wood,
            quantity: HOUSE_WOOD_REQUIRED,
        },
        Pattern::SelfContains {
            concept: Concept::Stone,
            quantity: HOUSE_STONE_REQUIRED,
        },
    ],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::RecipeKnown(Concept::House),
    gates: &[
        Gate::InventoryHasQuantity {
            concept: Concept::Wood,
            quantity: HOUSE_WOOD_REQUIRED,
        },
        Gate::InventoryHasQuantity {
            concept: Concept::Stone,
            quantity: HOUSE_STONE_REQUIRED,
        },
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::RemoveFromInventory {
            concept: Concept::Wood,
            quantity: HOUSE_WOOD_REQUIRED,
        },
        RuntimeOp::RemoveFromInventory {
            concept: Concept::Stone,
            quantity: HOUSE_STONE_REQUIRED,
        },
        RuntimeOp::SpawnSite {
            target: Concept::House,
            requirements: HOUSE_REQUIREMENTS,
            initial_items: HOUSE_REQUIREMENTS,
            labor_required: Some(HOUSE_LABOR_TICKS),
        },
    ],
    hooks: Hooks::EMPTY,
    recipe: Some(Recipe {
        concept: Concept::House,
        requirements: HOUSE_REQUIREMENTS,
        provides: HOUSE_PROVIDES,
        build_time_ticks: HOUSE_LABOR_TICKS,
    }),
};
