// Bevy systems routinely use Query tuples and many-argument signatures, which
// trip these lints constantly. The Bevy book explicitly recommends silencing
// them at the crate root.
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

pub mod agent;
pub mod constants;
pub mod core;
pub mod testing;
pub mod ui;
pub mod world;
