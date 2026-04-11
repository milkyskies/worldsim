//! Sleep actions - sleeping and waking up.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::constants::actions::sleep::{
    ALERTNESS_PER_SEC, BASE_COST, GLUCOSE_DRAIN_PER_SEC, STAMINA_PER_SEC,
};

pub struct SleepAction;

impl Action for SleepAction {
    fn action_type(&self) -> ActionType {
        ActionType::Sleep
    }

    fn name(&self) -> &'static str {
        "Sleep"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    // Planning: Sleep leads to full stamina
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Stamina,
            Value::Int(100),
        )]
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Sleep declares only FullBody. Blocking every other action while
        // asleep is enforced by an explicit short-circuit in `start_actions`
        // — spreading 1.0 across every active channel would refuse Sleep on
        // any species whose per-channel capacity doesn't happen to match the
        // human default (a wolf's 0.4 Manipulation can never "satisfy"
        // Manipulation 1.0 through the admission math, so it couldn't even
        // start sleeping).
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::FullBody, 1.0)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Stationary)
    }

    // Sleep uses the default `interruptible = true`. WakeUp has to preempt
    // Sleep through the normal channel-admission path (both touch FullBody),
    // and `interruptible = false` deadlocks that: WakeUp could never free
    // the FullBody slot. Protection against *other* actions casually
    // evicting Sleep lives at a higher layer — the `start_actions`
    // short-circuit in `execution.rs` rejects every non-WakeUp admission
    // while Sleep is active, so interruptibility here only matters for the
    // WakeUp transition itself.

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            glucose_drain_per_sec: GLUCOSE_DRAIN_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
            // Dreams feed the mind. Sleep gently raises curiosity so
            // the agent wakes with fresh novelty-seeking urge.
            curiosity_per_sec: 0.01,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("falling asleep")
    }
}
