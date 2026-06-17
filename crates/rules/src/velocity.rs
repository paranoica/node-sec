//! In-process velocity tracking for the S1 card slice (card-testing, BIN-attack, decline-retry).
//!
//! State is in-memory and **per-entity unbounded** — a previously unseen device/BIN/card adds a map
//! entry that is never removed (each entry's time window self-evicts, but the key set only grows).
//! This is the S1 stand-in; S2 (T020–T022) replaces it with the streamed online feature store, at
//! which point the rules read precomputed features instead of tracking state here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use domain::{Geo, ReasonCode, Transaction};
use time::{Duration, OffsetDateTime};

use crate::config::{AmountAnomaly, ImpossibleTravel, VelocityConfig};
use crate::engine::{Disposition, RuleHit};

/// Per-entity sliding-window counters feeding the velocity rules.
#[derive(Debug, Default)]
pub struct VelocityTracker {
    state: Mutex<Windows>,
}

#[derive(Debug, Default)]
struct Windows {
    /// device → timestamps of low-value auths (card testing).
    device_low_value: HashMap<String, VecDeque<OffsetDateTime>>,
    /// BIN → (timestamp, card token) pairs (BIN enumeration).
    bin_pans: HashMap<String, VecDeque<(OffsetDateTime, String)>>,
    /// card token → timestamps of declines (retry storm).
    card_declines: HashMap<String, VecDeque<OffsetDateTime>>,
    /// card token → last seen (geo, time) for impossible travel.
    card_last_location: HashMap<String, (Geo, OffsetDateTime)>,
    /// card token → (sample count, running mean minor-unit amount) for amount anomaly.
    card_amount_stats: HashMap<String, (u64, f64)>,
}

fn evict_ts(dq: &mut VecDeque<OffsetDateTime>, now: OffsetDateTime, window_secs: i64) {
    let cutoff = now - Duration::seconds(window_secs);
    while dq.front().is_some_and(|&t| t < cutoff) {
        dq.pop_front();
    }
}

fn evict_pairs(dq: &mut VecDeque<(OffsetDateTime, String)>, now: OffsetDateTime, window_secs: i64) {
    let cutoff = now - Duration::seconds(window_secs);
    while dq.front().is_some_and(|(t, _)| *t < cutoff) {
        dq.pop_front();
    }
}

