// Re-export GameLog from core so that all references to ui::hud::GameLog
// point to the same type as CorePlugin inserts (crate::core::log::GameLog).
pub use crate::core::log::GameLog;
