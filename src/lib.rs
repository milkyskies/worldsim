// Bevy systems routinely use Query tuples and many-argument signatures, which
// trip these lints constantly. The Bevy book explicitly recommends silencing
// them at the crate root.
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

pub mod agent;
pub mod cli;
pub mod constants;
pub mod core;
pub mod eyes;
pub mod headless;
pub mod injuries;
pub mod markings;
pub mod menu;
pub mod outline;
pub mod palette;
pub mod particles;
pub mod silhouette;
pub mod testing;
pub mod ui;
pub mod world;
