use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{ActionPrimitive, Behavior, IntensityPolicy, TargetSelector};
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
            crate::agent::actions::motor::Intent::Fatigue,
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
        // WakeUp is an exclusive transition — eyes opening, body
        // stretching, the mind re-booting from sleep. It needs to
        // mutex with every other action for its 30-tick duration:
        //
        // - FullBody 0.4 keeps the Sleep→WakeUp preemption path
        //   working (Sleep holds FullBody 1.0, WakeUp's 0.4 pushes
        //   total to 1.4 → hard conflict → Sleep is interruptible
        //   post-#352 → preempted → WakeUp admits).
        // - Focus 1.0 blocks Observe and any other Focus
        //   user from running in parallel. The user saw WakeUp and
        //   Observe both in `active_actions` during the transition
        //   and flipping in the UI every frame — a waking agent
        //   isn't also scanning the room, they're re-orienting.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::FullBody, 0.4),
            ChannelUsage::new(Channel::Focus, 1.0),
        ];
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
