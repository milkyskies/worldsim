//! Per-metric causal breakdown: computes `(source, signed_rate)` contributors
//! for glucose, stamina, hydration, and stomach so callers can explain *why*
//! a metric is moving right now.
//!
//! Reads: ActiveActions, PhysicalNeeds, ActivityConfig, ActionRegistry, SpeciesProfile, CurrentActivity
//! Writes: nothing (returns a Vec)
//! Upstream: testing::world::print_why (text dump), core::field_logger (JSON inline breakdown)
//! Downstream: headless inspection output, JSONL field logger

use bevy::prelude::*;

use crate::agent::actions::{ActionRegistry, ActiveActions};
use crate::agent::activity::{ActivityConfig, CurrentActivity};
use crate::agent::body::needs::PhysicalNeeds;

/// Which metric the contributor breakdown is being computed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributionKind {
    Glucose,
    Stamina,
    Hydration,
    Stomach,
}

/// One contributor to a metric: a human-readable source name plus its signed
/// per-second rate contribution.
#[derive(Debug, Clone)]
pub struct Contribution {
    pub source: String,
    pub rate: f32,
}

/// Compute the current contributor list for `kind` on `agent`. Each entry is
/// a `(source, signed_rate)` pair; negative rates drain the metric, positive
/// rates add to it. Returns an empty vector when no contributors are active.
pub fn compute_contributions(
    world: &World,
    agent: Entity,
    kind: ContributionKind,
) -> Vec<Contribution> {
    let mut contribs: Vec<Contribution> = Vec::new();
    let cfg = world.get_resource::<ActivityConfig>();

    match kind {
        ContributionKind::Glucose => glucose_contributions(world, agent, cfg, &mut contribs),
        ContributionKind::Stamina => stamina_contributions(world, agent, &mut contribs),
        ContributionKind::Hydration => hydration_contributions(world, agent, cfg, &mut contribs),
        ContributionKind::Stomach => stomach_contributions(world, agent, &mut contribs),
    }

    contribs
}

/// Sum over every contribution's rate. The "net per-second change" — positive
/// when the metric is growing, negative when it's shrinking.
pub fn net_rate(contribs: &[Contribution]) -> f32 {
    contribs.iter().map(|c| c.rate).sum()
}

fn glucose_contributions(
    world: &World,
    agent: Entity,
    cfg: Option<&ActivityConfig>,
    out: &mut Vec<Contribution>,
) {
    use crate::agent::actions::registry::ActionKind;
    use crate::agent::body::effort::{self, DEFAULT_BODY_MASS, compute_action_cost};
    use crate::agent::body::species::SpeciesProfile;
    use crate::agent::movement::effective_intensity;

    if let Some(cfg) = cfg {
        let bmr = cfg.base.effects.glucose_drain;
        if bmr != 0.0 {
            out.push(Contribution {
                source: "BMR (base metabolic rate)".into(),
                rate: -bmr,
            });
        }
    }
    let (Some(active), Some(reg)) = (
        world.get::<ActiveActions>(agent),
        world.get_resource::<ActionRegistry>(),
    ) else {
        return;
    };
    let body_mass = world
        .get::<SpeciesProfile>(agent)
        .map(|s| s.mass_kg)
        .unwrap_or(DEFAULT_BODY_MASS);
    let stamina = world
        .get::<PhysicalNeeds>(agent)
        .map(|p| p.stamina.clone())
        .unwrap_or_default();

    for state in active.iter() {
        let Some(action) = reg.get(state.action_type) else {
            continue;
        };
        let primitive = action.motor_primitive();
        let intensity =
            if matches!(action.kind(), ActionKind::Movement) && state.locomotion_intensity > 0.0 {
                effective_intensity(state.locomotion_intensity, &stamina)
            } else {
                action.default_behavior().intensity.resolve()
            };
        let profile = primitive.effort_profile().scaled(intensity);
        let cost = compute_action_cost(&profile, body_mass);
        if cost.energy != 0.0 {
            let reserves = world
                .get::<PhysicalNeeds>(agent)
                .map(|p| p.metabolism.reserves)
                .unwrap_or(0.0);
            let gluc_frac = effort::effective_glucose_fraction(profile.peak_intensity(), reserves);
            out.push(Contribution {
                source: format!("{:?}", state.action_type),
                rate: -cost.energy * gluc_frac,
            });
        }
    }
}

