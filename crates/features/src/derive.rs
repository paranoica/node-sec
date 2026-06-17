//! Request-time deviation features (T022; `term:deviation-feature`).
//!
//! Pure derivation from the current transaction plus the looked-up online aggregates — it takes the
//! aggregates by reference and returns a value, so by construction it performs **no store write** on
//! the hot path.

use domain::Transaction;
use stream::WindowAggregates;

/// The window used as the deviation baseline.
const REFERENCE_WINDOW: &str = "30d";

/// Features derived at decision time from the transaction and the entity's looked-up aggregates.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestFeatures {
    /// This amount ÷ the entity's rolling mean over the reference window (1.0 with no history).
    pub amount_to_mean_ratio: f64,
    /// Z-score of this amount vs the entity's window distribution (0.0 with < 2 samples / no spread).
    pub amount_z_score: f64,
    /// Transaction count in the reference window (a velocity feature surfaced to the model).
    pub velocity_count: u64,
}

/// Number of model input features — must match `ml` `FEATURE_NAMES` and the ONNX graph's input.
pub const MODEL_FEATURE_LEN: usize = 8;

/// MCCs treated as high-risk for the model's `high_risk_mcc` feature: gambling (7995), wire/money
/// transfer (4829), and quasi-cash / crypto (6051). Mirrors the rules' MCC-risk intent.
const HIGH_RISK_MCC: [u32; 3] = [7995, 4829, 6051];

/// Assemble the model input vector in the canonical `FEATURE_NAMES` order from the transaction and
/// the entity's online aggregates:
/// `[velocity_5m, velocity_1h, velocity_24h, amount_to_mean_ratio, amount_z_score,
///   distinct_devices_24h, decline_rate_1h, high_risk_mcc]`.
///
/// All eight features are now derived: velocity and amount-deviation from the windowed aggregates,
/// `distinct_devices_24h` and `decline_rate_1h` from the device/decline aggregates, and
/// `high_risk_mcc` from the (enriched) transaction MCC. A cold entity (empty aggregates) scores the
/// neutral defaults rather than a stub.
#[must_use]
pub fn model_vector(txn: &Transaction, aggregates: &WindowAggregates) -> Vec<f32> {
    let dev = derive(txn, aggregates);
    let velocity = |label: &str| aggregates.get(label).map_or(0, |s| s.count) as f32;
    let distinct_devices_24h = aggregates.get("24h").map_or(0, |s| s.distinct_devices) as f32;
    let decline_rate_1h = aggregates.get("1h").map_or(0.0, |s| {
        if s.count > 0 {
            s.decline_count as f32 / s.count as f32
        } else {
            0.0
        }
    });
    let high_risk_mcc = txn.mcc.is_some_and(|mcc| HIGH_RISK_MCC.contains(&mcc));
    vec![
        velocity("5m"),
        velocity("1h"),
        velocity("24h"),
        dev.amount_to_mean_ratio as f32,
        dev.amount_z_score as f32,
        distinct_devices_24h,
        decline_rate_1h,
        f32::from(u8::from(high_risk_mcc)),
    ]
}

