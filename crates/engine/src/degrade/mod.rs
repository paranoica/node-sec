//! Fail-safe degradation (D-003; `term:fail-safe-degradation`, `arch:decision-within-budget`).
//!
//! When the online feature store exceeds its per-call timeout, the hot path degrades to the
//! rules-only decision and still returns within the latency SLA — a degraded answer beats a timeout.
//! (Once the model is wired in T031/T032 it consumes the features when present; until then the
//! rules-only decision is produced either way, but the feature read still exercises this path.)

use std::sync::Arc;

use features::OnlineFeatures;
use rules::RulesEngine;
use stream::FeatureStore;

use crate::decide::Decider;
use crate::decision::RulesDecider;
use crate::pb::{DecisionRequest, DecisionResponse};

/// Reason code marking a decision that ran rules-only because the feature store degraded.
pub const DEGRADED_REASON: &str = "DEGRADED_RULES_ONLY";

/// A decider that consults the online feature store fail-safe: a store timeout or error degrades the
/// decision to rules-only instead of blocking the hot path.
pub struct FeatureAwareDecider<S> {
    rules: RulesDecider,
    online: OnlineFeatures<S>,
}

impl<S> FeatureAwareDecider<S>
where
    S: FeatureStore + Send + Sync + 'static,
{
    /// Build over a rules engine and an online feature reader (which carries the per-call timeout).
    #[must_use]
    pub fn new(rules_engine: Arc<RulesEngine>, online: OnlineFeatures<S>) -> Self {
        Self {
            rules: RulesDecider::new(rules_engine),
            online,
        }
    }

    /// Decide for a request, reading features within budget. Returns the decision and whether it
    /// degraded to rules-only. A slow/failing store yields `degraded = true` without blocking.
    pub async fn decide(&self, req: &DecisionRequest) -> (DecisionResponse, bool) {
        let degraded = match card_entity_key(req) {
            Some(entity) => self.online.read(&entity).await.is_degraded(),
            None => false,
        };
        let mut response = self.rules.decide(req);
        if degraded {
            response.reason_codes.push(DEGRADED_REASON.to_string());
        }
        (response, degraded)
    }
}

/// The online-store key for the request's card (mirrors `stream::entity_keys`).
fn card_entity_key(req: &DecisionRequest) -> Option<String> {
    if req.pan.is_empty() {
        None
    } else {
        Some(format!(
            "card:{}",
            domain::Pan::new(req.pan.clone()).redacted()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use stream::store::StoreError;
    use stream::{InMemoryFeatureStore, WindowAggregates};

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

    fn blocklisting_engine() -> Arc<RulesEngine> {
        let cfg = rules::RulesConfig {
            version: "t".to_string(),
            blocklists: rules::Blocklists {
                bins: vec!["411111".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        Arc::new(RulesEngine::from_config(cfg))
    }

    /// A store whose reads block far longer than any budget.
    #[derive(Default)]
    struct SlowStore;
    impl FeatureStore for SlowStore {
        fn put(&self, _e: &str, _a: &WindowAggregates) -> Result<(), StoreError> {
            Ok(())
        }
        fn get(&self, _e: &str) -> Result<Option<WindowAggregates>, StoreError> {
            std::thread::sleep(Duration::from_millis(500));
            Ok(None)
        }
    }

    #[tokio::test]
    async fn failsafe_degrades_to_rules_only_within_budget() {
        let online = OnlineFeatures::new(Arc::new(SlowStore), Duration::from_millis(20));
        let decider = FeatureAwareDecider::new(blocklisting_engine(), online);

        let start = Instant::now();
        let (resp, degraded) = decider.decide(&req("4111110000001234")).await;
        let elapsed = start.elapsed();

        assert!(degraded, "a slow store must force degradation");
        assert!(
            elapsed < Duration::from_millis(200),
            "must return within budget despite a 500ms store, took {elapsed:?}"
        );
        // The rules-only decision still applied (blocklisted BIN → decline), plus the degraded marker.
        assert_eq!(resp.action, "DECLINE");
        assert!(resp.reason_codes.iter().any(|c| c == DEGRADED_REASON));
        assert!(resp.reason_codes.iter().any(|c| c == "BLOCKLIST_BIN"));
    }

    #[tokio::test]
    async fn failsafe_not_triggered_by_a_healthy_store() {
        let online = OnlineFeatures::new(
            Arc::new(InMemoryFeatureStore::default()),
            Duration::from_millis(50),
        );
        let decider = FeatureAwareDecider::new(blocklisting_engine(), online);
        let (resp, degraded) = decider.decide(&req("4222220000001234")).await;
        assert!(!degraded);
        assert_eq!(resp.action, "APPROVE");
        assert!(!resp.reason_codes.iter().any(|c| c == DEGRADED_REASON));
    }
}
