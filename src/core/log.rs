use bevy::prelude::*;
use chrono::Local;
use std::collections::{HashSet, VecDeque};

// ═══════════════════════════════════════════════════════════════════════════
// LOG CATEGORIES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum LogCategory {
    Brain,       // Brain decisions (Survival/Rational/Emotional)
    Action,      // Actions executed (eat, harvest, walk)
    Plan,        // Plan lifecycle (created, completed, invalidated)
    Perception,  // What agents perceive
    Event,       // World events (spawns, deaths)
    Performance, // Performance metrics (FPS, Frame Time, profiling)
    Debug,       // Verbose debug info
}

impl LogCategory {
    /// All categories for initialization
    pub fn all() -> HashSet<LogCategory> {
        HashSet::from([
            LogCategory::Brain,
            LogCategory::Action,
            LogCategory::Plan,
            LogCategory::Perception,
            LogCategory::Event,
            LogCategory::Performance,
            LogCategory::Debug,
        ])
    }

    /// Default enabled categories (all enabled for now)
    pub fn defaults() -> HashSet<LogCategory> {
        HashSet::from([
            LogCategory::Brain,
            LogCategory::Action,
            LogCategory::Plan,
            LogCategory::Perception,
            LogCategory::Event,
            // LogCategory::Performance,
            LogCategory::Debug,
        ])
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            LogCategory::Brain => "[Brain]",
            LogCategory::Action => "[Action]",
            LogCategory::Plan => "[Plan]",
            LogCategory::Perception => "[Perception]",
            LogCategory::Event => "[Event]",
            LogCategory::Performance => "[Performance]",
            LogCategory::Debug => "[Debug]",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LOG ENTRY
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub category: LogCategory,
    pub message: String,
    pub count: usize,
    /// Optional entity this log entry is associated with (for filtering)
    pub entity: Option<Entity>,
}

// ═══════════════════════════════════════════════════════════════════════════
// GAME LOG RESOURCE
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct GameLog {
    #[reflect(ignore)]
    pub entries: VecDeque<LogEntry>,
    pub max_entries: usize,
    #[reflect(ignore)]
    pub enabled: HashSet<LogCategory>,
    /// Filter to show only logs from specific entities (empty = show all)
    #[reflect(ignore)]
    pub entity_filter: HashSet<Entity>,
}

impl Default for GameLog {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries: 500,
            enabled: LogCategory::defaults(),
            entity_filter: HashSet::new(),
        }
    }
}

