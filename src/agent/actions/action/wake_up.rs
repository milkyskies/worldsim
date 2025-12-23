use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};

pub struct WakeUpAction;

impl Action for WakeUpAction {
    fn action_type(&self) -> ActionType {
        ActionType::WakeUp
    }

    fn name(&self) -> &'static str {
        "Wake Up"
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

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            alertness_per_sec: 100.0,
            ..Default::default()
        }
    }
}
