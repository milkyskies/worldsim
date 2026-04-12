//! Groom action — self-care when nothing else is pressing.
//!
//! Grooming is what animals do when they have nothing else to do. It
//! fills the space between meaningful behaviours: a cat cleans its
//! paws, a bird preens its feathers, a human fidgets with their hair
//! or tidies a sleeve. It's not emergency recovery (that's Sleep) and
//! it's not active rest (that's Rest). It's the natural expression of
//! "I'm safe, I'm fed, and I have a moment to myself."
//!
//! Mapping: Emotional brain proposes Groom at a very low urgency as
//! the no-drive baseline. Any real drive outbids it.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{ActionPrimitive, Behavior, IntensityPolicy, TargetSelector};
use crate::agent::actions::registry::{Action, ActionKind};

pub struct GroomAction;

impl Action for GroomAction {
    fn action_type(&self) -> ActionType {
        ActionType::Groom
    }

    fn name(&self) -> &'static str {
        "Groom"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Manipulate,
            TargetSelector::InPlace,
            IntensityPolicy::Ambient,
            crate::agent::actions::motor::Intent::Goal,
        )
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Grooming uses hands (or the species equivalent). Mild
        // manipulation so it doesn't block a parallel Eat on the
        // Consumption channel — an agent could groom and eat at once.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.3)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Posture-agnostic. Dogs scratch while walking, birds preen
        // while hopping, humans fidget with a sleeve mid-stride. The
        // action is about self-care, not about being planted.
        None
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("grooming")
    }
}
