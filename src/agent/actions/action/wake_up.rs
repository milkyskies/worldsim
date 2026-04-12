use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind};
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};

pub struct WakeUpAction;

impl Action for WakeUpAction {
    fn action_type(&self) -> ActionType {
        ActionType::WakeUp
    }

    fn name(&self) -> &'static str {
        "Wake Up"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Rest,
            TargetSelector::InPlace,
            IntensityPolicy::Fixed(0.3),
            Intent::Fatigue,
        )
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed { duration_ticks: 30 }
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::HasTrait,
            Value::Concept(Concept::Awake),
        )]
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // WakeUp only needs FullBody to preempt Sleep. Focus is NOT
        // claimed here because cognitive channels scale to zero during
        // sleep (alertness = 0), and requiring Focus would make WakeUp
        // inadmissible — the agent needs to wake up to regain focus,
        // not have focus to wake up (#462).
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::FullBody, 0.4)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Stationary: you don't wake up mid-walk. The transition is
        // committed to one spot — this also mutexes WakeUp with any
        // Moving action someone might propose during the 30-tick
        // window (previously the gate was missing and a Walk could
        // silently run alongside WakeUp).
        Some(Posture::Stationary)
    }
}
