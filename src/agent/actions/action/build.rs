//! Build action — construct a campfire from wood in inventory.
//!
//! Build is uninterruptible: a half-built campfire is not something to drop
//! because a smaller urgency edged in. On completion, spawns a construction
//! site pre-stocked with the materials the agent just consumed — for a
//! single-agent build, the next `becomes_system` pass transforms it into
//! the finished entity.
//!
//! The recipe data (materials, build time, trait provides) is declared via
//! [`Recipe`] on the definition, and culture seeding auto-derives the
//! corresponding MindGraph triples — no parallel declaration in `culture.rs`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, RuntimeOp, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::build::{CAMPFIRE_DURATION_TICKS, CAMPFIRE_WOOD_REQUIRED};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::Focus, 0.4),
];

const CAMPFIRE_REQUIREMENTS: &[(Concept, u32)] = &[(Concept::Wood, CAMPFIRE_WOOD_REQUIRED)];
const CAMPFIRE_PROVIDES: &[Concept] = &[Concept::Warmth, Concept::Safety, Concept::Light];

pub static BUILD_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Build,
    name: "Build",
    kind: ActionKind::Timed {
        duration_ticks: CAMPFIRE_DURATION_TICKS,
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
    start_log: Some("started building"),
    complete_log: Some("built campfire"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: CAMPFIRE_WOOD_REQUIRED,
    }],
    plan_effects: &[EffectTemplate::SelfNearConcept(Concept::Campfire)],
    plan_consumes: &[Pattern::SelfContains {
        concept: Concept::Wood,
        quantity: CAMPFIRE_WOOD_REQUIRED,
    }],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::RecipeKnown(Concept::Campfire),
    gates: &[Gate::InventoryHasQuantity {
        concept: Concept::Wood,
        quantity: CAMPFIRE_WOOD_REQUIRED,
    }],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::RemoveFromInventory {
            concept: Concept::Wood,
            quantity: CAMPFIRE_WOOD_REQUIRED,
        },
        RuntimeOp::SpawnSite {
            target: Concept::Campfire,
            requirements: CAMPFIRE_REQUIREMENTS,
            initial_items: CAMPFIRE_REQUIREMENTS,
            labor_required: None,
        },
    ],
    hooks: Hooks::EMPTY,
    recipe: Some(Recipe {
        concept: Concept::Campfire,
        requirements: CAMPFIRE_REQUIREMENTS,
        provides: CAMPFIRE_PROVIDES,
        build_time_ticks: CAMPFIRE_DURATION_TICKS,
    }),
};
