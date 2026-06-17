//! Delayed labels, two label streams, and the random-control holdout (T034; D-012).
//!
//! Models the label reality fraud systems face: a fraudulent transaction that is approved only
//! reveals itself as a **chargeback** weeks later (slow, authoritative), while a sampled subset gets
//! a fast, noisy **investigator** label. Declined transactions normally produce no outcome
//! (censored) — except those in the **random-control holdout**, which are scored but not acted on so
//! their true outcome is always observed, yielding unbiased labels.

use domain::{Action, Transaction};
use time::{Duration, OffsetDateTime};

/// Which stream a label came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelSource {
    /// Fast, noisy analyst label.
    Investigator,
    /// Slow, authoritative chargeback (or chargeback-window-closed-legit).
    Chargeback,
}

/// The label value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelValue {
    /// Confirmed fraud.
    Fraud,
    /// Confirmed (or presumed) legitimate.
    Legit,
}

/// A label that becomes known at `available_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The transaction the label concerns.
    pub transaction_id: String,
    /// Fraud or legit.
    pub value: LabelValue,
    /// Which stream produced it.
    pub source: LabelSource,
    /// When the label becomes known (delayed-label clock).
    pub available_at: OffsetDateTime,
}

/// The labelling outcome for one decided transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabeledOutcome {
    /// True if the transaction is in the random-control holdout (scored but not acted on).
    pub holdout: bool,
    /// The labels generated (0..2): an investigator label and/or a chargeback label.
    pub labels: Vec<Label>,
}

/// Configuration for the labeller.
#[derive(Debug, Clone)]
pub struct LabelConfig {
    /// Delay before a chargeback label becomes known.
    pub chargeback_delay: Duration,
    /// Delay before an investigator label becomes known.
    pub investigator_delay: Duration,
    /// Fraction of transactions that receive an investigator label.
    pub investigator_sample_rate: f64,
    /// Probability the investigator label is wrong (noise).
    pub investigator_error_rate: f64,
    /// Fraction of transactions in the random-control holdout.
    pub holdout_rate: f64,
}

impl Default for LabelConfig {
    fn default() -> Self {
        Self {
            chargeback_delay: Duration::days(45),
            investigator_delay: Duration::hours(6),
            investigator_sample_rate: 0.05,
            investigator_error_rate: 0.1,
            holdout_rate: 0.01,
        }
    }
}

/// Generates labels for decided transactions per [`LabelConfig`], deterministically by transaction id.
#[derive(Debug, Clone)]
pub struct Labeler {
    config: LabelConfig,
}

impl Labeler {
    /// Build a labeller.
    #[must_use]
    pub fn new(config: LabelConfig) -> Self {
        Self { config }
    }

    /// Whether a transaction is in the random-control holdout.
    #[must_use]
    pub fn is_holdout(&self, transaction_id: &str) -> bool {
        fraction(transaction_id, HOLDOUT_SALT) < self.config.holdout_rate
    }

    /// Generate the labels for a decided transaction given its ground-truth fraud flag and action.
    #[must_use]
    pub fn label(&self, txn: &Transaction, is_fraud: bool, action: Action) -> LabeledOutcome {
        let id = txn.id.as_str();
        let holdout = self.is_holdout(id);
        // The true outcome is observable only if the transaction actually went through: it was
        // approved, or it is a holdout (scored-but-not-acted → let through for measurement).
        let went_through = holdout || matches!(action, Action::Approve);

        let mut labels = Vec::new();

        if went_through {
            labels.push(Label {
                transaction_id: id.to_string(),
                value: if is_fraud {
                    LabelValue::Fraud
                } else {
                    LabelValue::Legit
                },
                source: LabelSource::Chargeback,
                available_at: txn.occurred_at + self.config.chargeback_delay,
            });
        }

        // Investigator stream: a sampled subset gets a fast, possibly-wrong label regardless of action.
        if fraction(id, INVEST_SALT) < self.config.investigator_sample_rate {
            let wrong = fraction(id, FLIP_SALT) < self.config.investigator_error_rate;
            let truth = if is_fraud {
                LabelValue::Fraud
            } else {
                LabelValue::Legit
            };
            let value = if wrong { flip(truth) } else { truth };
            labels.push(Label {
                transaction_id: id.to_string(),
                value,
                source: LabelSource::Investigator,
                available_at: txn.occurred_at + self.config.investigator_delay,
            });
        }

        LabeledOutcome { holdout, labels }
    }
}

