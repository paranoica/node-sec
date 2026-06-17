//! Decisioning (D-005): fuse rule hits (+ the calibrated model score) → risk band → action.
//!
//! [`fuse`] is the rules-only heuristic used by [`RulesDecider`]. [`fuse_ev`] (T032) adds the model
//! score and selects the action by **expected value** over a [`CostMatrix`]; a hard rule override
//! always wins. Wiring `fuse_ev` into the gRPC path (assemble feature vector → `model` score →
//! `fuse_ev`) is the remaining integration step.

use std::sync::Arc;

use domain::{
    Action, Channel, Currency, DeviceId, MerchantId, Money, Pan, RiskBand, Transaction,
    TransactionId, Vertical,
};
use rules::{Disposition, Evaluation, RulesEngine};
use time::OffsetDateTime;

use crate::decide::Decider;
use crate::pb::{DecisionRequest, DecisionResponse};

/// A [`Decider`] backed by the rules engine: it converts the request to a domain transaction,
/// evaluates the rules, and fuses the hits into a banded action with reason codes.
pub struct RulesDecider {
    rules: Arc<RulesEngine>,
}

impl RulesDecider {
    /// Build around a shared rules engine.
    #[must_use]
    pub fn new(rules: Arc<RulesEngine>) -> Self {
        Self { rules }
    }
}

impl Decider for RulesDecider {
    fn decide(&self, req: &DecisionRequest) -> DecisionResponse {
        let txn = request_to_transaction(req);
        let eval = self.rules.evaluate(&txn);
        let (band, action, score) = fuse(&eval);
        let reason_codes = eval
            .hits
            .iter()
            .map(|h| h.reason_code.as_str().to_string())
            .collect();
        DecisionResponse {
            transaction_id: req.transaction_id.clone(),
            action: action_str(action).to_string(),
            score,
            band: band_str(band).to_string(),
            reason_codes,
            rule_version: self.rules.version(),
            model_version: "none".to_string(),
        }
    }
}

/// Fuse a rules evaluation into `(band, action, score)`. Rules-only heuristic: a hard decline wins
/// outright; otherwise the count of soft signals drives the band.
#[must_use]
pub fn fuse(eval: &Evaluation) -> (RiskBand, Action, f64) {
    if eval.is_hard_decline() {
        return (RiskBand::VeryHigh, Action::Decline, 0.99);
    }
    let soft = eval
        .hits
        .iter()
        .filter(|h| h.disposition == Disposition::Soft)
        .count();
    match soft {
        0 => (RiskBand::Low, Action::Approve, 0.02),
        1..=2 => (RiskBand::Medium, Action::StepUp, 0.5),
        _ => (RiskBand::High, Action::Review, 0.8),
    }
}

/// The cost of one action under each true outcome (arbitrary cost units, tuned per business).
#[derive(Debug, Clone, Copy)]
pub struct ActionCost {
    /// Cost if the transaction is actually fraud.
    pub if_fraud: f64,
    /// Cost if the transaction is actually legitimate.
    pub if_legit: f64,
}

/// Expected-value cost matrix: the cost of each action under each true outcome (D-005).
#[derive(Debug, Clone, Copy)]
pub struct CostMatrix {
    /// Cost of approving.
    pub approve: ActionCost,
    /// Cost of declining.
    pub decline: ActionCost,
    /// Cost of a step-up challenge.
    pub step_up: ActionCost,
    /// Cost of routing to review.
    pub review: ActionCost,
    /// Cost of a hold.
    pub hold: ActionCost,
}

impl Default for CostMatrix {
    fn default() -> Self {
        Self {
            approve: ActionCost {
                if_fraud: 100.0,
                if_legit: 0.0,
            },
            decline: ActionCost {
                if_fraud: 0.0,
                if_legit: 30.0,
            },
            step_up: ActionCost {
                if_fraud: 20.0,
                if_legit: 5.0,
            },
            review: ActionCost {
                if_fraud: 8.0,
                if_legit: 15.0,
            },
            hold: ActionCost {
                if_fraud: 5.0,
                if_legit: 18.0,
            },
        }
    }
}

impl CostMatrix {
    fn expected_cost(cost: ActionCost, p_fraud: f64) -> f64 {
        p_fraud * cost.if_fraud + (1.0 - p_fraud) * cost.if_legit
    }
}

fn band_for_score(score: f64) -> RiskBand {
    if score < 0.3 {
        RiskBand::Low
    } else if score < 0.6 {
        RiskBand::Medium
    } else if score < 0.85 {
        RiskBand::High
    } else {
        RiskBand::VeryHigh
    }
}