impl GameLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            enabled: LogCategory::defaults(),
            entity_filter: HashSet::new(),
        }
    }

    /// Toggle a category on/off
    pub fn toggle(&mut self, category: LogCategory) {
        if self.enabled.contains(&category) {
            self.enabled.remove(&category);
        } else {
            self.enabled.insert(category);
        }
    }

    /// Check if category is enabled
    pub fn is_enabled(&self, category: LogCategory) -> bool {
        self.enabled.contains(&category)
    }

    // ─── Entity filter management ───

    /// Set filter to show only logs from a specific entity
    pub fn filter_by_entity(&mut self, entity: Entity) {
        self.entity_filter.clear();
        self.entity_filter.insert(entity);
    }

    /// Add an entity to the filter (shows logs from multiple entities)
    pub fn add_entity_to_filter(&mut self, entity: Entity) {
        self.entity_filter.insert(entity);
    }

    /// Remove an entity from the filter
    pub fn remove_entity_from_filter(&mut self, entity: Entity) {
        self.entity_filter.remove(&entity);
    }

    /// Clear entity filter (show all entities)
    pub fn clear_entity_filter(&mut self) {
        self.entity_filter.clear();
    }

    /// Check if filtering by entities
    pub fn has_entity_filter(&self) -> bool {
        !self.entity_filter.is_empty()
    }

    // ─── Core logging ───

    fn log_internal(&mut self, category: LogCategory, message: String, entity: Option<Entity>) {
        if !self.is_enabled(category) {
            return;
        }

        let now = Local::now();
        let timestamp = now.format("%H:%M:%S").to_string();

        // Check for deduplication
        if let Some(last_entry) = self.entries.back_mut() {
            // Check if category, message, and entity match
            if last_entry.category == category
                && last_entry.message == message
                && last_entry.entity == entity
            {
                last_entry.count += 1;
                // Move cursor up and clear line to overwrite previous log
                // \x1B[1A = Move up 1 line
                // \x1B[2K = Clear entire line
                print!("\x1B[1A\x1B[2K");
                println!(
                    "[{}] {} {} (x{})",
                    timestamp,
                    category.prefix(),
                    message,
                    last_entry.count
                );
                return;
            }
        }

        // Print to console
        println!("[{}] {} {}", timestamp, category.prefix(), message);

        // Store entry
        self.entries.push_back(LogEntry {
            timestamp,
            category,
            message,
            count: 1,
            entity,
        });

        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
    }

    /// Raw log with category (no entity)
    pub fn log(&mut self, category: LogCategory, message: impl Into<String>) {
        self.log_internal(category, message.into(), None);
    }

    /// Raw log with category and entity
    pub fn log_for_entity(
        &mut self,
        category: LogCategory,
        message: impl Into<String>,
        entity: Entity,
    ) {
        self.log_internal(category, message.into(), Some(entity));
    }

    // ─── Structured logging methods ───

    /// Log brain decision: "[Agent] BRAIN_TYPE won → Action (reasoning)"
    pub fn brain(
        &mut self,
        agent: &str,
        brain_type: &str,
        action: &str,
        reasoning: &str,
        entity: Option<Entity>,
    ) {
        self.log_internal(
            LogCategory::Brain,
            format!(
                "[{}] {} won → {} ({})",
                agent, brain_type, action, reasoning
            ),
            entity,
        );
    }

    /// Log action execution: "[Agent] did Action" or "[Agent] did Action → result"
    pub fn action(
        &mut self,
        agent: &str,
        action: &str,
        result: Option<&str>,
        entity: Option<Entity>,
    ) {
        let msg = match result {
            Some(r) => format!("[{}] {} → {}", agent, action, r),
            None => format!("[{}] {}", agent, action),
        };
        self.log_internal(LogCategory::Action, msg, entity);
    }

    /// Log plan lifecycle: "[Agent] Plan: status"
    pub fn plan(&mut self, agent: &str, status: &str, entity: Option<Entity>) {
        self.log_internal(
            LogCategory::Plan,
            format!("[{}] Plan: {}", agent, status),
            entity,
        );
    }

    /// Log perception: "[Agent] saw/heard/noticed something"
    pub fn perception(&mut self, agent: &str, perception: &str, entity: Option<Entity>) {
        self.log_internal(
            LogCategory::Perception,
            format!("[{}] {}", agent, perception),
            entity,
        );
    }

    /// Log world event: "Event happened"
    pub fn event(&mut self, event: &str) {
        self.log_internal(LogCategory::Event, event.to_string(), None);
    }

    /// Log performance info
    pub fn performance(&mut self, message: impl Into<String>) {
        self.log_internal(LogCategory::Performance, message.into(), None);
    }

    /// Log debug info
    pub fn log_debug(&mut self, message: impl Into<String>) {
        self.log_internal(LogCategory::Debug, message.into(), None);
    }

    // ─── Filtered view for UI ───

    /// Get entries filtered by currently enabled categories and entity filter
    pub fn visible_entries(&self) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| self.enabled.contains(&e.category))
            .filter(|e| {
                // If no entity filter, show all; otherwise only show matching entities
                self.entity_filter.is_empty()
                    || e.entity.is_none()
                    || e.entity
                        .map(|ent| self.entity_filter.contains(&ent))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Get all entries (for UI that does its own filtering)
    pub fn all_entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }
}
