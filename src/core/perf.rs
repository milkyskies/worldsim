//! Live per-system tick timer + F3-style overlay data source.
//!
//! Reads: wall-clock timings emitted by `mark_start` / `mark_end` bracket
//! systems scheduled around each [`PerfBucket`] set.
//! Writes: [`PerfTracker`] (resource) — rolling windows per bucket plus the
//! total-tick window.
//! Upstream: FixedFirst (`perf_tick_begin`), FixedLast (`perf_tick_end`),
//! per-bucket begin/end systems inserted by [`PerfPlugin`].
//! Downstream: `ui::perf_overlay` (egui F3 panel), `headless` (stdout printout +
//! `perf_stats` section of the JSON report).
//!
//! # Why not criterion?
//!
//! Criterion answers "is this microbenchmark faster than last week?" This
//! module answers "which part of the current simulation is eating my tick
//! budget right now?" Both matter; this issue is only the live view.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::app::{FixedFirst, FixedLast};
use bevy::prelude::*;

/// Logical groupings of per-tick work. Each variant is a [`SystemSet`] that
/// an individual Bevy system can opt into via `.in_set(PerfBucket::X)`. A
/// begin/end pair of systems is scheduled around each set and records the
/// wall-clock latency into [`PerfTracker`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerfBucket {
    Perception,
    Memory,
    Psyche,
    Skills,
    Biology,
    Brain,
    Communication,
    Action,
}

impl PerfBucket {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Perception => "perception",
            Self::Memory => "memory",
            Self::Psyche => "psyche",
            Self::Skills => "skills",
            Self::Biology => "biology",
            Self::Brain => "brain",
            Self::Communication => "communication",
            Self::Action => "action",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Perception => 0,
            Self::Memory => 1,
            Self::Psyche => 2,
            Self::Skills => 3,
            Self::Biology => 4,
            Self::Brain => 5,
            Self::Communication => 6,
            Self::Action => 7,
        }
    }

    pub const ALL: [PerfBucket; 8] = [
        Self::Perception,
        Self::Memory,
        Self::Psyche,
        Self::Skills,
        Self::Biology,
        Self::Brain,
        Self::Communication,
        Self::Action,
    ];
}

/// Finer-grained groups inside a parent [`PerfBucket`]. Only the parents
/// that hide something interesting are subdivided — the cheap uniform
/// buckets (skills, biology, psyche, communication) stay flat. Each variant
/// is a [`SystemSet`] that individual systems opt into with
/// `.in_set(PerfSubBucket::X)` *alongside* the parent `.in_set(PerfBucket::Y)`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerfSubBucket {
    // Perception ————————————————————————————————————
    /// N² visibility checks + writing perceptions back to the mind graph.
    /// Usually the heaviest perception cost.
    PerceptionVisual,
    /// Cheap scans: body, tile sensing (water/grass), temperature, hearing,
    /// danger reaction.
    PerceptionSensory,
    /// Social recognition + theory-of-mind updates.
    PerceptionSocial,

    // Memory ————————————————————————————————————————
    /// Working-memory bookkeeping: perception ingest, WM tick, decay,
    /// belief updates from action outcomes.
    MemoryWmTick,
    /// WM → MindGraph consolidation pass.
    MemoryConsolidation,
    /// Draining pending triple mutations into the MindGraph.
    MemoryMindgraphDrain,

    // Brain —————————————————————————————————————————
    /// CNS urgency/drive generation.
    BrainUrgency,
    /// Rational brain A* planner.
    BrainPlanning,
    /// Arbitration between survival / emotional / rational proposals.
    BrainArbitration,
    /// Brain history bookkeeping.
    BrainHistory,

    // Action ————————————————————————————————————————
    /// Starting, ticking, and applying effects of running actions.
    ActionExecution,
    /// World-side mutations: labor accumulation, `becomes`, emitted effects.
    ActionWorldMutation,
}