/// Fuse rule signals and the calibrated model score, then select the action by expected value over
/// the cost matrix (D-005). A hard rule override always wins; otherwise soft signals raise the fraud
/// probability and the minimum-expected-cost action is chosen. Returns `(band, action, fused_score)`.
#[must_use]
pub fn fuse_ev(eval: &Evaluation, model_score: f64, costs: &CostMatrix) -> (RiskBand, Action, f64) {
    // Fail safe on a non-finite model score: NaN/inf would propagate through `clamp` (which passes
    // NaN through) and make every expected cost NaN, panicking the `min_by` comparator. Treat a
    // non-finite score as maximum risk rather than crashing the decision.
    let model_score = if model_score.is_finite() {
        model_score
    } else {
        1.0
    };
    if eval.is_hard_decline() {
        return (RiskBand::VeryHigh, Action::Decline, model_score.max(0.99));
    }
    let soft = eval
        .hits
        .iter()
        .filter(|h| h.disposition == Disposition::Soft)
        .count();
    let p_fraud = (model_score + 0.1 * soft as f64).clamp(0.0, 1.0);

    let candidates = [
        (Action::Approve, costs.approve),
        (Action::Decline, costs.decline),
        (Action::StepUp, costs.step_up),
        (Action::Review, costs.review),
        (Action::Hold, costs.hold),
    ];
    let (action, _) = candidates
        .into_iter()
        .map(|(action, cost)| (action, CostMatrix::expected_cost(cost, p_fraud)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).expect("finite costs"))
        .expect("non-empty candidate set");

    (band_for_score(p_fraud), action, p_fraud)
}

fn action_str(action: Action) -> &'static str {
    match action {
        Action::Approve => "APPROVE",
        Action::Decline => "DECLINE",
        Action::StepUp => "STEP_UP",
        Action::Review => "REVIEW",
        Action::Hold => "HOLD",
    }
}

fn band_str(band: RiskBand) -> &'static str {
    match band {
        RiskBand::Low => "LOW",
        RiskBand::Medium => "MEDIUM",
        RiskBand::High => "HIGH",
        RiskBand::VeryHigh => "VERY_HIGH",
    }
}

/// Convert a gRPC request to a domain transaction. Lenient: unknown enum strings fall back to sane
/// defaults (the simulator emits valid values; production callers are validated upstream). MCC, AVS,
/// CVV and geo are not yet carried on the request wire, so those rules stay dormant for gRPC traffic
/// until the request schema is enriched.
fn request_to_transaction(req: &DecisionRequest) -> Transaction {
    let occurred_at =
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(req.occurred_at_unix_ms) * 1_000_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);

    let mut txn = Transaction::new(
        TransactionId::new(req.transaction_id.clone()),
        Money::from_minor_units(req.amount_minor_units, parse_currency(&req.currency)),
        occurred_at,
        parse_vertical(&req.vertical),
        parse_channel(&req.channel),
    );
    if !req.pan.is_empty() {
        txn = txn.with_pan(Pan::new(req.pan.clone()));
    }
    if !req.merchant.is_empty() {
        txn = txn.with_merchant(MerchantId::new(req.merchant.clone()));
    }
    if !req.device.is_empty() {
        txn = txn.with_device(DeviceId::new(req.device.clone()));
    }
    txn
}

fn parse_currency(code: &str) -> Currency {
    match code {
        "EUR" => Currency::Eur,
        "GBP" => Currency::Gbp,
        "JPY" => Currency::Jpy,
        _ => Currency::Usd,
    }
}

fn parse_vertical(name: &str) -> Vertical {
    match name {
        "P2P" => Vertical::P2p,
        "CRYPTO" => Vertical::Crypto,
        _ => Vertical::Card,
    }
}

