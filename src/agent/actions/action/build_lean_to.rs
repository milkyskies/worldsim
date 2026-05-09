//! BuildLeanTo action. Spawns a labor-gated construction site that
//! becomes a `LeanTo` after `LEAN_TO_LABOR_TICKS` ticks of `Construct`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, RuntimeOp, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::build::{LEAN_TO_LABOR_TICKS, LEAN_TO_WOOD_REQUIRED};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::Focus, 0.4),
];

const LEAN_TO_REQUIREMENTS: &[(Concept, u32)] = &[(Concept::Wood, LEAN_TO_WOOD_REQUIRED)];
const LEAN_TO_PROVIDES: &[Concept] = &[Concept::ShelterProviding, Concept::Safety];

/// Placement step duration. Labor on the site is accounted separately
/// by `Construct`.
const PLACEMENT_DURATION_TICKS: u32 = 30;

pub static BUILD_LEAN_TO_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::BuildLeanTo,
    kind: ActionKind::Timed {
        duration_ticks: PLACEMENT_DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 6.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: false,
    start_log: Some("started building lean-to"),
    complete_log: Some("placed lean-to site"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: LEAN_TO_WOOD_REQUIRED,
    }],
    plan_effects: &[EffectTemplate::SelfNearConcept(Concept::LeanTo)],
    plan_consumes: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: LEAN_TO_WOOD_REQUIRED,
    }],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::RecipeKnown(Concept::LeanTo),
    gates: &[Gate::InventoryHasQuantity {
        concept: Concept::Wood,
        quantity: LEAN_TO_WOOD_REQUIRED,
    }],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::RemoveFromInventory {
            concept: Concept::Wood,
            quantity: LEAN_TO_WOOD_REQUIRED,
        },
        RuntimeOp::SpawnSite {
            target: Concept::LeanTo,
            requirements: LEAN_TO_REQUIREMENTS,
            initial_items: LEAN_TO_REQUIREMENTS,
            labor_required: Some(LEAN_TO_LABOR_TICKS),
        },
    ],
    hooks: Hooks::EMPTY,
    recipe: Some(Recipe {
        concept: Concept::LeanTo,
        requirements: LEAN_TO_REQUIREMENTS,
        provides: LEAN_TO_PROVIDES,
        build_time_ticks: LEAN_TO_LABOR_TICKS,
    }),
};
