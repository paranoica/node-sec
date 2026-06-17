//! Offline feature materialisation for training (D-006; point-in-time correct, `arch:feature-parity`).
//!
//! Uses the **same** windowing definition as the online path (`stream::EntityWindows`), so features
//! materialised offline for training equal the ones served online — parity by construction. The
//! cross-language parity check is `ml/tests/test_parity.py`, run against the same golden fixture this
//! module's test loads.

use stream::{EntityWindows, WindowAggregates};
use time::OffsetDateTime;

/// Materialise an entity's window aggregates as of `now` from its event history. Point-in-time
/// correct: events after `now` are excluded, so a training label never sees future data.
#[must_use]
pub fn materialize(events: &[(OffsetDateTime, i64)], now: OffsetDateTime) -> WindowAggregates {
    let mut windows = EntityWindows::default();
    for &(ts, amount) in events {
        if ts <= now {
            windows.record(ts, amount);
        }
    }
    windows.aggregates(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The same golden fixture the Python offline test uses — proves online == offline.
    const FIXTURE: &str = include_str!("../../../ml/tests/fixtures/parity_case.json");

    fn ts(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(secs).unwrap()
    }

    #[test]
    fn online_definition_matches_the_parity_golden() {
        let case: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let now = ts(case["now_unix"].as_i64().unwrap());
        let events: Vec<(OffsetDateTime, i64)> = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    ts(e["ts_unix"].as_i64().unwrap()),
                    e["amount_minor"].as_i64().unwrap(),
                )
            })
            .collect();

        let agg = materialize(&events, now);
        let expected = &case["expected"];
        for (label, _) in stream::WINDOWS {
            let stat = agg.get(label).unwrap();
            let exp = &expected[label];
            assert_eq!(stat.count, exp["count"].as_u64().unwrap(), "count {label}");
            assert_eq!(
                stat.sum_minor,
                exp["sum_minor"].as_i64().unwrap(),
                "sum {label}"
            );
            assert_eq!(
                stat.sum_sq,
                exp["sum_sq"].as_i64().unwrap(),
                "sum_sq {label}"
            );
        }
    }

    #[test]
    fn point_in_time_excludes_future_events() {
        let now = ts(1_000_000);
        let events = vec![(ts(1_000_000 - 10), 100), (ts(1_000_000 + 100), 999)]; // second is future
        let agg = materialize(&events, now);
        assert_eq!(agg.get("1m").unwrap().count, 1);
        assert_eq!(agg.get("1m").unwrap().sum_minor, 100);
    }
}