impl PerfSubBucket {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PerceptionVisual => "visual",
            Self::PerceptionSensory => "sensory",
            Self::PerceptionSocial => "social",
            Self::MemoryWmTick => "wm_tick",
            Self::MemoryConsolidation => "consolidation",
            Self::MemoryMindgraphDrain => "mindgraph_drain",
            Self::BrainUrgency => "urgency",
            Self::BrainPlanning => "planning",
            Self::BrainArbitration => "arbitration",
            Self::BrainHistory => "history",
            Self::ActionExecution => "execution",
            Self::ActionWorldMutation => "world_mutation",
        }
    }

    pub const fn parent(self) -> PerfBucket {
        match self {
            Self::PerceptionVisual | Self::PerceptionSensory | Self::PerceptionSocial => {
                PerfBucket::Perception
            }
            Self::MemoryWmTick | Self::MemoryConsolidation | Self::MemoryMindgraphDrain => {
                PerfBucket::Memory
            }
            Self::BrainUrgency
            | Self::BrainPlanning
            | Self::BrainArbitration
            | Self::BrainHistory => PerfBucket::Brain,
            Self::ActionExecution | Self::ActionWorldMutation => PerfBucket::Action,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::PerceptionVisual => 0,
            Self::PerceptionSensory => 1,
            Self::PerceptionSocial => 2,
            Self::MemoryWmTick => 3,
            Self::MemoryConsolidation => 4,
            Self::MemoryMindgraphDrain => 5,
            Self::BrainUrgency => 6,
            Self::BrainPlanning => 7,
            Self::BrainArbitration => 8,
            Self::BrainHistory => 9,
            Self::ActionExecution => 10,
            Self::ActionWorldMutation => 11,
        }
    }

    pub const ALL: [PerfSubBucket; 12] = [
        Self::PerceptionVisual,
        Self::PerceptionSensory,
        Self::PerceptionSocial,
        Self::MemoryWmTick,
        Self::MemoryConsolidation,
        Self::MemoryMindgraphDrain,
        Self::BrainUrgency,
        Self::BrainPlanning,
        Self::BrainArbitration,
        Self::BrainHistory,
        Self::ActionExecution,
        Self::ActionWorldMutation,
    ];
}

/// Fixed-capacity ring buffer of per-tick durations. Keeps an incremental
/// running sum so `avg()` is O(1) instead of scanning the whole buffer — on a
/// tool that measures hot paths, paying 120 `Duration` additions every snapshot
/// would be self-defeating.
#[derive(Debug, Clone)]
struct RollingWindow {
    buf: VecDeque<Duration>,
    capacity: usize,
    sum: Duration,
}

