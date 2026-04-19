//! Action definitions — one `pub static FOO_DEF: ActionDefinition` per file.
//!
//! Every action is pure data. Irreducibly custom logic (metabolism-aware
//! on_complete, target-contingent preconditions, picker algorithms) lives
//! as named helper functions referenced through
//! [`Hooks`](super::definition::Hooks).

pub mod attack;
pub mod bite;
pub mod build;
pub mod construct;
pub mod converse;
pub mod deposit;
pub mod devour;
pub mod drink;
pub mod eat;
pub mod explore;
pub mod flee;
pub mod graze;
pub mod harvest;
pub mod idle;
pub mod initiate_conversation;
pub mod look_for;
pub mod observe;
pub mod rest;
pub mod search_utils;
pub mod sleep;
pub mod take;
pub mod wake_up;
pub mod walk;
pub mod wander;
pub mod warm_up;

pub use attack::ATTACK_DEF;
pub use bite::BITE_DEF;
pub use build::BUILD_DEF;
pub use construct::CONSTRUCT_DEF;
pub use converse::CONVERSE_DEF;
pub use deposit::DEPOSIT_DEF;
pub use devour::DEVOUR_DEF;
pub use drink::DRINK_DEF;
pub use eat::EAT_DEF;
pub use explore::EXPLORE_DEF;
pub use flee::FLEE_DEF;
pub use graze::GRAZE_DEF;
pub use harvest::HARVEST_DEF;
pub use idle::IDLE_DEF;
pub use initiate_conversation::INITIATE_CONVERSATION_DEF;
pub use look_for::LOOK_FOR_DEF;
pub use observe::OBSERVE_DEF;
pub use rest::REST_DEF;
pub use sleep::SLEEP_DEF;
pub use take::TAKE_DEF;
pub use wake_up::WAKE_UP_DEF;
pub use walk::WALK_DEF;
pub use wander::WANDER_DEF;
pub use warm_up::WARM_UP_DEF;
