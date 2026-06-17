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
///
/// All fields are required on deserialize (no `serde(default)`): a stored value missing a field is a
/// **schema mismatch**, and reading it must fail (→ fail-safe degrade, then recompute on the
/// entity's next event) rather than silently default the field to 0, which would feed the rules and
/// the model a wrong variance / device-spread / decline-rate with no signal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStat {
    /// Window label (e.g. `5m`).
    pub label: String,
    /// Number of events in the window.
    pub count: u64,
    /// Summed amount (minor units) in the window.
    pub sum_minor: i64,
    /// Summed squared amount (minor units) — feeds variance / z-score. `i128` because a squared
    /// i64 amount overflows i64, and the running sum over a window would silently saturate and
    /// corrupt every variance/z-score that feeds the rules and the model.
    pub sum_sq: i128,
    /// Distinct devices seen on this entity in the window (feeds `distinct_devices_24h`).
    pub distinct_devices: u64,
    /// Declined events in the window (feeds `decline_rate_1h` together with `count`).
    pub decline_count: u64,
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

/// One recorded event: time, amount, and the device + decline outcome that feed the cardinality and
/// decline-rate features.
#[derive(Debug, Clone)]
struct Event {
    at: OffsetDateTime,
    amount: i64,
    device: Option<String>,
    declined: bool,
}

/// Per-entity event history, retained out to the largest window and queried for all windows.
#[derive(Debug, Default)]
pub struct EntityWindows {
    events: VecDeque<Event>,
}

impl EntityWindows {
    /// Record an amount-only event (no device / decline outcome).
    pub fn record(&mut self, at: OffsetDateTime, amount_minor: i64) {
        self.record_full(at, amount_minor, None, false);
    }

    /// Record a full event — amount plus the device fingerprint and whether the decision declined —
    /// and evict anything older than the largest window relative to the most recent event.
    pub fn record_full(
        &mut self,
        at: OffsetDateTime,
        amount_minor: i64,
        device: Option<String>,
        declined: bool,
    ) {
        self.events.push_back(Event {
            at,
            amount: amount_minor,
            device,
            declined,
        });
        let latest = self.events.back().map_or(at, |e| e.at);
        let max_secs = WINDOWS[WINDOWS.len() - 1].1;
        let cutoff = latest - Duration::seconds(max_secs);
        while self.events.front().is_some_and(|e| e.at < cutoff) {
            self.events.pop_front();
        }
    }

    /// Compute count + sum + distinct-devices + declines for every window as of `now` in one pass.
    #[must_use]
    pub fn aggregates(&self, now: OffsetDateTime) -> WindowAggregates {
        let mut windows: Vec<WindowStat> = WINDOWS
            .iter()
            .map(|(label, _)| WindowStat {
                label: (*label).to_string(),
                count: 0,
                sum_minor: 0,
                sum_sq: 0,
                distinct_devices: 0,
                decline_count: 0,
            })
            .collect();
        // Distinct device sets per window (index-aligned with `windows`).
        let mut devices: Vec<std::collections::HashSet<&str>> = WINDOWS
            .iter()
            .map(|_| std::collections::HashSet::new())
            .collect();

        for event in &self.events {
            let age = (now - event.at).whole_seconds();
            if age < 0 {
                continue; // event in the future relative to the query point
            }
            for (i, (_, secs)) in WINDOWS.iter().enumerate() {
                if age <= *secs {
                    windows[i].count += 1;
                    windows[i].sum_minor = windows[i].sum_minor.saturating_add(event.amount);
                    // i128: a squared i64 amount cannot overflow i128, so the variance input stays
                    // exact for any realistic volume instead of saturating to garbage.
                    let sq = i128::from(event.amount) * i128::from(event.amount);
                    windows[i].sum_sq = windows[i].sum_sq.saturating_add(sq);
                    if event.declined {
                        windows[i].decline_count += 1;
                    }
                    if let Some(device) = &event.device {
                        devices[i].insert(device.as_str());
                    }
                }
            }
        }
        for (i, set) in devices.iter().enumerate() {
            windows[i].distinct_devices = set.len() as u64;
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
    fn sum_sq_does_not_overflow_on_large_amounts() {
        // $50M in cents: amount^2 = 2.5e19 overflows i64 (max ~9.2e18). With i128 the squared sum
        // stays exact instead of saturating to garbage (which would corrupt the variance/z-score).
        let now = datetime!(2026-06-17 12:00 UTC);
        let amount: i64 = 5_000_000_000;
        let mut w = EntityWindows::default();
        w.record(now - TDuration::seconds(10), amount);
        w.record(now - TDuration::seconds(20), amount);
        let agg = w.aggregates(now);
        let expected = 2 * i128::from(amount) * i128::from(amount);
        assert_eq!(agg.get("5m").unwrap().sum_sq, expected);
        assert!(
            expected > i128::from(i64::MAX),
            "test must exceed i64 range"
        );
    }

    #[test]
    fn deserialising_a_schema_mismatch_fails_rather_than_defaulting() {
        // An old-schema value missing `sum_sq` must error on read (→ fail-safe degrade + recompute),
        // not silently deserialize with sum_sq = 0 and corrupt the variance.
        let old = r#"{"windows":[{"label":"5m","count":3,"sum_minor":600}]}"#;
        assert!(serde_json::from_str::<WindowAggregates>(old).is_err());
        // A complete current value still round-trips.
        let current = WindowAggregates {
            windows: vec![WindowStat {
                label: "5m".to_string(),
                count: 3,
                sum_minor: 600,
                sum_sq: 120_000,
                distinct_devices: 1,
                decline_count: 0,
            }],
        };
        let json = serde_json::to_string(&current).unwrap();
        assert_eq!(
            serde_json::from_str::<WindowAggregates>(&json).unwrap(),
            current
        );
    }

    #[test]
    fn aggregates_count_distinct_devices_and_declines() {
        let now = datetime!(2026-06-17 12:00 UTC);
        let mut w = EntityWindows::default();
        w.record_full(
            now - TDuration::seconds(10),
            100,
            Some("d1".to_string()),
            false,
        );
        w.record_full(
            now - TDuration::seconds(20),
            100,
            Some("d2".to_string()),
            true,
        );
        w.record_full(
            now - TDuration::seconds(30),
            100,
            Some("d1".to_string()),
            true,
        ); // repeat
        let s = w.aggregates(now);
        let stat = s.get("5m").unwrap();
        assert_eq!(stat.count, 3);
        assert_eq!(stat.distinct_devices, 2); // d1, d2 — the repeat isn't double-counted
        assert_eq!(stat.decline_count, 2);
    }

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
