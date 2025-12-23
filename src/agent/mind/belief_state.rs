use crate::agent::brains::thinking::TriplePattern;
use crate::agent::mind::knowledge::MindGraph;

/// Represents the agent's subjective belief state for planning.
/// Wraps the MindGraph to provide probability estimates for patterns.
pub struct BeliefState<'a> {
    pub mind: &'a MindGraph,
}

impl<'a> BeliefState<'a> {
    pub fn new(mind: &'a MindGraph) -> Self {
        Self { mind }
    }

    /// Check if a pattern is satisfied in the MindGraph.
    /// Returns the confidence of the first matching triple, or 0.0 if no match.
    pub fn pattern_confidence(&self, pattern: &TriplePattern) -> f32 {
        self.mind
            .query(
                pattern.subject.as_ref(),
                pattern.predicate,
                pattern.object.as_ref(),
            )
            .first()
            .map(|t| t.meta.confidence)
            .unwrap_or(0.0)
    }

    /// Check if any triple matches the pattern (boolean).
    pub fn pattern_exists(&self, pattern: &TriplePattern) -> bool {
        !self
            .mind
            .query(
                pattern.subject.as_ref(),
                pattern.predicate,
                pattern.object.as_ref(),
            )
            .is_empty()
    }

    /// Get the average confidence across all matching triples.
    pub fn pattern_aggregate_confidence(&self, pattern: &TriplePattern) -> f32 {
        let matches = self.mind.query(
            pattern.subject.as_ref(),
            pattern.predicate,
            pattern.object.as_ref(),
        );
        if matches.is_empty() {
            return 0.0;
        }
        matches.iter().map(|t| t.meta.confidence).sum::<f32>() / matches.len() as f32
    }
}
