//! The decision logic seam.
//!
//! [`Decider`] maps a request to a verdict; the real rules engine (T011) and model (T031) plug in
//! here later. For T010 the only implementation is [`ApproveAllDecider`], a stand-in that approves
//! everything and counts its invocations so idempotent replay (which must not re-decide) is
//! observable in tests.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::pb::{DecisionRequest, DecisionResponse};

/// Maps a decision request to a verdict.
pub trait Decider: Send + Sync {
    /// Compute a verdict for `req`. Implementations must be pure with respect to the engine's
    /// idempotency cache (the engine guarantees this runs at most once per idempotency key).
    fn decide(&self, req: &DecisionRequest) -> DecisionResponse;
}

/// The async decision seam the gRPC engine drives — the model-backed path reads online features
/// asynchronously, so the live decider is async. Sync deciders adapt trivially (see the
/// [`ApproveAllDecider`] impl).
#[tonic::async_trait]
pub trait AsyncDecider: Send + Sync {
    /// Compute a verdict for `req`.
    async fn decide(&self, req: &DecisionRequest) -> DecisionResponse;
}

#[tonic::async_trait]
impl AsyncDecider for ApproveAllDecider {
    async fn decide(&self, req: &DecisionRequest) -> DecisionResponse {
        Decider::decide(self, req)
    }
}

/// A placeholder decider that approves every transaction. Counts invocations.
#[derive(Debug, Default)]
pub struct ApproveAllDecider {
    calls: AtomicU64,
}

impl ApproveAllDecider {
    /// Number of times [`Decider::decide`] has run — used to assert idempotent replay does not
    /// re-decide.
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Decider for ApproveAllDecider {
    fn decide(&self, req: &DecisionRequest) -> DecisionResponse {
        self.calls.fetch_add(1, Ordering::Relaxed);
        DecisionResponse {
            transaction_id: req.transaction_id.clone(),
            action: "APPROVE".to_string(),
            score: 0.0,
            band: "LOW".to_string(),
            reason_codes: Vec::new(),
            rule_version: "rules-v0".to_string(),
            model_version: "none".to_string(),
        }
    }
}
