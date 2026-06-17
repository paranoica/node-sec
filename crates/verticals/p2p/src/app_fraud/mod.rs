//! Authorised-Push-Payment (APP) fraud signals: new-payee + Confirmation of Payee (T060).
//!
//! APP fraud is a *legitimate* push the payer was socially engineered into authorising, so the
//! card-style "was this really you?" question misses it. The tell is the **payee**: a first-ever
//! payment to a new beneficiary, larger than anything the payer has sent before, to an account the
//! payer's peers don't commonly pay. That triple fires a **Confirmation of Payee** (name/account
//! match) check and a **step-up** before the funds leave.

use std::collections::HashSet;

use domain::{Action, ReasonCode};
use rules::engine::{Disposition, RuleHit};

/// What the payer has done before — the baseline a new payee is judged against.
#[derive(Debug, Clone, Default)]
pub struct PayerHistory {
    /// Largest single payment the payer has previously sent (minor units).
    pub max_payment_minor: i64,
    /// Payees the payer has paid before.
    pub known_payees: HashSet<String>,
}

/// How the payee looks across the wider population.
#[derive(Debug, Clone, Default)]
pub struct PeerContext {
    /// How many of the payer's peers commonly pay this payee.
    pub peer_payment_count: u64,
}

/// A P2P payment under evaluation.
#[derive(Debug, Clone)]
pub struct P2pPayment {
    /// The paying account.
    pub payer: String,
    /// The beneficiary account.
    pub payee: String,
    /// Amount (minor units).
    pub amount_minor: i64,
}

/// Tunables (data, not code).
#[derive(Debug, Clone)]
pub struct P2pConfig {
    /// At/above this many peer payments, a payee is "commonly paid" and the signal is suppressed.
    pub peer_common_threshold: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            peer_common_threshold: 5,
        }
    }
}

/// The APP-fraud signal, expressed in the shared vocabulary the engine already consumes.
#[derive(Debug, Clone)]
pub struct P2pSignal {
    /// The rule firing (rules vocabulary).
    pub hit: RuleHit,
    /// The recommended action (domain vocabulary).
    pub action: Action,
    /// Whether to run a Confirmation-of-Payee check.
    pub confirm_payee: bool,
}

/// Evaluate APP-fraud risk for a P2P payment.
///
/// Fires only when the payment is to a **new** payee, **exceeds** the payer's historical maximum,
/// and the payee is **not** commonly paid by peers — then recommends a CoP check and a step-up.
#[must_use]
pub fn evaluate_app_fraud(
    payment: &P2pPayment,
    history: &PayerHistory,
    peer: &PeerContext,
    config: &P2pConfig,
) -> Option<P2pSignal> {
    let is_new_payee = !history.known_payees.contains(&payment.payee);
    let exceeds_max = payment.amount_minor > history.max_payment_minor;
    let commonly_paid = peer.peer_payment_count >= config.peer_common_threshold;

    if is_new_payee && exceeds_max && !commonly_paid {
        Some(P2pSignal {
            hit: RuleHit {
                rule_id: "p2p.app_fraud.new_payee".to_string(),
                reason_code: ReasonCode::new("P2P_APP_NEW_PAYEE"),
                typology: "app_fraud".to_string(),
                disposition: Disposition::Soft,
            },
            action: Action::StepUp,
            confirm_payee: true,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> PayerHistory {
        PayerHistory {
            max_payment_minor: 50_000, // £500 historical max
            known_payees: HashSet::from(["landlord".to_string()]),
        }
    }

    #[test]
    fn app_fraud_new_large_uncommon_payee_triggers_cop_and_stepup() {
        let payment = P2pPayment {
            payer: "alice".into(),
            payee: "scammer-mule".into(),
            amount_minor: 800_000, // £8,000 — far above the £500 max
        };
        let signal = evaluate_app_fraud(
            &payment,
            &history(),
            &PeerContext {
                peer_payment_count: 0,
            },
            &P2pConfig::default(),
        )
        .expect("signal");
        assert!(signal.confirm_payee);
        assert_eq!(signal.action, Action::StepUp);
        assert_eq!(signal.hit.typology, "app_fraud");
    }

    #[test]
    fn app_fraud_known_payee_does_not_trigger() {
        let payment = P2pPayment {
            payer: "alice".into(),
            payee: "landlord".into(), // previously paid
            amount_minor: 800_000,
        };
        assert!(evaluate_app_fraud(
            &payment,
            &history(),
            &PeerContext::default(),
            &P2pConfig::default()
        )
        .is_none());
    }

    #[test]
    fn app_fraud_within_historical_max_does_not_trigger() {
        let payment = P2pPayment {
            payer: "alice".into(),
            payee: "new-shop".into(),
            amount_minor: 40_000, // below the £500 max
        };
        assert!(evaluate_app_fraud(
            &payment,
            &history(),
            &PeerContext::default(),
            &P2pConfig::default()
        )
        .is_none());
    }

    #[test]
    fn app_fraud_peer_common_payee_does_not_trigger() {
        let payment = P2pPayment {
            payer: "alice".into(),
            payee: "popular-merchant".into(),
            amount_minor: 800_000,
        };
        // Commonly paid by peers — a legitimate popular destination, not APP fraud.
        assert!(evaluate_app_fraud(
            &payment,
            &history(),
            &PeerContext {
                peer_payment_count: 50,
            },
            &P2pConfig::default()
        )
        .is_none());
    }
}
