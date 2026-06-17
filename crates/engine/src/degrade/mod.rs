//! Model-backed hot-path decision with fail-safe degradation (D-003/D-004/D-005;
//! `term:fail-safe-degradation`, `arch:decision-within-budget`).
//!
//! The full decision: read the online features within budget → assemble the model vector → score
//! with the champion (challenger shadow-scores in parallel, logged, never used) → fuse the rule
//! signals and the model score by **expected value** over the cost matrix. **Fail-safe:** the model
//! needs features, so if the store times out / errors the path degrades to the rules-only decision
//! and still returns within the SLA — a degraded answer beats a timeout, and it never fails *open*
//! (a hard rule decline still declines). With no model deployed it is rules-only by construction.

use std::sync::Arc;

use features::{model_vector, OnlineFeatures, ReadResult};
use model::{reason_codes, ModelRegistry};
use rules::RulesEngine;
use stream::FeatureStore;

use crate::decision::{action_str, band_str, fuse, fuse_ev, request_to_transaction, CostMatrix};
use crate::pb::{DecisionRequest, DecisionResponse};

/// Reason code marking a decision that ran rules-only because the feature store degraded.
pub const DEGRADED_REASON: &str = "DEGRADED_RULES_ONLY";

/// How many model reason codes to attach.
const MODEL_REASON_TOP_K: usize = 3;

/// The hot-path decider: rules + online features + (optional) champion/challenger model, fused by
/// expected value, with fail-safe degradation to rules-only on a feature-store fault.
pub struct FeatureAwareDecider<S> {
    rules: Arc<RulesEngine>,
    online: OnlineFeatures<S>,
    model: Option<Arc<ModelRegistry>>,
    costs: CostMatrix,
}

impl<S> FeatureAwareDecider<S>
where
    S: FeatureStore + Send + Sync + 'static,
{
    /// Build over a rules engine and an online feature reader (which carries the per-call timeout).
    /// Rules-only until a model is attached with [`with_model`](Self::with_model).
    #[must_use]
    pub fn new(rules_engine: Arc<RulesEngine>, online: OnlineFeatures<S>) -> Self {
        Self {
            rules: rules_engine,
            online,
            model: None,
            costs: CostMatrix::default(),
        }
    }

    /// Attach the champion/challenger registry and the expected-value cost matrix so decisions are
    /// model-driven (and fall back to rules-only when features degrade).
    #[must_use]
    pub fn with_model(mut self, model: Arc<ModelRegistry>, costs: CostMatrix) -> Self {
        self.model = Some(model);
        self.costs = costs;
        self
    }

    /// Decide for a request within budget. Returns the decision and whether it degraded to
    /// rules-only. A slow/failing store yields `degraded = true` without blocking the hot path.
    pub async fn decide(&self, req: &DecisionRequest) -> (DecisionResponse, bool) {
        let txn = request_to_transaction(req);
        let eval = self.rules.evaluate(&txn);

        let read = match card_entity_key(req) {
            Some(entity) => self.online.read(&entity).await,
            None => ReadResult::Miss, // no entity to look up — not a fault
        };
        let degraded = read.is_degraded();

        // Model path: features usable (a hit, or a cold miss → empty aggregates) and a model is live.
        if !degraded {
            if let Some(registry) = self.model.as_ref() {
                let aggregates = read.hit().unwrap_or_default();
                let vector = model_vector(&txn, &aggregates);
                let scored = registry.score(&vector); // champion + challenger shadow (logged)
                let (band, action, score) =
                    fuse_ev(&eval, f64::from(scored.champion_score), &self.costs);

                let mut codes: Vec<String> = eval
                    .hits
                    .iter()
                    .map(|h| h.reason_code.as_str().to_string())
                    .collect();
                codes.extend(
                    reason_codes(&vector, MODEL_REASON_TOP_K)
                        .iter()
                        .map(|c| c.as_str().to_string()),
                );

                let response = DecisionResponse {
                    transaction_id: req.transaction_id.clone(),
                    action: action_str(action).to_string(),
                    score,
                    band: band_str(band).to_string(),
                    reason_codes: codes,
                    rule_version: self.rules.version(),
                    model_version: scored.champion_version,
                };
                return (response, false);
            }
        }

        // Rules-only path (no model deployed, or features degraded) — fail-safe, never fail-open.
        let (band, action, score) = fuse(&eval);
        let mut codes: Vec<String> = eval
            .hits
            .iter()
            .map(|h| h.reason_code.as_str().to_string())
            .collect();
        if degraded {
            codes.push(DEGRADED_REASON.to_string());
        }
        let response = DecisionResponse {
            transaction_id: req.transaction_id.clone(),
            action: action_str(action).to_string(),
            score,
            band: band_str(band).to_string(),
            reason_codes: codes,
            rule_version: self.rules.version(),
            model_version: "none".to_string(),
        };
        (response, degraded)
    }
}