fn flip(value: LabelValue) -> LabelValue {
    match value {
        LabelValue::Fraud => LabelValue::Legit,
        LabelValue::Legit => LabelValue::Fraud,
    }
}

const HOLDOUT_SALT: u64 = 0x1111_1111;
const INVEST_SALT: u64 = 0x2222_2222;
const FLIP_SALT: u64 = 0x3333_3333;

/// A deterministic fraction in `[0, 1)` from a string + salt (FNV-1a), for stable sampling.
fn fraction(s: &str, salt: u64) -> f64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ salt;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    f64::from((hash % 10_000) as u32) / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Channel, Currency, Money, TransactionId, Vertical};
    use time::macros::datetime;

    fn txn(id: &str) -> Transaction {
        Transaction::new(
            TransactionId::new(id),
            Money::from_minor_units(5_000, Currency::Usd),
            datetime!(2026-06-17 00:00 UTC),
            Vertical::Card,
            Channel::CardNotPresent,
        )
    }

    fn cfg() -> LabelConfig {
        LabelConfig::default()
    }

    #[test]
    fn labels_fraud_approved_gets_a_delayed_chargeback() {
        let labeler = Labeler::new(cfg());
        let t = txn("txn-1");
        let out = labeler.label(&t, true, Action::Approve);
        let cb = out
            .labels
            .iter()
            .find(|l| l.source == LabelSource::Chargeback)
            .expect("approved fraud must produce a chargeback");
        assert_eq!(cb.value, LabelValue::Fraud);
        assert_eq!(cb.available_at, t.occurred_at + Duration::days(45));
    }

    #[test]
    fn labels_declined_non_holdout_is_censored() {
        // No holdout, no investigator sampling → a decline yields no observable label.
        let labeler = Labeler::new(LabelConfig {
            holdout_rate: 0.0,
            investigator_sample_rate: 0.0,
            ..cfg()
        });
        let out = labeler.label(&txn("txn-2"), true, Action::Decline);
        assert!(out.labels.is_empty(), "a censored decline has no labels");
        assert!(!out.holdout);
    }

    #[test]
    fn labels_holdout_observes_outcome_even_when_declined() {
        // Everything in the holdout → outcome observed despite the decline (unbiased label).
        let labeler = Labeler::new(LabelConfig {
            holdout_rate: 1.0,
            investigator_sample_rate: 0.0,
            ..cfg()
        });
        let out = labeler.label(&txn("txn-3"), true, Action::Decline);
        assert!(out.holdout);
        assert_eq!(out.labels.len(), 1);
        assert_eq!(out.labels[0].source, LabelSource::Chargeback);
        assert_eq!(out.labels[0].value, LabelValue::Fraud);
    }

    #[test]
    fn labels_two_streams_investigator_is_faster_than_chargeback() {
        let labeler = Labeler::new(LabelConfig {
            investigator_sample_rate: 1.0,
            investigator_error_rate: 0.0,
            ..cfg()
        });
        let out = labeler.label(&txn("txn-4"), true, Action::Approve);
        let inv = out
            .labels
            .iter()
            .find(|l| l.source == LabelSource::Investigator)
            .unwrap();
        let cb = out
            .labels
            .iter()
            .find(|l| l.source == LabelSource::Chargeback)
            .unwrap();
        assert!(
            inv.available_at < cb.available_at,
            "investigator label arrives first"
        );
        assert_eq!(inv.value, LabelValue::Fraud);
    }
}