fn parse_channel(name: &str) -> Channel {
    match name {
        "CARD_PRESENT" => Channel::CardPresent,
        "P2P_PUSH" => Channel::P2pPush,
        "CRYPTO_WITHDRAWAL" => Channel::CryptoWithdrawal,
        "CRYPTO_DEPOSIT" => Channel::CryptoDeposit,
        _ => Channel::CardNotPresent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ReasonCode;
    use rules::{Blocklists, RuleHit, RulesConfig};

    fn soft_eval(n: usize) -> Evaluation {
        let hits = (0..n)
            .map(|i| RuleHit {
                rule_id: format!("r{i}"),
                reason_code: ReasonCode::new("SOFT"),
                typology: "t".to_string(),
                disposition: Disposition::Soft,
            })
            .collect();
        Evaluation { hits }
    }

    #[test]
    fn clean_evaluation_approves() {
        let (band, action, _) = fuse(&Evaluation::default());
        assert_eq!(action, Action::Approve);
        assert_eq!(band, RiskBand::Low);
    }

    #[test]
    fn one_or_two_soft_signals_step_up() {
        assert_eq!(fuse(&soft_eval(1)).1, Action::StepUp);
        assert_eq!(fuse(&soft_eval(2)).1, Action::StepUp);
    }

    #[test]
    fn three_soft_signals_route_to_review() {
        assert_eq!(fuse(&soft_eval(3)).1, Action::Review);
    }

    #[test]
    fn hard_decline_declines() {
        let eval = Evaluation {
            hits: vec![RuleHit {
                rule_id: "blocklist.bin".to_string(),
                reason_code: ReasonCode::new("BLOCKLIST_BIN"),
                typology: "blocklist".to_string(),
                disposition: Disposition::HardDecline,
            }],
        };
        let (band, action, _) = fuse(&eval);
        assert_eq!(action, Action::Decline);
        assert_eq!(band, RiskBand::VeryHigh);
    }

    fn hard_decline_eval() -> Evaluation {
        Evaluation {
            hits: vec![RuleHit {
                rule_id: "blocklist.bin".to_string(),
                reason_code: ReasonCode::new("BLOCKLIST_BIN"),
                typology: "blocklist".to_string(),
                disposition: Disposition::HardDecline,
            }],
        }
    }

    #[test]
    fn fusion_ev_hard_override_always_declines() {
        // Even with a near-zero model score, a hard rule override wins.
        let (_, action, _) = fuse_ev(&hard_decline_eval(), 0.001, &CostMatrix::default());
        assert_eq!(action, Action::Decline);
    }

    #[test]
    fn fusion_ev_low_score_approves() {
        let (band, action, _) = fuse_ev(&Evaluation::default(), 0.02, &CostMatrix::default());
        assert_eq!(action, Action::Approve);
        assert_eq!(band, RiskBand::Low);
    }

    #[test]
    fn fusion_ev_high_score_declines() {
        let (band, action, score) = fuse_ev(&Evaluation::default(), 0.95, &CostMatrix::default());
        assert_eq!(action, Action::Decline);
        assert_eq!(band, RiskBand::VeryHigh);
        assert!((score - 0.95).abs() < 1e-9);
    }

    #[test]
    fn fusion_ev_nan_score_does_not_panic() {
        // A non-finite model score must not panic the comparator; it is treated as max risk.
        let (_, action, score) = fuse_ev(&Evaluation::default(), f64::NAN, &CostMatrix::default());
        assert_eq!(action, Action::Decline);
        assert!(score.is_finite());
    }

    #[test]
    fn fusion_ev_picks_minimum_expected_cost_at_mid_score() {
        // Mid risk: neither approve (fraud loss) nor decline (friction) is optimal → review/hold.
        let (_, action, _) = fuse_ev(&Evaluation::default(), 0.5, &CostMatrix::default());
        assert!(matches!(action, Action::Review | Action::Hold));
    }

    fn req(pan: &str) -> DecisionRequest {
        DecisionRequest {
            idempotency_key: "k".to_string(),
            transaction_id: "txn-1".to_string(),
            amount_minor_units: 4_999,
            currency: "USD".to_string(),
            vertical: "CARD".to_string(),
            channel: "CARD_NOT_PRESENT".to_string(),
            pan: pan.to_string(),
            merchant: "mrc-1".to_string(),
            device: "dev-1".to_string(),
            occurred_at_unix_ms: 1_780_000_000_000,
        }
    }

    #[test]
    fn blocked_bin_request_is_declined_with_reason() {
        let cfg = RulesConfig {
            version: "t".to_string(),
            blocklists: Blocklists {
                bins: vec!["411111".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let decider = RulesDecider::new(Arc::new(RulesEngine::from_config(cfg)));
        let resp = decider.decide(&req("4111110000001234"));
        assert_eq!(resp.action, "DECLINE");
        assert!(resp.reason_codes.contains(&"BLOCKLIST_BIN".to_string()));
    }

    #[test]
    fn clean_request_is_approved() {
        let decider = RulesDecider::new(Arc::new(RulesEngine::from_config(RulesConfig::default())));
        let resp = decider.decide(&req("4222220000001234"));
        assert_eq!(resp.action, "APPROVE");
        assert!(resp.reason_codes.is_empty());
    }
}