/// Derive request-time deviation features. Reads only the supplied aggregates; never writes.
#[must_use]
pub fn derive(txn: &Transaction, aggregates: &WindowAggregates) -> RequestFeatures {
    let amount = txn.amount.minor_units() as f64;

    let (mean, std_dev, count) = aggregates
        .get(REFERENCE_WINDOW)
        .map_or((0.0, 0.0, 0), |s| (s.mean(), s.std_dev(), s.count));

    let amount_to_mean_ratio = if mean > 0.0 { amount / mean } else { 1.0 };
    let amount_z_score = if std_dev > 0.0 {
        (amount - mean) / std_dev
    } else {
        0.0
    };

    RequestFeatures {
        amount_to_mean_ratio,
        amount_z_score,
        velocity_count: count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Channel, Currency, Money, TransactionId, Vertical};
    use stream::WindowStat;
    use time::macros::datetime;

    fn txn(amount: i64) -> Transaction {
        Transaction::new(
            TransactionId::new("t"),
            Money::from_minor_units(amount, Currency::Usd),
            datetime!(2026-06-17 12:00 UTC),
            Vertical::Card,
            Channel::CardNotPresent,
        )
    }

    fn aggregates_30d(count: u64, sum_minor: i64, sum_sq: i128) -> WindowAggregates {
        WindowAggregates {
            windows: vec![WindowStat {
                label: "30d".to_string(),
                count,
                sum_minor,
                sum_sq,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn derive_amount_to_mean_ratio() {
        // 10 events summing 1000 → mean 100; amount 1000 → ratio 10.
        let f = derive(&txn(1_000), &aggregates_30d(10, 1_000, 100_000));
        assert!((f.amount_to_mean_ratio - 10.0).abs() < 1e-9);
        assert_eq!(f.velocity_count, 10);
    }

    #[test]
    fn derive_z_score_from_window_distribution() {
        // 2 events: 100 and 200 → mean 150, sum_sq 50000, std 50; amount 250 → z = (250-150)/50 = 2.
        let f = derive(&txn(250), &aggregates_30d(2, 300, 50_000));
        assert!((f.amount_z_score - 2.0).abs() < 1e-9);
    }

    #[test]
    fn derive_with_no_history_is_neutral() {
        let f = derive(&txn(1_000), &WindowAggregates::default());
        assert!((f.amount_to_mean_ratio - 1.0).abs() < 1e-9);
        assert!(f.amount_z_score.abs() < 1e-9);
        assert_eq!(f.velocity_count, 0);
    }

    fn aggregates_velocity(v5m: u64, v1h: u64, v24h: u64) -> WindowAggregates {
        WindowAggregates {
            windows: vec![
                WindowStat {
                    label: "5m".to_string(),
                    count: v5m,
                    sum_minor: 0,
                    sum_sq: 0,
                    ..Default::default()
                },
                WindowStat {
                    label: "1h".to_string(),
                    count: v1h,
                    sum_minor: 0,
                    sum_sq: 0,
                    ..Default::default()
                },
                WindowStat {
                    label: "24h".to_string(),
                    count: v24h,
                    sum_minor: 0,
                    sum_sq: 0,
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn model_vector_maps_windows_to_canonical_order() {
        let v = model_vector(&txn(1_000), &aggregates_velocity(2, 5, 13));
        assert_eq!(v.len(), MODEL_FEATURE_LEN);
        assert_eq!(v[0], 2.0); // velocity_5m
        assert_eq!(v[1], 5.0); // velocity_1h
        assert_eq!(v[2], 13.0); // velocity_24h
        assert_eq!(v[3], 1.0); // amount_to_mean_ratio (neutral, no 30d history)
        assert_eq!(v[4], 0.0); // amount_z_score (neutral)
                               // No device/decline data and no MCC in this fixture → the last three are 0.
        assert_eq!(&v[5..8], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn model_vector_derives_device_spread_and_decline_rate() {
        // 24h window has 2 distinct devices; 1h window has 2 of 4 declined → decline_rate 0.5.
        let agg = WindowAggregates {
            windows: vec![
                WindowStat {
                    label: "1h".to_string(),
                    count: 4,
                    decline_count: 2,
                    ..Default::default()
                },
                WindowStat {
                    label: "24h".to_string(),
                    count: 4,
                    distinct_devices: 2,
                    ..Default::default()
                },
            ],
        };
        let v = model_vector(&txn(1_000), &agg);
        assert_eq!(v[5], 2.0); // distinct_devices_24h
        assert!((v[6] - 0.5).abs() < 1e-6); // decline_rate_1h
    }

    #[test]
    fn model_vector_sets_high_risk_mcc_from_transaction() {
        // A gambling MCC lights the high_risk_mcc feature; a grocery MCC does not.
        let gambling = model_vector(&txn(1_000).with_mcc(7995), &WindowAggregates::default());
        assert_eq!(gambling[7], 1.0);
        let grocery = model_vector(&txn(1_000).with_mcc(5411), &WindowAggregates::default());
        assert_eq!(grocery[7], 0.0);
    }

    #[test]
    fn derive_does_not_mutate_the_aggregates() {
        // The signature takes &WindowAggregates and returns a value — no store handle, no write.
        let agg = aggregates_30d(5, 500, 50_000);
        let _ = derive(&txn(100), &agg);
        assert_eq!(agg.get("30d").unwrap().count, 5);
    }
}
