//! Coercion/behavioural P2P signals + recipient-side mule freeze (T061).
//!
//! Two complementary defences for the same scam:
//!
//! * **Payer side** — a genuine push made under duress. The biometric tell is *coached entry*:
//!   the payment starts shortly after an inbound call, with segmented (dictated) typing or long
//!   session dead-time (the victim listening to instructions). That fires a **hold** plus a
//!   coercion-specific warning, breaking the scammer's real-time grip.
//! * **Recipient side** — when the beneficiary account scores as a likely mule, the inbound credit
//!   is **held/frozen** and a **SAR** is raised, so laundered funds don't simply pass through.

use domain::{Action, ReasonCode};
use rules::engine::{Disposition, RuleHit};

/// Behavioural-biometric signals captured during the payment session.
#[derive(Debug, Clone, Default)]
pub struct BehaviorSignals {
    /// The payment was initiated shortly after an inbound call.
    pub recent_inbound_call: bool,
    /// Segmented / dictated typing cadence (entry coached over the phone).
    pub segmented_typing: bool,
    /// Total session dead-time in seconds (pauses spent listening to instructions).
    pub session_dead_time_secs: f64,
}

/// Tunables (data, not code).
#[derive(Debug, Clone)]
pub struct CoercionConfig {
    /// Dead-time at/above which the session looks coached.
    pub dead_time_threshold_secs: f64,
    /// Mule score at/above which inbound credit is frozen and a SAR is raised.
    pub mule_freeze_threshold: f64,
}

impl Default for CoercionConfig {
    fn default() -> Self {
        Self {
            dead_time_threshold_secs: 20.0,
            mule_freeze_threshold: 0.5, // mirrors `graph::mule` decision threshold
        }
    }
}

/// A coercion signal — a hold plus a coercion-specific warning.
#[derive(Debug, Clone)]
pub struct CoercionSignal {
    /// The rule firing (rules vocabulary).
    pub hit: RuleHit,
    /// The recommended action (domain vocabulary).
    pub action: Action,
    /// The coercion-specific warning shown to the payer.
    pub warning: String,
}

/// Evaluate payer-side coercion from session behaviour.
///
/// Fires only when the payment follows an inbound call **and** shows coached entry (segmented
/// typing or dead-time over threshold) — then recommends a hold and a coercion warning.
#[must_use]
pub fn evaluate_coercion(
    signals: &BehaviorSignals,
    config: &CoercionConfig,
) -> Option<CoercionSignal> {
    let coached = signals.segmented_typing
        || signals.session_dead_time_secs >= config.dead_time_threshold_secs;

    if signals.recent_inbound_call && coached {
        Some(CoercionSignal {
            hit: RuleHit {
                rule_id: "p2p.coercion.coached_payment".to_string(),
                reason_code: ReasonCode::new("P2P_COERCION"),
                typology: "coercion".to_string(),
                disposition: Disposition::Soft,
            },
            action: Action::Hold,
            warning: "Payment started during a call with coached-entry behaviour — \
                      this can be a scam. We've paused it so you can check."
                .to_string(),
        })
    } else {
        None
    }
}

/// The recipient-side outcome: whether to freeze the inbound credit and raise a SAR.
#[derive(Debug, Clone, Default)]
pub struct RecipientOutcome {
    /// Hold/freeze the inbound credit.
    pub freeze_credit: bool,
    /// Raise a SAR on the recipient.
    pub raise_sar: bool,
    /// The rule firing, when frozen.
    pub hit: Option<RuleHit>,
}

/// Evaluate the recipient account: freeze inbound credit and raise a SAR if it scores as a mule.
#[must_use]
pub fn evaluate_recipient(mule_score: f64, config: &CoercionConfig) -> RecipientOutcome {
    if mule_score >= config.mule_freeze_threshold {
        RecipientOutcome {
            freeze_credit: true,
            raise_sar: true,
            hit: Some(RuleHit {
                rule_id: "p2p.recipient.mule_freeze".to_string(),
                reason_code: ReasonCode::new("P2P_MULE_FREEZE"),
                typology: "mule".to_string(),
                disposition: Disposition::HardDecline,
            }),
        }
    } else {
        RecipientOutcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coercion_coached_payment_after_call_holds() {
        let signals = BehaviorSignals {
            recent_inbound_call: true,
            segmented_typing: true,
            session_dead_time_secs: 3.0,
        };
        let signal = evaluate_coercion(&signals, &CoercionConfig::default()).expect("signal");
        assert_eq!(signal.action, Action::Hold);
        assert_eq!(signal.hit.typology, "coercion");
        assert!(!signal.warning.is_empty());
    }

    #[test]
    fn coercion_long_dead_time_after_call_holds() {
        let signals = BehaviorSignals {
            recent_inbound_call: true,
            segmented_typing: false,
            session_dead_time_secs: 45.0, // long pauses listening to a coach
        };
        assert!(evaluate_coercion(&signals, &CoercionConfig::default()).is_some());
    }

    #[test]
    fn coercion_without_inbound_call_does_not_trigger() {
        // Coached-looking behaviour but no call — not the coercion pattern.
        let signals = BehaviorSignals {
            recent_inbound_call: false,
            segmented_typing: true,
            session_dead_time_secs: 60.0,
        };
        assert!(evaluate_coercion(&signals, &CoercionConfig::default()).is_none());
    }

    #[test]
    fn coercion_recipient_mule_freezes_credit_and_raises_sar() {
        let outcome = evaluate_recipient(0.8, &CoercionConfig::default());
        assert!(outcome.freeze_credit);
        assert!(outcome.raise_sar);
        assert_eq!(outcome.hit.unwrap().disposition, Disposition::HardDecline);
    }

    #[test]
    fn coercion_recipient_clean_account_is_not_frozen() {
        let outcome = evaluate_recipient(0.1, &CoercionConfig::default());
        assert!(!outcome.freeze_credit);
        assert!(!outcome.raise_sar);
        assert!(outcome.hit.is_none());
    }
}