impl RollingWindow {
    fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
            sum: Duration::ZERO,
        }
    }

    fn push(&mut self, d: Duration) {
        if self.buf.len() == self.capacity
            && let Some(oldest) = self.buf.pop_front()
        {
            self.sum = self.sum.saturating_sub(oldest);
        }
        self.buf.push_back(d);
        self.sum += d;
    }

    fn avg(&self) -> Duration {
        if self.buf.is_empty() {
            return Duration::ZERO;
        }
        self.sum / self.buf.len() as u32
    }

    fn max(&self) -> Duration {
        self.buf.iter().copied().max().unwrap_or_default()
    }

    fn len(&self) -> usize {
        self.buf.len()
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

struct BucketData {
    window: RollingWindow,
    in_flight: Option<Instant>,
}

impl BucketData {
    fn new(capacity: usize) -> Self {
        Self {
            window: RollingWindow::new(capacity),
            in_flight: None,
        }
    }
}

/// Rolling wall-clock measurements for each [`PerfBucket`] plus the total
/// FixedMain cycle. Numbers are latency, not CPU time, so parallel execution
/// may push summed bucket latencies above 100% of the tick — the sorted
/// ranking is still honest even when the percentages don't.
#[derive(Resource)]
pub struct PerfTracker {
    buckets: [BucketData; 8],
    sub_buckets: [BucketData; 12],
    total: RollingWindow,
    total_in_flight: Option<Instant>,
}

impl PerfTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            buckets: std::array::from_fn(|_| BucketData::new(capacity)),
            sub_buckets: std::array::from_fn(|_| BucketData::new(capacity)),
            total: RollingWindow::new(capacity),
            total_in_flight: None,
        }
    }

    pub fn mark_start(&mut self, bucket: PerfBucket) {
        self.buckets[bucket.index()].in_flight = Some(Instant::now());
    }

    pub fn mark_end(&mut self, bucket: PerfBucket) {
        // If `mark_end` fires without a matching `mark_start`, drop it
        // silently. Bevy's .before/.after scheduling should keep the pair
        // balanced; a missed pair would mean a broken constraint, not a
        // runtime bug worth panicking over.
        if let Some(start) = self.buckets[bucket.index()].in_flight.take() {
            self.buckets[bucket.index()].window.push(start.elapsed());
        }
    }

    pub fn mark_sub_start(&mut self, bucket: PerfSubBucket) {
        self.sub_buckets[bucket.index()].in_flight = Some(Instant::now());
    }

    pub fn mark_sub_end(&mut self, bucket: PerfSubBucket) {
        if let Some(start) = self.sub_buckets[bucket.index()].in_flight.take() {
            self.sub_buckets[bucket.index()]
                .window
                .push(start.elapsed());
        }
    }

    pub fn mark_tick_begin(&mut self) {
        self.total_in_flight = Some(Instant::now());
    }

    pub fn mark_tick_end(&mut self) {
        if let Some(start) = self.total_in_flight.take() {
            self.total.push(start.elapsed());
        }
    }

    pub fn samples(&self) -> usize {
        self.total.len()
    }

    pub fn capacity(&self) -> usize {
        self.total.capacity()
    }

    /// Snapshot sorted by avg descending — the UI wants the heaviest bucket
    /// on top without having to re-sort client-side. Sub-buckets are sorted
    /// independently (descending within each parent).
    pub fn snapshot(&self) -> PerfSnapshot {
        let total_avg = self.total.avg();
        let total_max = self.total.max();
        let tick_budget_us = total_avg.as_secs_f64() * 1_000_000.0;

        let mut buckets: Vec<BucketStats> = PerfBucket::ALL
            .iter()
            .map(|bucket| {
                let data = &self.buckets[bucket.index()];
                let avg_us = data.window.avg().as_secs_f64() * 1_000_000.0;
                let max_us = data.window.max().as_secs_f64() * 1_000_000.0;
                let pct_of_tick = if tick_budget_us > 0.0 {
                    (avg_us / tick_budget_us) * 100.0
                } else {
                    0.0
                };
                BucketStats {
                    name: bucket.label(),
                    avg_us,
                    max_us,
                    pct_of_tick,
                }
            })
            .collect();
        buckets.sort_by(|a, b| {
            b.avg_us
                .partial_cmp(&a.avg_us)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut sub_buckets: Vec<SubBucketStats> = PerfSubBucket::ALL
            .iter()
            .map(|sub| {
                let data = &self.sub_buckets[sub.index()];
                let avg_us = data.window.avg().as_secs_f64() * 1_000_000.0;
                let max_us = data.window.max().as_secs_f64() * 1_000_000.0;
                let pct_of_tick = if tick_budget_us > 0.0 {
                    (avg_us / tick_budget_us) * 100.0
                } else {
                    0.0
                };
                SubBucketStats {
                    parent: sub.parent().label(),
                    name: sub.label(),
                    avg_us,
                    max_us,
                    pct_of_tick,
                }
            })
            .collect();
        // Sort: primary by parent's rank in `buckets`, secondary by avg desc
        // within the parent. That way consumers can iterate sub_buckets and
        // naturally get parent-grouped, heaviest-first rows without doing
        // their own grouping.
        let parent_rank: std::collections::HashMap<&'static str, usize> = buckets
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name, i))
            .collect();
        sub_buckets.sort_by(|a, b| {
            let ra = parent_rank.get(a.parent).copied().unwrap_or(usize::MAX);
            let rb = parent_rank.get(b.parent).copied().unwrap_or(usize::MAX);
            ra.cmp(&rb).then_with(|| {
                b.avg_us
                    .partial_cmp(&a.avg_us)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        PerfSnapshot {
            total_avg_us: total_avg.as_secs_f64() * 1_000_000.0,
            total_max_us: total_max.as_secs_f64() * 1_000_000.0,
            samples: self.total.len(),
            buckets,
            sub_buckets,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BucketStats {
    pub name: &'static str,
    pub avg_us: f64,
    pub max_us: f64,
    pub pct_of_tick: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubBucketStats {
    pub parent: &'static str,
    pub name: &'static str,
    pub avg_us: f64,
    pub max_us: f64,
    pub pct_of_tick: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PerfSnapshot {
    pub total_avg_us: f64,
    pub total_max_us: f64,
    pub samples: usize,
    pub buckets: Vec<BucketStats>,
    pub sub_buckets: Vec<SubBucketStats>,
}

impl PerfSnapshot {
    /// Sum of all bucket avg latencies — useful as a signal for how much
    /// parallelism the scheduler got. When `sum_bucket_avg_us ≈
    /// total_avg_us`, buckets are mostly serial; when sum >> total, they
    /// ran concurrently.
    pub fn sum_bucket_avg_us(&self) -> f64 {
        self.buckets.iter().map(|b| b.avg_us).sum()
    }

    pub fn format_table(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "tick avg: {:>7.1}µs   max: {:>7.1}µs   window: {} samples\n",
            self.total_avg_us, self.total_max_us, self.samples
        ));
        out.push_str(&format!(
            "Σ bucket avg: {:>7.1}µs  (higher than tick avg → parallel execution)\n",
            self.sum_bucket_avg_us(),
        ));
        out.push_str(&format!(
            "{:<20} {:>10} {:>10} {:>8}\n",
            "system", "avg µs", "max µs", "% tick"
        ));
        for parent in &self.buckets {
            out.push_str(&format!(
                "{:<20} {:>10.1} {:>10.1} {:>7.1}%\n",
                parent.name, parent.avg_us, parent.max_us, parent.pct_of_tick
            ));
            for child in self.sub_buckets.iter().filter(|s| s.parent == parent.name) {
                out.push_str(&format!(
                    "  └ {:<16} {:>10.1} {:>10.1} {:>7.1}%\n",
                    child.name, child.avg_us, child.max_us, child.pct_of_tick
                ));
            }
        }
        out
    }
}

/// Windowed-only: F3 toggles the live overlay. Headless ignores this.
#[derive(Resource, Default)]
pub struct PerfOverlayEnabled(pub bool);

// ═══════════════════════════════════════════════════════════════════════════
// PLUGIN
// ═══════════════════════════════════════════════════════════════════════════

/// Default rolling-window length. At 60 ticks/second that's two seconds of
/// history — enough to smooth out jitter without hiding a real regression.
pub const DEFAULT_WINDOW: usize = 120;

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PerfTracker::new(DEFAULT_WINDOW))
            .init_resource::<PerfOverlayEnabled>()
            .add_systems(FixedFirst, |mut t: ResMut<PerfTracker>| t.mark_tick_begin())
            .add_systems(FixedLast, |mut t: ResMut<PerfTracker>| t.mark_tick_end());

        for bucket in PerfBucket::ALL {
            let begin = move |mut t: ResMut<PerfTracker>| t.mark_start(bucket);
            let end = move |mut t: ResMut<PerfTracker>| t.mark_end(bucket);
            app.add_systems(FixedUpdate, begin.before(bucket))
                .add_systems(FixedUpdate, end.after(bucket));
        }

        for sub in PerfSubBucket::ALL {
            let begin = move |mut t: ResMut<PerfTracker>| t.mark_sub_start(sub);
            let end = move |mut t: ResMut<PerfTracker>| t.mark_sub_end(sub);
            app.add_systems(FixedUpdate, begin.before(sub))
                .add_systems(FixedUpdate, end.after(sub));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_drops_oldest_at_capacity() {
        let mut w = RollingWindow::new(3);
        w.push(Duration::from_micros(10));
        w.push(Duration::from_micros(20));
        w.push(Duration::from_micros(30));
        assert_eq!(w.len(), 3);
        w.push(Duration::from_micros(40));
        assert_eq!(w.len(), 3);
        assert_eq!(w.buf.front(), Some(&Duration::from_micros(20)));
        assert_eq!(w.buf.back(), Some(&Duration::from_micros(40)));
    }

    #[test]
    fn rolling_window_avg_and_max() {
        let mut w = RollingWindow::new(4);
        for us in [10, 20, 30, 40] {
            w.push(Duration::from_micros(us));
        }
        assert_eq!(w.avg(), Duration::from_micros(25));
        assert_eq!(w.max(), Duration::from_micros(40));
    }

    #[test]
    fn rolling_window_incremental_sum_stays_correct_across_evictions() {
        let mut w = RollingWindow::new(3);
        for us in [10u64, 20, 30, 40, 50] {
            w.push(Duration::from_micros(us));
        }
        // Buffer now holds 30, 40, 50 → avg 40
        assert_eq!(w.avg(), Duration::from_micros(40));
        assert_eq!(w.max(), Duration::from_micros(50));
    }

    #[test]
    fn rolling_window_empty_returns_zero() {
        let w = RollingWindow::new(3);
        assert_eq!(w.avg(), Duration::ZERO);
        assert_eq!(w.max(), Duration::ZERO);
    }

    #[test]
    fn mark_start_end_records_into_window() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_start(PerfBucket::Perception);
        std::thread::sleep(Duration::from_millis(1));
        tracker.mark_end(PerfBucket::Perception);
        let snapshot = tracker.snapshot();
        let perception = snapshot
            .buckets
            .iter()
            .find(|b| b.name == "perception")
            .unwrap();
        assert!(perception.max_us >= 1000.0);
    }

    #[test]
    fn mark_end_without_start_is_noop() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_end(PerfBucket::Memory);
        let snapshot = tracker.snapshot();
        let memory = snapshot
            .buckets
            .iter()
            .find(|b| b.name == "memory")
            .unwrap();
        assert_eq!(memory.avg_us, 0.0);
        assert_eq!(memory.max_us, 0.0);
    }

    #[test]
    fn snapshot_sorts_by_avg_descending() {
        let mut tracker = PerfTracker::new(10);
        let samples = [
            (PerfBucket::Perception, 50u64),
            (PerfBucket::Memory, 200),
            (PerfBucket::Psyche, 30),
            (PerfBucket::Skills, 10),
            (PerfBucket::Biology, 80),
            (PerfBucket::Brain, 500),
            (PerfBucket::Communication, 20),
            (PerfBucket::Action, 150),
        ];
        for (bucket, us) in samples {
            tracker.mark_start(bucket);
            // Simulate a fixed elapsed time by overriding the in-flight
            // timestamp after-the-fact isn't possible; instead we push
            // directly into the window via the public API and a long
            // enough sleep would be too flaky. Use a tight loop that
            // pushes a known value through the real path.
            std::thread::sleep(Duration::from_micros(us));
            tracker.mark_end(bucket);
        }
        tracker.mark_tick_begin();
        std::thread::sleep(Duration::from_micros(1000));
        tracker.mark_tick_end();

        let snap = tracker.snapshot();
        assert_eq!(snap.buckets.len(), PerfBucket::ALL.len());
        for pair in snap.buckets.windows(2) {
            assert!(pair[0].avg_us >= pair[1].avg_us);
        }
    }

    #[test]
    fn snapshot_pct_of_tick_is_zero_when_total_is_empty() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_start(PerfBucket::Brain);
        std::thread::sleep(Duration::from_millis(1));
        tracker.mark_end(PerfBucket::Brain);
        let snap = tracker.snapshot();
        for row in &snap.buckets {
            assert_eq!(row.pct_of_tick, 0.0);
        }
    }

    #[test]
    fn format_table_includes_header_and_all_buckets() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_tick_begin();
        std::thread::sleep(Duration::from_micros(100));
        tracker.mark_tick_end();
        for bucket in PerfBucket::ALL {
            tracker.mark_start(bucket);
            tracker.mark_end(bucket);
        }
        let table = tracker.snapshot().format_table();
        assert!(table.contains("tick avg:"));
        assert!(table.contains("% tick"));
        for bucket in PerfBucket::ALL {
            assert!(
                table.contains(bucket.label()),
                "missing bucket {} in table:\n{}",
                bucket.label(),
                table
            );
        }
    }

    #[test]
    fn capacity_is_reported_consistently() {
        let tracker = PerfTracker::new(42);
        assert_eq!(tracker.capacity(), 42);
    }
}
