//! In-process velocity tracking for the S1 card slice (card-testing, BIN-attack, decline-retry).
//!
//! State is in-memory and **per-entity unbounded** — a previously unseen device/BIN/card adds a map
//! entry that is never removed (each entry's time window self-evicts, but the key set only grows).
//! This is the S1 stand-in; S2 (T020–T022) replaces it with the streamed online feature store, at
//! which point the rules read precomputed features instead of tracking state here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use domain::{ReasonCode, Transaction};
use time::{Duration, OffsetDateTime};

use crate::config::VelocityConfig;
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
    pub fn observe(&self, txn: &Transaction, cfg: &VelocityConfig) -> Vec<RuleHit> {
        let now = txn.occurred_at;
        let mut state = self.state.lock().expect("velocity state poisoned");
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

        hits
    }

    /// Record a decline for a card (feedback after a decision), feeding the decline-retry window.
    pub fn record_decline(&self, txn: &Transaction, cfg: &VelocityConfig) {
        let Some(pan) = &txn.pan else { return };
        let now = txn.occurred_at;
        let mut state = self.state.lock().expect("velocity state poisoned");
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Channel, Currency, DeviceId, Money, Pan, TransactionId, Vertical};
    use time::macros::datetime;
    use time::Duration as TDuration;

    fn low_value_card_txn(pan: &str, device: &str, at: OffsetDateTime) -> Transaction {
        Transaction::new(
            TransactionId::new("t"),
            Money::from_minor_units(100, Currency::Usd),
            at,
            Vertical::Card,
            Channel::CardNotPresent,
        )
        .with_pan(Pan::new(pan))
        .with_device(DeviceId::new(device))
    }

    #[test]
    fn card_testing_fires_after_the_burst_threshold() {
        let cfg = VelocityConfig::default(); // max_low_value_auths = 5
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        let mut fired = false;
        for i in 0..7 {
            let txn =
                low_value_card_txn("4111110000001234", "dev-1", start + TDuration::seconds(i));
            if !tracker.observe(&txn, &cfg).is_empty() {
                fired = true;
            }
        }
        assert!(
            fired,
            "card-testing must fire once the device exceeds the low-value burst threshold"
        );
    }

    #[test]
    fn bin_attack_fires_on_many_distinct_pans_one_bin() {
        let cfg = VelocityConfig::default(); // max_distinct_pans = 10
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        let mut fired = false;
        for i in 0..12u64 {
            // Same BIN 411111, distinct tails → distinct card tokens.
            let pan = format!("411111{i:010}");
            let txn = low_value_card_txn(&pan, "dev-x", start + TDuration::seconds(i as i64));
            if tracker
                .observe(&txn, &cfg)
                .iter()
                .any(|h| h.reason_code.as_str() == "VELOCITY_BIN_ATTACK")
            {
                fired = true;
            }
        }
        assert!(
            fired,
            "BIN-attack must fire once distinct PANs per BIN exceed the threshold"
        );
    }

    #[test]
    fn decline_retry_fires_only_after_recorded_declines() {
        let cfg = VelocityConfig::default(); // max_declines = 5
        let tracker = VelocityTracker::new();
        let at = datetime!(2026-06-17 00:00 UTC);
        let txn = low_value_card_txn("4111110000009999", "dev-2", at);

        // No declines recorded yet → a fresh observe sees no storm.
        assert!(!tracker
            .observe(&txn, &cfg)
            .iter()
            .any(|h| h.reason_code.as_str() == "VELOCITY_DECLINE_RETRY"));

        // Feed 6 declines, then the next observe sees the storm.
        for _ in 0..6 {
            tracker.record_decline(&txn, &cfg);
        }
        assert!(tracker
            .observe(&txn, &cfg)
            .iter()
            .any(|h| h.reason_code.as_str() == "VELOCITY_DECLINE_RETRY"));
    }

    #[test]
    fn window_expiry_drops_stale_events() {
        let cfg = VelocityConfig::default(); // card_testing window = 300s
        let tracker = VelocityTracker::new();
        let start = datetime!(2026-06-17 00:00 UTC);
        // 5 low-value auths, then a 6th far outside the window → never exceeds 5-in-window.
        for i in 0..5 {
            let txn =
                low_value_card_txn("4111110000001234", "dev-3", start + TDuration::seconds(i));
            assert!(tracker.observe(&txn, &cfg).is_empty());
        }
        let late = low_value_card_txn(
            "4111110000001234",
            "dev-3",
            start + TDuration::seconds(10_000),
        );
        assert!(
            tracker.observe(&late, &cfg).is_empty(),
            "events outside the window must not accumulate into a burst"
        );
    }
}
