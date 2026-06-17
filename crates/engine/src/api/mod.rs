//! The gRPC decision service and its idempotency cache (D-001 sync path, D-016 idempotency).
// tonic's contract is to return `Status` by value; boxing it to satisfy result_large_err would
// fight the framework on every handler, so allow the large-err result here.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tonic::{Request, Response, Status};

use crate::decide::Decider;
use crate::pb::decision_service_server::{DecisionService, DecisionServiceServer};
use crate::pb::{DecisionRequest, DecisionResponse};

/// The decision engine: a [`Decider`] behind an idempotency cache so a retried request returns the
/// original verdict and never re-decides or double-updates state (`arch:idempotent-decision`).
///
/// # Limitation (T010 skeleton)
/// `cache` is an **unbounded** in-process map: it never evicts, so it grows without limit and is an
/// out-of-memory vector under sustained load. This is deliberately deferred — the production
/// idempotency store must be **bounded with a TTL** matched to the client retry window (and is the
/// natural home for the Redis online store from D-006/T021). Do not put this path under sustained
/// load (T016 latency harness, T065 load test) until that store replaces this map.
pub struct DecisionEngine {
    decider: Arc<dyn Decider>,
    cache: Mutex<HashMap<String, DecisionResponse>>,
}

impl DecisionEngine {
    /// Build an engine around a decider.
    #[must_use]
    pub fn new(decider: Arc<dyn Decider>) -> Self {
        Self {
            decider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Decide for a request, honoring idempotency. Separated from the gRPC trait so it is callable
    /// directly in tests without a transport.
    ///
    /// # Errors
    /// Returns `InvalidArgument` if `idempotency_key` is empty — a decision request must be
    /// idempotent.
    pub fn decide_idempotent(&self, req: &DecisionRequest) -> Result<DecisionResponse, Status> {
        if req.idempotency_key.is_empty() {
            return Err(Status::invalid_argument("idempotency_key is required"));
        }

        // Fast path: a previously decided key returns its original verdict — no re-decide, no write.
        if let Some(existing) = self
            .cache
            .lock()
            .expect("idempotency cache poisoned")
            .get(&req.idempotency_key)
        {
            return Ok(existing.clone());
        }

        let fresh = self.decider.decide(req);
        let mut cache = self.cache.lock().expect("idempotency cache poisoned");
        // A concurrent request with the same key may have inserted first; keep the first verdict so
        // one key maps to exactly one decision.
        let stored = cache
            .entry(req.idempotency_key.clone())
            .or_insert(fresh)
            .clone();
        Ok(stored)
    }

    /// Wrap into a mountable tonic server service.
    #[must_use]
    pub fn into_server(self) -> DecisionServiceServer<Self> {
        DecisionServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl DecisionService for DecisionEngine {
    async fn decide(
        &self,
        request: Request<DecisionRequest>,
    ) -> Result<Response<DecisionResponse>, Status> {
        let verdict = self.decide_idempotent(request.get_ref())?;
        Ok(Response::new(verdict))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decide::ApproveAllDecider;

    fn req(key: &str) -> DecisionRequest {
        DecisionRequest {
            idempotency_key: key.to_string(),
            transaction_id: "txn-1".to_string(),
            amount_minor_units: 4_999,
            currency: "USD".to_string(),
            vertical: "CARD".to_string(),
            channel: "CARD_NOT_PRESENT".to_string(),
            pan: "4111111111111111".to_string(),
            merchant: "mrc-1".to_string(),
            device: "dev-1".to_string(),
        }
    }

    #[test]
    fn returns_a_decision() {
        let engine = DecisionEngine::new(Arc::new(ApproveAllDecider::default()));
        let resp = engine.decide_idempotent(&req("k1")).unwrap();
        assert_eq!(resp.action, "APPROVE");
        assert_eq!(resp.transaction_id, "txn-1");
    }

    #[test]
    fn idempotent_replay_returns_original_without_redeciding() {
        let decider = Arc::new(ApproveAllDecider::default());
        let engine = DecisionEngine::new(decider.clone());
        let first = engine.decide_idempotent(&req("k1")).unwrap();
        let second = engine.decide_idempotent(&req("k1")).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            decider.calls(),
            1,
            "idempotent replay must not re-run the decider"
        );
    }

    #[test]
    fn distinct_keys_each_decide() {
        let decider = Arc::new(ApproveAllDecider::default());
        let engine = DecisionEngine::new(decider.clone());
        engine.decide_idempotent(&req("k1")).unwrap();
        engine.decide_idempotent(&req("k2")).unwrap();
        assert_eq!(decider.calls(), 2);
    }

    #[test]
    fn empty_idempotency_key_is_rejected() {
        let engine = DecisionEngine::new(Arc::new(ApproveAllDecider::default()));
        let err = engine.decide_idempotent(&req("")).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn grpc_trait_delegates_to_idempotent_path() {
        let decider = Arc::new(ApproveAllDecider::default());
        let engine = DecisionEngine::new(decider.clone());
        let resp = DecisionService::decide(&engine, Request::new(req("k1")))
            .await
            .unwrap();
        assert_eq!(resp.get_ref().action, "APPROVE");
        // A second call through the gRPC entry point with the same key must not re-decide.
        DecisionService::decide(&engine, Request::new(req("k1")))
            .await
            .unwrap();
        assert_eq!(decider.calls(), 1);
    }
}
