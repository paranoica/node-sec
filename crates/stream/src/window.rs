//! Per-entity sliding-window aggregates (D-001 async update path; `term:sliding-window`,
//! `term:aggregate-feature`). Count and summed amount over the standard window set.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

/// The standard sliding windows: label and length in seconds.
pub const WINDOWS: [(&str, i64); 6] = [
    ("1m", 60),
    ("5m", 300),
    ("1h", 3_600),
    ("24h", 86_400),
    ("7d", 604_800),
    ("30d", 2_592_000),
];

/// Count, summed amount, and summed squared amount over one window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStat {
    /// Window label (e.g. `5m`).
    pub label: String,
    /// Number of events in the window.
    pub count: u64,
    /// Summed amount (minor units) in the window.
    pub sum_minor: i64,
    /// Summed squared amount (minor units) — feeds variance / z-score.
    #[serde(default)]
    pub sum_sq: i64,
}

impl WindowStat {
    /// Mean amount in the window (0 if empty).
    #[must_use]
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum_minor as f64 / self.count as f64
        }
    }

    /// Population standard deviation of amounts in the window (0 with fewer than 2 samples).
    #[must_use]
    pub fn std_dev(&self) -> f64 {
        if self.count < 2 {
            return 0.0;
        }
        let n = self.count as f64;
        let mean = self.sum_minor as f64 / n;
        let variance = (self.sum_sq as f64 / n) - mean * mean;
        variance.max(0.0).sqrt()
    }
}

/// Aggregates across every window for one entity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowAggregates {
    /// One [`WindowStat`] per entry in [`WINDOWS`], in the same order.
    pub windows: Vec<WindowStat>,
}

impl WindowAggregates {
    /// The stat for a window label, if present.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&WindowStat> {
        self.windows.iter().find(|w| w.label == label)
    }
}

/// Per-entity event history, retained out to the largest window and queried for all windows.
#[derive(Debug, Default)]
pub struct EntityWindows {
    events: VecDeque<(OffsetDateTime, i64)>,
}

impl EntityWindows {
    /// Record an event `(at, amount)` and evict anything older than the largest window relative to
    /// the most recent event.
    pub fn record(&mut self, at: OffsetDateTime, amount_minor: i64) {
        self.events.push_back((at, amount_minor));
        let latest = self.events.back().map_or(at, |&(t, _)| t);
        let max_secs = WINDOWS[WINDOWS.len() - 1].1;
        let cutoff = latest - Duration::seconds(max_secs);
        while self.events.front().is_some_and(|&(t, _)| t < cutoff) {
            self.events.pop_front();
        }
    }

    /// Compute count + sum for every window as of `now` in a single pass.
    #[must_use]
    pub fn aggregates(&self, now: OffsetDateTime) -> WindowAggregates {
        let mut windows: Vec<WindowStat> = WINDOWS
            .iter()
            .map(|(label, _)| WindowStat {
                label: (*label).to_string(),
                count: 0,
                sum_minor: 0,
                sum_sq: 0,
            })
            .collect();

        for &(t, amount) in &self.events {
            let age = (now - t).whole_seconds();
            if age < 0 {
                continue; // event in the future relative to the query point
            }
            for (i, (_, secs)) in WINDOWS.iter().enumerate() {
                if age <= *secs {
                    windows[i].count += 1;
                    windows[i].sum_minor = windows[i].sum_minor.saturating_add(amount);
                    windows[i].sum_sq = windows[i]
                        .sum_sq
                        .saturating_add(amount.saturating_mul(amount));
                }
            }
        }
        WindowAggregates { windows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use time::Duration as TDuration;

    #[test]
    fn windows_count_only_events_inside_each_window() {
        let now = datetime!(2026-06-17 12:00 UTC);
        let mut w = EntityWindows::default();
        // 3 events within the last 5 minutes.
        w.record(now - TDuration::seconds(10), 100);
        w.record(now - TDuration::seconds(120), 200);
        w.record(now - TDuration::seconds(250), 300);
        // 1 event ~2 hours ago (inside 24h, outside 1h).
        w.record(now - TDuration::seconds(7_200), 1_000);

        let agg = w.aggregates(now);
        assert_eq!(agg.get("1m").unwrap().count, 1); // only the 10s-ago event
        assert_eq!(agg.get("5m").unwrap().count, 3);
        assert_eq!(agg.get("5m").unwrap().sum_minor, 600);
        assert_eq!(agg.get("1h").unwrap().count, 3); // 2h-ago event excluded
        assert_eq!(agg.get("24h").unwrap().count, 4);
        assert_eq!(agg.get("24h").unwrap().sum_minor, 1_600);
    }

    #[test]
    fn old_events_are_evicted_beyond_the_largest_window() {
        let now = datetime!(2026-06-17 12:00 UTC);
        let mut w = EntityWindows::default();
        w.record(now - TDuration::days(40), 100); // older than 30d
        w.record(now, 200);
        // The 40-day-old event is evicted on the second record (relative to the latest).
        let agg = w.aggregates(now);
        assert_eq!(agg.get("30d").unwrap().count, 1);
        assert_eq!(agg.get("30d").unwrap().sum_minor, 200);
    }
}
