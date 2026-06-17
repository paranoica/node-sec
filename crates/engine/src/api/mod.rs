//! The gRPC decision service and its idempotency cache (D-001 sync path, D-016 idempotency).
// tonic's contract is to return `Status` by value; boxing it to satisfy result_large_err would
// fight the framework on every handler, so allow the large-err result here.
#![allow(clippy::result_large_err)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use tonic::{Request, Response, Status};

use crate::decide::AsyncDecider;
use crate::pb::decision_service_server::{DecisionService, DecisionServiceServer};
use crate::pb::{DecisionRequest, DecisionResponse};

/// Maximum idempotency keys retained. Bounds memory: at ~20k tx/s this covers ~50 s of unique
/// traffic, well past any realistic client retry window. A bounded in-process cache; the durable
/// TTL store (Redis, D-006/T021) is the production replacement.
const CACHE_CAPACITY: usize = 1_000_000;

/// A bounded, FIFO-evicting idempotency cache: one key → one verdict, capped so unique-key traffic
/// cannot grow it without limit (memory-DoS). Keys older than the last `CACHE_CAPACITY` decisions
/// are evicted — far beyond the retry window that idempotency needs to cover.
struct IdempotencyCache {
    map: HashMap<String, DecisionResponse>,
    order: VecDeque<String>,
}

impl IdempotencyCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, key: &str) -> Option<&DecisionResponse> {
        self.map.get(key)
    }

    /// Insert `value` for `key` unless already present; return the verdict that is now authoritative
    /// for the key (the first writer wins, so a concurrent same-key insert is idempotent).
    fn insert_if_absent(&mut self, key: String, value: DecisionResponse) -> DecisionResponse {
        if let Some(existing) = self.map.get(&key) {
            return existing.clone();
        }
        while self.map.len() >= CACHE_CAPACITY {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value.clone());
        value
    }
}

/// The decision engine: an [`AsyncDecider`] (the model-backed hot path) behind a bounded idempotency
/// cache so a retried request returns the original verdict and never re-decides or double-updates
/// state (`arch:idempotent-decision`). The cache is capped at [`CACHE_CAPACITY`] with FIFO eviction.
pub struct DecisionEngine {
    decider: Arc<dyn AsyncDecider>,
    cache: Mutex<IdempotencyCache>,
}

impl DecisionEngine {
    /// Build an engine around an async decider (e.g. the model-backed `FeatureAwareDecider`).
    #[must_use]
    pub fn new(decider: Arc<dyn AsyncDecider>) -> Self {
        Self {
            decider,
            cache: Mutex::new(IdempotencyCache::new()),
        }
    }

    /// Decide for a request, honoring idempotency. Separated from the gRPC trait so it is callable
    /// directly in tests without a transport.
    ///
    /// # Errors
    /// Returns `InvalidArgument` if `idempotency_key` is empty — a decision request must be
    /// idempotent.
    pub async fn decide_idempotent(
        &self,
        req: &DecisionRequest,
    ) -> Result<DecisionResponse, Status> {
        if req.idempotency_key.is_empty() {
            return Err(Status::invalid_argument("idempotency_key is required"));
        }

        // Fast path: a previously decided key returns its original verdict — no re-decide, no write.
        // A poisoned lock is recovered (the data is intact; poisoning only flags a prior panic) so a
        // single panicked request can never wedge the whole decision service. The guard is dropped
        // before the await below, so no lock is held across it.
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = cache.get(&req.idempotency_key) {
                return Ok(existing.clone());
            }
        }

        let fresh = self.decider.decide(req).await;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        // First writer wins, so one key maps to exactly one decision even under a same-key race.
        Ok(cache.insert_if_absent(req.idempotency_key.clone(), fresh))
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
        let verdict = self.decide_idempotent(request.get_ref()).await?;
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
            occurred_at_unix_ms: 0,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn returns_a_decision() {
        let engine = DecisionEngine::new(Arc::new(ApproveAllDecider::default()));
        let resp = engine.decide_idempotent(&req("k1")).await.unwrap();
        assert_eq!(resp.action, "APPROVE");
        assert_eq!(resp.transaction_id, "txn-1");
    }

    #[tokio::test]
    async fn idempotent_replay_returns_original_without_redeciding() {
        let decider = Arc::new(ApproveAllDecider::default());
        let engine = DecisionEngine::new(decider.clone());
        let first = engine.decide_idempotent(&req("k1")).await.unwrap();
        let second = engine.decide_idempotent(&req("k1")).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(
            decider.calls(),
            1,
            "idempotent replay must not re-run the decider"
        );
    }

    #[tokio::test]
    async fn distinct_keys_each_decide() {
        let decider = Arc::new(ApproveAllDecider::default());
        let engine = DecisionEngine::new(decider.clone());
        engine.decide_idempotent(&req("k1")).await.unwrap();
        engine.decide_idempotent(&req("k2")).await.unwrap();
        assert_eq!(decider.calls(), 2);
    }

    #[tokio::test]
    async fn empty_idempotency_key_is_rejected() {
        let engine = DecisionEngine::new(Arc::new(ApproveAllDecider::default()));
        let err = engine.decide_idempotent(&req("")).await.unwrap_err();
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

    #[test]
    fn idempotency_cache_keeps_the_first_verdict_per_key() {
        let mut cache = IdempotencyCache::new();
        let first = DecisionResponse {
            transaction_id: "t".into(),
            action: "APPROVE".into(),
            score: 0.1,
            band: "LOW".into(),
            reason_codes: vec![],
            rule_version: "v".into(),
            model_version: "m".into(),
        };
        let second = DecisionResponse {
            action: "DECLINE".into(),
            ..first.clone()
        };
        assert_eq!(cache.insert_if_absent("k".into(), first.clone()), first);
        // A later insert for the same key returns the first verdict and ignores the new one.
        assert_eq!(cache.insert_if_absent("k".into(), second), first);
        assert_eq!(cache.get("k").unwrap().action, "APPROVE");
    }

    #[tokio::test]
    async fn engine_drives_the_model_backed_decider() {
        use crate::decision::CostMatrix;
        use crate::FeatureAwareDecider;
        use features::OnlineFeatures;
        use rules::{RulesConfig, RulesEngine};
        use std::time::Duration;
        use stream::InMemoryFeatureStore;

        struct HighScorer;
        impl model::Scorer for HighScorer {
            fn score(&self, _features: &[f32]) -> f32 {
                0.97
            }
        }

        let online = OnlineFeatures::new(
            Arc::new(InMemoryFeatureStore::default()),
            Duration::from_millis(50),
        );
        let registry = Arc::new(model::ModelRegistry::new("champ", Box::new(HighScorer)));
        let decider = FeatureAwareDecider::new(
            Arc::new(RulesEngine::from_config(RulesConfig::default())),
            online,
        )
        .with_model(registry, CostMatrix::default());
        let engine = DecisionEngine::new(Arc::new(decider));

        // The model-backed path runs through the gRPC engine: a high score declines, and the
        // model version is stamped on the verdict.
        let resp = engine.decide_idempotent(&req("k1")).await.unwrap();
        assert_eq!(resp.action, "DECLINE");
        assert_eq!(resp.model_version, "champ");
        // Idempotent replay returns the identical verdict.
        let again = engine.decide_idempotent(&req("k1")).await.unwrap();
        assert_eq!(resp, again);
    }
}