#[tonic::async_trait]
impl<S> crate::decide::AsyncDecider for FeatureAwareDecider<S>
where
    S: FeatureStore + Send + Sync + 'static,
{
    async fn decide(&self, req: &DecisionRequest) -> DecisionResponse {
        // The gRPC engine only needs the verdict; the degraded flag is for metrics/audit on the
        // direct path. Call the inherent method explicitly to avoid shadowing by this trait method.
        FeatureAwareDecider::decide(self, req).await.0
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

    /// A deterministic stand-in for the ONNX model in unit tests.
    struct ConstScorer(f32);
    impl model::Scorer for ConstScorer {
        fn score(&self, _features: &[f32]) -> f32 {
            self.0
        }
    }

    fn healthy_online() -> OnlineFeatures<InMemoryFeatureStore> {
        OnlineFeatures::new(
            Arc::new(InMemoryFeatureStore::default()),
            Duration::from_millis(50),
        )
    }

    #[tokio::test]
    async fn model_path_uses_champion_score_and_version() {
        // A high champion score with no rule hits must drive the decision to DECLINE via fuse_ev.
        let registry = Arc::new(model::ModelRegistry::new(
            "champion-test",
            Box::new(ConstScorer(0.95)),
        ));
        let decider = FeatureAwareDecider::new(blocklisting_engine(), healthy_online())
            .with_model(registry, crate::decision::CostMatrix::default());

        let (resp, degraded) = decider.decide(&req("4222220000001234")).await;
        assert!(!degraded);
        assert_eq!(resp.action, "DECLINE");
        assert_eq!(resp.model_version, "champion-test");
        assert!((resp.score - 0.95).abs() < 1e-6);
    }

    #[tokio::test]
    async fn model_champion_drives_decision_not_challenger() {
        // Champion says low-risk → APPROVE; a high-scoring challenger only shadows, never decides.
        let registry = Arc::new(
            model::ModelRegistry::new("champ", Box::new(ConstScorer(0.01)))
                .with_challenger("chall", Box::new(ConstScorer(0.99))),
        );
        let decider = FeatureAwareDecider::new(blocklisting_engine(), healthy_online())
            .with_model(registry, crate::decision::CostMatrix::default());

        let (resp, _) = decider.decide(&req("4222220000001234")).await;
        assert_eq!(resp.action, "APPROVE");
        assert_eq!(resp.model_version, "champ");
    }

    #[tokio::test]
    async fn model_hard_rule_override_beats_a_low_model_score() {
        // Blocklisted BIN + a near-zero model score: the hard rule decline still wins.
        let registry = Arc::new(model::ModelRegistry::new(
            "champ",
            Box::new(ConstScorer(0.0)),
        ));
        let decider = FeatureAwareDecider::new(blocklisting_engine(), healthy_online())
            .with_model(registry, crate::decision::CostMatrix::default());

        let (resp, _) = decider.decide(&req("4111110000001234")).await;
        assert_eq!(resp.action, "DECLINE");
        assert!(resp.reason_codes.iter().any(|c| c == "BLOCKLIST_BIN"));
    }

    #[tokio::test]
    async fn model_degraded_store_falls_back_to_rules_only() {
        // A model is deployed, but a faulted store must degrade to rules-only — the model is never
        // consulted (model_version stays "none") and the answer carries the degraded marker.
        let online = OnlineFeatures::new(Arc::new(SlowStore), Duration::from_millis(20));
        let registry = Arc::new(model::ModelRegistry::new(
            "champ",
            Box::new(ConstScorer(0.95)),
        ));
        let decider = FeatureAwareDecider::new(blocklisting_engine(), online)
            .with_model(registry, crate::decision::CostMatrix::default());

        // Clean PAN so, rules-only, it approves — proving the high model score was NOT consulted.
        let (resp, degraded) = decider.decide(&req("4222220000001234")).await;
        assert!(degraded);
        assert_eq!(resp.model_version, "none");
        assert_eq!(resp.action, "APPROVE");
        assert!(resp.reason_codes.iter().any(|c| c == DEGRADED_REASON));
    }
}
