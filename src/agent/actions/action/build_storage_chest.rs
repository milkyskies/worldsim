//! BuildStorageChest action. Spawns a labor-gated construction site that
//! becomes a `StorageChest` after `STORAGE_CHEST_LABOR_TICKS` ticks of
//! `Construct`. Same shape as `BuildLeanTo`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, RuntimeOp, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::build::{STORAGE_CHEST_LABOR_TICKS, STORAGE_CHEST_WOOD_REQUIRED};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::Focus, 0.4),
];

const STORAGE_CHEST_REQUIREMENTS: &[(Concept, u32)] =
    &[(Concept::Wood, STORAGE_CHEST_WOOD_REQUIRED)];
const STORAGE_CHEST_PROVIDES: &[Concept] = &[Concept::Safety];

const PLACEMENT_DURATION_TICKS: u32 = 30;

pub static BUILD_STORAGE_CHEST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::BuildStorageChest,
    kind: ActionKind::Timed {
        duration_ticks: PLACEMENT_DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 5.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: false,
    start_log: Some("started building storage chest"),
    complete_log: Some("placed storage chest site"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: STORAGE_CHEST_WOOD_REQUIRED,
    }],
    plan_effects: &[EffectTemplate::SelfNearConcept(Concept::StorageChest)],
    plan_consumes: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: STORAGE_CHEST_WOOD_REQUIRED,
    }],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::RecipeKnown(Concept::StorageChest),
    gates: &[Gate::InventoryHasQuantity {
        concept: Concept::Wood,
        quantity: STORAGE_CHEST_WOOD_REQUIRED,
    }],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::RemoveFromInventory {
            concept: Concept::Wood,
            quantity: STORAGE_CHEST_WOOD_REQUIRED,
        },
        RuntimeOp::SpawnSite {
            target: Concept::StorageChest,
            requirements: STORAGE_CHEST_REQUIREMENTS,
            initial_items: STORAGE_CHEST_REQUIREMENTS,
            labor_required: Some(STORAGE_CHEST_LABOR_TICKS),
        },
    ],
    hooks: Hooks::EMPTY,
    recipe: Some(Recipe {
        concept: Concept::StorageChest,
        requirements: STORAGE_CHEST_REQUIREMENTS,
        provides: STORAGE_CHEST_PROVIDES,
        build_time_ticks: STORAGE_CHEST_LABOR_TICKS,
    }),
};
