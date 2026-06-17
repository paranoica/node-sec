//! The decision — the verdict emitted for a [`crate::transaction::Transaction`].
//!
//! A [`Decision`] records the rule and model versions that produced it (`arch:versioned-decision`)
//! so it can be deterministically replayed from the audit log.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ids::TransactionId;

/// The operational outcome of a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    /// Let the transaction through.
    Approve,
    /// Hard block.
    Decline,
    /// Escalate to a challenge (3DS / OTP / out-of-band).
    StepUp,
    /// Send to the analyst review queue.
    Review,
    /// Delay / freeze pending review.
    Hold,
}

/// A coarse risk band a score maps into; drives the [`Action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskBand {
    /// Lowest risk.
    Low,
    /// Moderate risk.
    Medium,
    /// High risk.
    High,
    /// Highest risk.
    VeryHigh,
}

/// A stable, human-readable reason code attached to a decision (rule hit or model contribution).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReasonCode(String);

impl ReasonCode {
    /// Wrap a reason-code string (e.g. `"VELOCITY_CARD_5M"`).
    #[must_use]
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// Borrow the underlying code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A calibrated fraud probability in `[0.0, 1.0]`.
///
/// Not money — a probability is legitimately fractional (`arch:money-integer` applies to
/// [`crate::money::Money`], not to scores). The range invariant is enforced on construction **and**
/// on deserialisation, so an out-of-range value can never enter the system from the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RiskScore(f64);

impl RiskScore {
    /// Create a score, returning `None` unless `p` is finite and within `[0.0, 1.0]`.
    #[must_use]
    pub fn new(p: f64) -> Option<Self> {
        (p.is_finite() && (0.0..=1.0).contains(&p)).then_some(Self(p))
    }

    /// The probability value.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for RiskScore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let p = f64::deserialize(deserializer)?;
        RiskScore::new(p).ok_or_else(|| serde::de::Error::custom("risk score out of [0.0, 1.0]"))
    }
}

/// The verdict for one transaction. Immutable once emitted; the audit log stores it for replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// The transaction this decision is about.
    pub transaction_id: TransactionId,
    /// The selected action.
    pub action: Action,
    /// The calibrated fraud probability that drove the band.
    pub score: RiskScore,
    /// The risk band the score mapped into.
    pub band: RiskBand,
    /// The dominant contributing reason codes.
    pub reason_codes: Vec<ReasonCode>,
    /// Version of the rule configuration that produced this decision (`arch:versioned-decision`).
    pub rule_version: String,
    /// Version of the model that produced this decision (`arch:versioned-decision`).
    pub model_version: String,
    /// When the decision was made (UTC).
    pub decided_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn risk_score_validates_range() {
        assert!(RiskScore::new(0.0).is_some());
        assert!(RiskScore::new(1.0).is_some());
        assert!(RiskScore::new(0.42).is_some());
        assert!(RiskScore::new(-0.01).is_none());
        assert!(RiskScore::new(1.01).is_none());
        assert!(RiskScore::new(f64::NAN).is_none());
        assert!(RiskScore::new(f64::INFINITY).is_none());
    }

    #[test]
    fn risk_band_orders_low_to_high() {
        assert!(RiskBand::Low < RiskBand::Medium);
        assert!(RiskBand::High < RiskBand::VeryHigh);
    }

    fn sample() -> Decision {
        Decision {
            transaction_id: TransactionId::new("txn-1"),
            action: Action::Review,
            score: RiskScore::new(0.87).unwrap(),
            band: RiskBand::High,
            reason_codes: vec![
                ReasonCode::new("VELOCITY_CARD_5M"),
                ReasonCode::new("GEO_IMPOSSIBLE"),
            ],
            rule_version: "rules-2026-06-17".to_string(),
            model_version: "lgbm-card-v1".to_string(),
            decided_at: datetime!(2026-06-17 12:00:01 UTC),
        }
    }

    #[test]
    fn decision_serde_roundtrip() {
        let d = sample();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Decision>(&json).unwrap(), d);
    }

    #[test]
    fn deserialising_invalid_score_is_rejected() {
        // A tampered/corrupt audit record with an out-of-range score must not deserialise.
        let mut d = sample();
        d.rule_version = "x".to_string();
        let json = serde_json::to_string(&d).unwrap().replace("0.87", "1.5");
        assert!(serde_json::from_str::<Decision>(&json).is_err());
    }
}