fn stamina_contributions(world: &World, agent: Entity, out: &mut Vec<Contribution>) {
    use crate::agent::actions::registry::ActionKind;
    use crate::agent::body::effort::{DEFAULT_BODY_MASS, compute_action_cost};
    use crate::agent::body::species::SpeciesProfile;
    use crate::agent::movement::effective_intensity;

    let (Some(active), Some(reg)) = (
        world.get::<ActiveActions>(agent),
        world.get_resource::<ActionRegistry>(),
    ) else {
        return;
    };
    let body_mass = world
        .get::<SpeciesProfile>(agent)
        .map(|s| s.mass_kg)
        .unwrap_or(DEFAULT_BODY_MASS);
    let stamina = world
        .get::<PhysicalNeeds>(agent)
        .map(|p| p.stamina.clone())
        .unwrap_or_default();

    for state in active.iter() {
        let Some(action) = reg.get(state.action_type) else {
            continue;
        };
        let primitive = action.motor_primitive();
        let intensity =
            if matches!(action.kind(), ActionKind::Movement) && state.locomotion_intensity > 0.0 {
                effective_intensity(state.locomotion_intensity, &stamina)
            } else {
                action.default_behavior().intensity.resolve()
            };
        let profile = primitive.effort_profile().scaled(intensity);
        let cost = compute_action_cost(&profile, body_mass);
        if cost.aerobic_drain != 0.0 {
            out.push(Contribution {
                source: format!("{:?}", state.action_type),
                rate: -cost.aerobic_drain,
            });
        }
    }
}

fn hydration_contributions(
    world: &World,
    agent: Entity,
    cfg: Option<&ActivityConfig>,
    out: &mut Vec<Contribution>,
) {
    let Some(cfg) = cfg else {
        return;
    };
    let base = cfg.base.effects.hydration_change;
    if base != 0.0 {
        out.push(Contribution {
            source: "baseline".into(),
            rate: base,
        });
    }
    if let Some(activity) = world.get::<CurrentActivity>(agent) {
        let activity_delta = cfg.get(activity).effects.hydration_change;
        if activity_delta != 0.0 {
            out.push(Contribution {
                source: format!("{:?}", activity),
                rate: activity_delta,
            });
        }
    }
}

fn stomach_contributions(world: &World, agent: Entity, out: &mut Vec<Contribution>) {
    use crate::agent::body::metabolism::{DIGEST_CARB_RATE, DIGEST_FAT_RATE};

    if let Some(needs) = world.get::<PhysicalNeeds>(agent) {
        let m = &needs.metabolism;
        if m.stomach_carbs > 0.0 {
            out.push(Contribution {
                source: "digestion: carbs → glucose".into(),
                rate: -DIGEST_CARB_RATE.min(m.stomach_carbs),
            });
        }
        if m.stomach_fat > 0.0 {
            out.push(Contribution {
                source: "digestion: fat → reserves".into(),
                rate: -DIGEST_FAT_RATE.min(m.stomach_fat),
            });
        }
    }
    let (Some(active), Some(reg)) = (
        world.get::<ActiveActions>(agent),
        world.get_resource::<ActionRegistry>(),
    ) else {
        return;
    };
    for state in active.iter() {
        let Some(action) = reg.get(state.action_type) else {
            continue;
        };
        let rate = action.runtime_effects().stomach_carbs_per_sec;
        if rate != 0.0 {
            out.push(Contribution {
                source: format!("{:?}: carbs in", state.action_type),
                rate,
            });
        }
    }
}