impl VelocityTracker {
    /// A fresh tracker with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record this attempt and return any velocity hits it triggers: card-testing (low-value auth
    /// burst per device), BIN-attack (distinct PANs per BIN), and decline-retry (the card is already
    /// in a decline storm from prior [`VelocityTracker::record_decline`] feedback).
    pub fn observe(
        &self,
        txn: &Transaction,
        cfg: &VelocityConfig,
        travel: &ImpossibleTravel,
        amount: &AmountAnomaly,
    ) -> Vec<RuleHit> {
        let now = txn.occurred_at;
        // Recover a poisoned lock (data is intact; poisoning only flags a prior panic) so one
        // panicked request can never wedge the velocity stage for every subsequent decision.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut hits = Vec::new();

        if let Some(device) = &txn.device {
            if txn.amount.minor_units() <= cfg.card_testing.low_value_threshold_minor {
                let dq = state
                    .device_low_value
                    .entry(device.as_str().to_string())
                    .or_default();
                dq.push_back(now);
                evict_ts(dq, now, cfg.card_testing.window_secs);
                if dq.len() as u64 > cfg.card_testing.max_low_value_auths {
                    hits.push(hit(
                        "velocity.card_testing",
                        "VELOCITY_CARD_TESTING",
                        "card-testing",
                    ));
                }
            }
        }

        if let Some(pan) = &txn.pan {
            if let Some(bin) = pan.bin() {
                let dq = state.bin_pans.entry(bin.as_str().to_string()).or_default();
                dq.push_back((now, pan.redacted()));
                evict_pairs(dq, now, cfg.bin_attack.window_secs);
                let distinct = dq
                    .iter()
                    .map(|(_, p)| p.as_str())
                    .collect::<HashSet<_>>()
                    .len();
                if distinct as u64 > cfg.bin_attack.max_distinct_pans {
                    hits.push(hit(
                        "velocity.bin_attack",
                        "VELOCITY_BIN_ATTACK",
                        "bin-attack",
                    ));
                }
            }
        }

        if let Some(pan) = &txn.pan {
            if let Some(dq) = state.card_declines.get_mut(&pan.redacted()) {
                evict_ts(dq, now, cfg.decline_retry.window_secs);
                if dq.len() as u64 > cfg.decline_retry.max_declines {
                    hits.push(hit(
                        "velocity.decline_retry",
                        "VELOCITY_DECLINE_RETRY",
                        "card-testing",
                    ));
                }
            }
        }

        // Impossible travel: implied speed since the card's last known location.
        if let (Some(pan), Some(geo)) = (&txn.pan, &txn.geo) {
            let token = pan.redacted();
            if let Some((last_geo, last_time)) = state.card_last_location.get(&token) {
                let hours = (now - *last_time).as_seconds_f64() / 3600.0;
                if hours > 0.0 && last_geo.distance_km(geo) / hours > travel.max_speed_kmh {
                    hits.push(hit(
                        "rule.impossible_travel",
                        "IMPOSSIBLE_TRAVEL",
                        "impossible-travel",
                    ));
                }
            }
            state.card_last_location.insert(token, (geo.clone(), now));
        }

        // Amount anomaly: amount far above the card's running mean ticket size.
        if let Some(pan) = &txn.pan {
            let token = pan.redacted();
            let value = txn.amount.minor_units() as f64;
            let entry = state.card_amount_stats.entry(token).or_insert((0, 0.0));
            let (count, mean) = *entry;
            if count >= amount.min_samples && value > mean * amount.factor {
                hits.push(soft(
                    "rule.amount_anomaly",
                    "AMOUNT_ANOMALY",
                    "amount-anomaly",
                ));
            }
            let new_count = count + 1;
            *entry = (new_count, mean + (value - mean) / new_count as f64);
        }

        hits
    }

    /// Record a decline for a card (feedback after a decision), feeding the decline-retry window.
    pub fn record_decline(&self, txn: &Transaction, cfg: &VelocityConfig) {
        let Some(pan) = &txn.pan else { return };
        let now = txn.occurred_at;
        // Recover a poisoned lock (data is intact; poisoning only flags a prior panic) so one
        // panicked request can never wedge the velocity stage for every subsequent decision.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let dq = state.card_declines.entry(pan.redacted()).or_default();
        dq.push_back(now);
        evict_ts(dq, now, cfg.decline_retry.window_secs);
    }
}

fn hit(rule_id: &str, reason_code: &str, typology: &str) -> RuleHit {
    RuleHit {
        rule_id: rule_id.to_string(),
        reason_code: ReasonCode::new(reason_code),
        typology: typology.to_string(),
        disposition: Disposition::HardDecline,
    }
}

