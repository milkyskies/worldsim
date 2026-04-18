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

use std::collections::{HashMap, VecDeque};
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

/// Fixed-capacity ring buffer of per-tick durations.
#[derive(Debug, Clone)]
struct RollingWindow {
    buf: VecDeque<Duration>,
    capacity: usize,
}

impl RollingWindow {
    fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, d: Duration) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(d);
    }

    fn avg(&self) -> Duration {
        if self.buf.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.buf.iter().sum();
        total / self.buf.len() as u32
    }

    fn max(&self) -> Duration {
        self.buf.iter().copied().max().unwrap_or_default()
    }

    fn len(&self) -> usize {
        self.buf.len()
    }
}

struct BucketData {
    window: RollingWindow,
    /// In-flight measurement: Some between `mark_start` and `mark_end`.
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
    buckets: HashMap<&'static str, BucketData>,
    total: RollingWindow,
    total_in_flight: Option<Instant>,
    capacity: usize,
}

impl PerfTracker {
    pub fn new(capacity: usize) -> Self {
        let mut buckets = HashMap::new();
        for bucket in PerfBucket::ALL {
            buckets.insert(bucket.label(), BucketData::new(capacity));
        }
        Self {
            buckets,
            total: RollingWindow::new(capacity),
            total_in_flight: None,
            capacity,
        }
    }

    pub fn mark_start(&mut self, bucket: &'static str) {
        if let Some(data) = self.buckets.get_mut(bucket) {
            data.in_flight = Some(Instant::now());
        }
    }

