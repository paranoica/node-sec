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

    fn aggregates_30d(count: u64, sum_minor: i64, sum_sq: i64) -> WindowAggregates {
        WindowAggregates {
            windows: vec![WindowStat {
                label: "30d".to_string(),
                count,
                sum_minor,
                sum_sq,
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

    #[test]
    fn derive_does_not_mutate_the_aggregates() {
        // The signature takes &WindowAggregates and returns a value — no store handle, no write.
        let agg = aggregates_30d(5, 500, 50_000);
        let _ = derive(&txn(100), &agg);
        assert_eq!(agg.get("30d").unwrap().count, 5);
    }
}
