//! AML transaction monitoring (T051; D-010, `term:aml-monitoring`).
//!
//! Typology-tagged rules over a windowed slice of an account's transactions. T051 covers the
//! headline typologies — **structuring** (deposits clustered just below the CTR threshold),
//! **funnel** (credits from many geographies, debits concentrated to few destinations), and
//! **round-tripping** (an outbound matched by a near-equal inbound) — each alert carrying its
//! hypothesised typology. Thresholds are data (`config/aml/`), not code (D-014).

use std::collections::HashSet;

use serde::Deserialize;
use time::OffsetDateTime;

/// Direction of a monitored transaction relative to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Credit into the account.
    In,
    /// Debit out of the account.
    Out,
}

/// One transaction in an account's monitored window.
#[derive(Debug, Clone)]
pub struct AmlTransaction {
    /// Amount (minor units).
    pub amount_minor: i64,
    /// Credit or debit.
    pub direction: Direction,
    /// The counterparty.
    pub counterparty: String,
    /// The transaction's geography (country).
    pub geography: String,
    /// Event time.
    pub at: OffsetDateTime,
}

/// An AML alert tagged with its hypothesised typology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmlAlert {
    /// The typology (e.g. `structuring`).
    pub typology: String,
    /// Human-readable detail.
    pub detail: String,
}

/// AML rule thresholds (data, not code).
#[derive(Debug, Clone, Deserialize)]
pub struct AmlConfig {
    /// CTR reporting threshold (minor units).
    pub ctr_threshold_minor: i64,
    /// How far below the CTR threshold counts as "just below".
    pub structuring_band_minor: i64,
    /// Minimum sub-threshold deposits to flag structuring.
    pub structuring_min_count: u64,
    /// Minimum distinct credit geographies for a funnel.
    pub funnel_min_geographies: u64,
    /// Maximum distinct debit destinations for a funnel.
    pub funnel_max_destinations: u64,
    /// Fractional tolerance for round-trip amount matching.
    pub round_trip_tolerance: f64,
    /// Round-trip matching window (seconds).
    pub round_trip_window_secs: i64,
}

impl Default for AmlConfig {
    fn default() -> Self {
        Self {
            ctr_threshold_minor: 1_000_000,  // $10,000
            structuring_band_minor: 100_000, // within $1,000 below
            structuring_min_count: 3,
            funnel_min_geographies: 3,
            funnel_max_destinations: 2,
            round_trip_tolerance: 0.05,
            round_trip_window_secs: 15_552_000, // 180 days
        }
    }
}

/// Evaluate the typology rules over a window of an account's transactions.
#[must_use]
pub fn evaluate(txns: &[AmlTransaction], config: &AmlConfig) -> Vec<AmlAlert> {
    let mut alerts = Vec::new();
    let credits = || txns.iter().filter(|t| t.direction == Direction::In);
    let debits = || txns.iter().filter(|t| t.direction == Direction::Out);

    // Structuring: deposits clustered just below the CTR threshold.
    let floor = config.ctr_threshold_minor - config.structuring_band_minor;
    let just_below = credits()
        .filter(|t| t.amount_minor >= floor && t.amount_minor < config.ctr_threshold_minor)
        .count() as u64;
    if just_below >= config.structuring_min_count {
        alerts.push(AmlAlert {
            typology: "structuring".to_string(),
            detail: format!("{just_below} deposits just below the CTR threshold"),
        });
    }

    // Funnel: credits from many geographies, debits concentrated to few destinations.
    let geos: HashSet<&str> = credits().map(|t| t.geography.as_str()).collect();
    let destinations: HashSet<&str> = debits().map(|t| t.counterparty.as_str()).collect();
    if geos.len() as u64 >= config.funnel_min_geographies
        && !destinations.is_empty()
        && destinations.len() as u64 <= config.funnel_max_destinations
    {
        alerts.push(AmlAlert {
            typology: "funnel".to_string(),
            detail: format!(
                "credits from {} geographies, debits to {} destinations",
                geos.len(),
                destinations.len()
            ),
        });
    }

    // Round-tripping: an outbound matched by a near-equal inbound within the window.
    for out in debits() {
        let matched = credits().any(|inb| {
            let dt = (inb.at - out.at).whole_seconds().abs();
            let diff = (inb.amount_minor - out.amount_minor).unsigned_abs() as f64;
            dt <= config.round_trip_window_secs
                && diff <= config.round_trip_tolerance * out.amount_minor as f64
        });
        if matched {
            alerts.push(AmlAlert {
                typology: "round_tripping".to_string(),
                detail: format!(
                    "outbound {} matched by a near-equal inbound",
                    out.amount_minor
                ),
            });
            break; // one round-trip alert per window
        }
    }

    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use time::Duration;

    fn tx(
        amount: i64,
        direction: Direction,
        counterparty: &str,
        geo: &str,
        at: OffsetDateTime,
    ) -> AmlTransaction {
        AmlTransaction {
            amount_minor: amount,
            direction,
            counterparty: counterparty.to_string(),
            geography: geo.to_string(),
            at,
        }
    }

    fn has(alerts: &[AmlAlert], typology: &str) -> bool {
        alerts.iter().any(|a| a.typology == typology)
    }

    #[test]
    fn aml_structuring_just_below_ctr_alerts() {
        let t = datetime!(2026-06-17 00:00 UTC);
        // Three $9,500 deposits — just below the $10,000 CTR threshold.
        let txns = vec![
            tx(950_000, Direction::In, "s1", "US", t),
            tx(950_000, Direction::In, "s2", "US", t + Duration::hours(1)),
            tx(950_000, Direction::In, "s3", "US", t + Duration::hours(2)),
        ];
        assert!(has(&evaluate(&txns, &AmlConfig::default()), "structuring"));
    }

    #[test]
    fn aml_funnel_many_geographies_few_destinations() {
        let t = datetime!(2026-06-17 00:00 UTC);
        let txns = vec![
            tx(50_000, Direction::In, "a", "US", t),
            tx(50_000, Direction::In, "b", "GB", t),
            tx(50_000, Direction::In, "c", "DE", t),
            tx(
                140_000,
                Direction::Out,
                "sink",
                "AE",
                t + Duration::hours(1),
            ),
        ];
        assert!(has(&evaluate(&txns, &AmlConfig::default()), "funnel"));
    }

    #[test]
    fn aml_round_tripping_matched_in_out() {
        let t = datetime!(2026-06-17 00:00 UTC);
        // $5,000 out, then ~$5,050 back in within the window.
        let txns = vec![
            tx(500_000, Direction::Out, "offshore", "KY", t),
            tx(
                505_000,
                Direction::In,
                "offshore",
                "KY",
                t + Duration::days(30),
            ),
        ];
        assert!(has(
            &evaluate(&txns, &AmlConfig::default()),
            "round_tripping"
        ));
    }

    #[test]
    fn aml_clean_activity_has_no_alerts() {
        let t = datetime!(2026-06-17 00:00 UTC);
        let txns = vec![
            tx(10_000, Direction::In, "salary", "US", t),
            tx(5_000, Direction::Out, "shop", "US", t + Duration::days(1)),
        ];
        assert!(evaluate(&txns, &AmlConfig::default()).is_empty());
    }
}