fn soft(rule_id: &str, reason_code: &str, typology: &str) -> RuleHit {
    RuleHit {
        rule_id: rule_id.to_string(),
        reason_code: ReasonCode::new(reason_code),
        typology: typology.to_string(),
        disposition: Disposition::Soft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Channel, Currency, DeviceId, Geo, Money, Pan, TransactionId, Vertical};
    use time::macros::datetime;
    use time::Duration as TDuration;

    fn card_txn(pan: &str, device: &str, at: OffsetDateTime, amount_minor: i64) -> Transaction {
        Transaction::new(
            TransactionId::new("t"),
            Money::from_minor_units(amount_minor, Currency::Usd),
            at,
            Vertical::Card,
            Channel::CardNotPresent,
        )
        .with_pan(Pan::new(pan))
        .with_device(DeviceId::new(device))
    }

    fn obs(tracker: &VelocityTracker, txn: &Transaction) -> Vec<RuleHit> {
        tracker.observe(
            txn,
            &VelocityConfig::default(),
            &ImpossibleTravel::default(),
            &AmountAnomaly::default(),
        )
    }

    fn fired(hits: &[RuleHit], reason: &str) -> bool {
        hits.iter().any(|h| h.reason_code.as_str() == reason)
    }

    #[test]
    fn card_testing_fires_after_the_burst_threshold() {
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        let mut saw = false;
        for i in 0..7 {
            let txn = card_txn(
                "4111110000001234",
                "dev-1",
                start + TDuration::seconds(i),
                100,
            );
            if fired(&obs(&tracker, &txn), "VELOCITY_CARD_TESTING") {
                saw = true;
            }
        }
        assert!(
            saw,
            "card-testing must fire once the device exceeds the low-value burst threshold"
        );
    }

    #[test]
    fn bin_attack_fires_on_many_distinct_pans_one_bin() {
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        let mut saw = false;
        for i in 0..12u64 {
            // Same BIN 411111, distinct tails → distinct card tokens.
            let pan = format!("411111{i:010}");
            let txn = card_txn(&pan, "dev-x", start + TDuration::seconds(i as i64), 100);
            if fired(&obs(&tracker, &txn), "VELOCITY_BIN_ATTACK") {
                saw = true;
            }
        }
        assert!(
            saw,
            "BIN-attack must fire once distinct PANs per BIN exceed the threshold"
        );
    }

    #[test]
    fn decline_retry_fires_only_after_recorded_declines() {
        let cfg = VelocityConfig::default();
        let tracker = VelocityTracker::new();
        let at = datetime!(2026-06-17 00:00 UTC);
        let txn = card_txn("4111110000009999", "dev-2", at, 100);

        assert!(!fired(&obs(&tracker, &txn), "VELOCITY_DECLINE_RETRY"));
        for _ in 0..6 {
            tracker.record_decline(&txn, &cfg);
        }
        assert!(fired(&obs(&tracker, &txn), "VELOCITY_DECLINE_RETRY"));
    }

    #[test]
    fn window_expiry_drops_stale_events() {
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        for i in 0..5 {
            let txn = card_txn(
                "4111110000001234",
                "dev-3",
                start + TDuration::seconds(i),
                100,
            );
            assert!(!fired(&obs(&tracker, &txn), "VELOCITY_CARD_TESTING"));
        }
        let late = card_txn(
            "4111110000001234",
            "dev-3",
            start + TDuration::seconds(10_000),
            100,
        );
        assert!(
            !fired(&obs(&tracker, &late), "VELOCITY_CARD_TESTING"),
            "events outside the window must not accumulate into a burst"
        );
    }

    #[test]
    fn impossible_travel_fires_on_implausible_speed() {
        let tracker = VelocityTracker::new();
        let t0 = datetime!(2026-06-17 00:00 UTC);
        let ny =
            card_txn("4111110000005555", "dev-a", t0, 5_000).with_geo(Geo::new("US", 40.71, -74.0));
        let tokyo = card_txn(
            "4111110000005555",
            "dev-b",
            t0 + TDuration::seconds(60),
            5_000,
        )
        .with_geo(Geo::new("JP", 35.68, 139.69));
        assert!(
            !fired(&obs(&tracker, &ny), "IMPOSSIBLE_TRAVEL"),
            "first sighting sets the baseline"
        );
        assert!(fired(&obs(&tracker, &tokyo), "IMPOSSIBLE_TRAVEL"));
    }

    #[test]
    fn amount_anomaly_fires_above_running_mean() {
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        // Baseline of $1 tickets (min_samples = 5).
        for i in 0..6 {
            let txn = card_txn(
                "4111110000007777",
                "dev-c",
                start + TDuration::seconds(i),
                100,
            );
            assert!(!fired(&obs(&tracker, &txn), "AMOUNT_ANOMALY"));
        }
        // A $1000 ticket is >10x the ~$1 mean → fires.
        let big = card_txn(
            "4111110000007777",
            "dev-c",
            start + TDuration::seconds(7),
            100_000,
        );
        assert!(fired(&obs(&tracker, &big), "AMOUNT_ANOMALY"));
    }
}