    pub fn mark_end(&mut self, bucket: &'static str) {
        if let Some(data) = self.buckets.get_mut(bucket)
            && let Some(start) = data.in_flight.take()
        {
            data.window.push(start.elapsed());
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

    /// Number of ticks held in the rolling window for the total.
    pub fn samples(&self) -> usize {
        self.total.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Snapshot of the current rolling averages, sorted by avg descending.
    pub fn snapshot(&self) -> PerfSnapshot {
        let total_avg = self.total.avg();
        let total_max = self.total.max();
        let tick_budget_us = total_avg.as_secs_f64() * 1_000_000.0;

        let mut rows: Vec<BucketStats> = PerfBucket::ALL
            .iter()
            .map(|bucket| {
                let label = bucket.label();
                let data = &self.buckets[label];
                let avg_us = data.window.avg().as_secs_f64() * 1_000_000.0;
                let max_us = data.window.max().as_secs_f64() * 1_000_000.0;
                let pct_of_tick = if tick_budget_us > 0.0 {
                    (avg_us / tick_budget_us) * 100.0
                } else {
                    0.0
                };
                BucketStats {
                    name: label,
                    avg_us,
                    max_us,
                    pct_of_tick,
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            b.avg_us
                .partial_cmp(&a.avg_us)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        PerfSnapshot {
            total_avg_us: total_avg.as_secs_f64() * 1_000_000.0,
            total_max_us: total_max.as_secs_f64() * 1_000_000.0,
            samples: self.total.len(),
            buckets: rows,
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
pub struct PerfSnapshot {
    pub total_avg_us: f64,
    pub total_max_us: f64,
    pub samples: usize,
    pub buckets: Vec<BucketStats>,
}

impl PerfSnapshot {
    /// Sum of all bucket avg latencies — useful as a signal for how much
    /// parallelism the scheduler got. When `sum_bucket_avg_us ≈
    /// total_avg_us`, buckets are mostly serial; when sum >> total, they
    /// ran concurrently.
    pub fn sum_bucket_avg_us(&self) -> f64 {
        self.buckets.iter().map(|b| b.avg_us).sum()
    }

    /// Format the snapshot as a plain-text table, Minecraft-F3 style. One
    /// header line plus one row per bucket, sorted by avg descending.
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
            "{:<14} {:>10} {:>10} {:>8}\n",
            "system", "avg µs", "max µs", "% tick"
        ));
        for row in &self.buckets {
            out.push_str(&format!(
                "{:<14} {:>10.1} {:>10.1} {:>7.1}%\n",
                row.name, row.avg_us, row.max_us, row.pct_of_tick
            ));
        }
        out
    }
}

/// Windowed-only: F3 toggles the live overlay. Headless ignores this.
#[derive(Resource, Default)]
pub struct PerfOverlayEnabled(pub bool);

// ═══════════════════════════════════════════════════════════════════════════
// MEASUREMENT SYSTEMS
// ═══════════════════════════════════════════════════════════════════════════

fn perf_tick_begin(mut tracker: ResMut<PerfTracker>) {
    tracker.mark_tick_begin();
}

fn perf_tick_end(mut tracker: ResMut<PerfTracker>) {
    tracker.mark_tick_end();
}

macro_rules! bucket_markers {
    ($begin:ident, $end:ident, $label:expr) => {
        fn $begin(mut tracker: ResMut<PerfTracker>) {
            tracker.mark_start($label);
        }
        fn $end(mut tracker: ResMut<PerfTracker>) {
            tracker.mark_end($label);
        }
    };
}

bucket_markers!(begin_perception, end_perception, "perception");
bucket_markers!(begin_memory, end_memory, "memory");
bucket_markers!(begin_psyche, end_psyche, "psyche");
bucket_markers!(begin_skills, end_skills, "skills");
bucket_markers!(begin_biology, end_biology, "biology");
bucket_markers!(begin_brain, end_brain, "brain");
bucket_markers!(begin_communication, end_communication, "communication");
bucket_markers!(begin_action, end_action, "action");

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
            .add_systems(FixedFirst, perf_tick_begin)
            .add_systems(FixedLast, perf_tick_end)
            .add_systems(
                FixedUpdate,
                (
                    begin_perception.before(PerfBucket::Perception),
                    end_perception.after(PerfBucket::Perception),
                    begin_memory.before(PerfBucket::Memory),
                    end_memory.after(PerfBucket::Memory),
                    begin_psyche.before(PerfBucket::Psyche),
                    end_psyche.after(PerfBucket::Psyche),
                    begin_skills.before(PerfBucket::Skills),
                    end_skills.after(PerfBucket::Skills),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    begin_biology.before(PerfBucket::Biology),
                    end_biology.after(PerfBucket::Biology),
                    begin_brain.before(PerfBucket::Brain),
                    end_brain.after(PerfBucket::Brain),
                    begin_communication.before(PerfBucket::Communication),
                    end_communication.after(PerfBucket::Communication),
                    begin_action.before(PerfBucket::Action),
                    end_action.after(PerfBucket::Action),
                ),
            );
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
        // Oldest (10) should have been dropped.
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
    fn rolling_window_empty_returns_zero() {
        let w = RollingWindow::new(3);
        assert_eq!(w.avg(), Duration::ZERO);
        assert_eq!(w.max(), Duration::ZERO);
    }

    #[test]
    fn mark_start_end_records_into_window() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_start("perception");
        std::thread::sleep(Duration::from_millis(1));
        tracker.mark_end("perception");
        let data = &tracker.buckets["perception"];
        assert_eq!(data.window.len(), 1);
        assert!(data.window.max() >= Duration::from_millis(1));
    }

    #[test]
    fn mark_end_without_start_is_noop() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_end("memory"); // no prior start
        assert_eq!(tracker.buckets["memory"].window.len(), 0);
    }

    #[test]
    fn unknown_bucket_names_are_ignored() {
        let mut tracker = PerfTracker::new(10);
        tracker.mark_start("does_not_exist");
        tracker.mark_end("does_not_exist");
        // Did not panic; no bucket was added.
        assert_eq!(tracker.buckets.len(), PerfBucket::ALL.len());
    }

    #[test]
    fn snapshot_sorts_by_avg_descending() {
        let mut tracker = PerfTracker::new(10);
        // Seed different durations per bucket.
        let samples = [
            ("perception", 50u64),
            ("memory", 200),
            ("psyche", 30),
            ("skills", 10),
            ("biology", 80),
            ("brain", 500),
            ("communication", 20),
            ("action", 150),
        ];
        for (name, us) in samples {
            let data = tracker.buckets.get_mut(name).unwrap();
            data.window.push(Duration::from_micros(us));
        }
        tracker.total.push(Duration::from_micros(1000));

        let snap = tracker.snapshot();
        assert_eq!(snap.buckets.len(), PerfBucket::ALL.len());
        // First row must be the heaviest (brain at 500µs).
        assert_eq!(snap.buckets[0].name, "brain");
        // Descending ordering across the whole list.
        for pair in snap.buckets.windows(2) {
            assert!(pair[0].avg_us >= pair[1].avg_us);
        }
    }

    #[test]
    fn snapshot_pct_of_tick_computes_against_total_avg() {
        let mut tracker = PerfTracker::new(10);
        tracker.total.push(Duration::from_micros(1000));
        tracker
            .buckets
            .get_mut("brain")
            .unwrap()
            .window
            .push(Duration::from_micros(250));

        let snap = tracker.snapshot();
        let brain = snap.buckets.iter().find(|b| b.name == "brain").unwrap();
        assert!((brain.pct_of_tick - 25.0).abs() < 0.01);
    }

    #[test]
    fn snapshot_pct_of_tick_is_zero_when_total_is_empty() {
        let mut tracker = PerfTracker::new(10);
        tracker
            .buckets
            .get_mut("brain")
            .unwrap()
            .window
            .push(Duration::from_micros(250));
        let snap = tracker.snapshot();
        for row in &snap.buckets {
            assert_eq!(row.pct_of_tick, 0.0);
        }
    }

    #[test]
    fn format_table_includes_header_and_all_buckets() {
        let mut tracker = PerfTracker::new(10);
        tracker.total.push(Duration::from_micros(1000));
        for bucket in PerfBucket::ALL {
            tracker
                .buckets
                .get_mut(bucket.label())
                .unwrap()
                .window
                .push(Duration::from_micros(100));
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
}
