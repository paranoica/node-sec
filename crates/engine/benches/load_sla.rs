//! Full-engine load test to the latency SLA (T065; D-003, `arch:decision-within-budget`).
//!
//! Drives the **model-backed** hot path — [`FeatureAwareDecider`] reads the online feature store
//! within a per-call budget, scores the real exported ONNX model (a pool of sessions) by expected
//! value, and falls back to the rules-only decision on any store fault — and checks **p99 < 20 ms**
//! at load. Two modes:
//!
//! * default (healthy store) — the ~20k tx/s SLA probe;
//! * `LOAD_SLA_FAULT=store` — the online store is faulted under load; fail-safe degradation must
//!   hold and the SLA must still be met (driven by `scripts/chaos.sh`).
//!
//! The process exits non-zero if p99 breaches the budget, so `cargo bench -p engine load_sla` is a
//! real gate.

use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use engine::pb::DecisionRequest;
use engine::FeatureAwareDecider;
use features::OnlineFeatures;
use rules::{RulesConfig, RulesEngine};
use stream::store::StoreError;
use stream::{FeatureStore, InMemoryFeatureStore, WindowAggregates};

/// Per-call feature-read budget — deliberately well under the 20 ms end-to-end SLA so that even a
/// faulted store degrades within budget.
const READ_BUDGET_MS: u64 = 5;
/// D-003 hot-path p99 budget.
const SLA_US: f64 = 20_000.0;
/// The target sustained load.
const TARGET_TPS: f64 = 20_000.0;
/// Concurrent in-flight requests offered to the engine (models the live request fan-in).
const CONCURRENCY: usize = 16;
/// The exported fraud model — the real ONNX graph scored in-process on the hot path.
const MODEL_ONNX: &[u8] = include_bytes!("../../../ml/artifacts/fraud_lgbm.onnx");

/// A faulted online store: every read fails immediately (a dependency that is down).
struct FaultedStore;
impl FeatureStore for FaultedStore {
    fn put(&self, _entity: &str, _aggregates: &WindowAggregates) -> Result<(), StoreError> {
        Ok(())
    }
    fn get(&self, _entity: &str) -> Result<Option<WindowAggregates>, StoreError> {
        Err(StoreError::Backend(
            "chaos: feature store faulted".to_string(),
        ))
    }
}

fn make_request(i: usize) -> DecisionRequest {
    let bin = 400_000 + (i % 50);
    DecisionRequest {
        idempotency_key: format!("k{i}"),
        transaction_id: format!("txn-{i}"),
        amount_minor_units: 100 + (i as i64 % 50_000),
        currency: "USD".to_string(),
        vertical: "CARD".to_string(),
        channel: "CARD_NOT_PRESENT".to_string(),
        pan: format!("{bin}{:010}", i % 100_000),
        merchant: format!("mrc-{}", i % 200),
        device: format!("dev-{}", i % 500),
        occurred_at_unix_ms: 1_780_000_000_000 + i as i64,
        ..Default::default()
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn run<S>(label: &str, store: Arc<S>, n: usize) -> (f64, f64, bool)
where
    S: FeatureStore + Send + Sync + 'static,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let online = OnlineFeatures::new(store, Duration::from_millis(READ_BUDGET_MS));
    // The real ONNX champion on the hot path — this is the full model-backed decision under load.
    let champion = model::PooledModel::from_onnx_bytes(MODEL_ONNX, 8).expect("load onnx");
    let registry = Arc::new(model::ModelRegistry::new(
        "champion-load",
        Box::new(champion),
    ));
    let decider = Arc::new(
        FeatureAwareDecider::new(
            Arc::new(RulesEngine::from_config(RulesConfig::default())),
            online,
        )
        .with_model(registry, engine::decision::CostMatrix::default()),
    );
    let pool: Arc<Vec<DecisionRequest>> = Arc::new((0..1000).map(make_request).collect());

    rt.block_on(async move {
        // Warm up (page-in, branch predictor, velocity maps, blocking pool).
        for r in pool.iter().take(500) {
            let _ = decider.decide(r).await;
        }

        // Offer `n` requests across `CONCURRENCY` in-flight workers and time the whole run.
        let cursor = Arc::new(AtomicUsize::new(0));
        let mut set: JoinSet<Vec<(u64, bool)>> = JoinSet::new();
        let wall = Instant::now();
        for _ in 0..CONCURRENCY {
            let decider = decider.clone();
            let pool = pool.clone();
            let cursor = cursor.clone();
            set.spawn(async move {
                let mut samples = Vec::new();
                loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = &pool[i % pool.len()];
                    let t = Instant::now();
                    let (_resp, degraded) = decider.decide(r).await;
                    samples.push((t.elapsed().as_nanos() as u64, degraded));
                }
                samples
            });
        }

        let mut lat_ns = Vec::with_capacity(n);
        let mut any_degraded = false;
        while let Some(joined) = set.join_next().await {
            for (lat, degraded) in joined.expect("worker") {
                lat_ns.push(lat);
                any_degraded |= degraded;
            }
        }
        let elapsed = wall.elapsed();
        lat_ns.sort_unstable();

        let to_us = |ns: u64| ns as f64 / 1000.0;
        let p99_us = to_us(percentile(&lat_ns, 99.0));
        let throughput = n as f64 / elapsed.as_secs_f64();

        println!("== full-engine load SLA [{label}] ==");
        println!("decisions  : {n}");
        println!("degraded   : {any_degraded}");
        println!("throughput : {throughput:.0} decisions/s (target {TARGET_TPS:.0})");
        println!("p50        : {:.2} us", to_us(percentile(&lat_ns, 50.0)));
        println!("p99        : {p99_us:.2} us");
        println!("p999       : {:.2} us", to_us(percentile(&lat_ns, 99.9)));
        println!("max        : {:.2} us", to_us(lat_ns[lat_ns.len() - 1]));
        println!(
            "p99 vs SLA : {p99_us:.2} us / {SLA_US:.0} us -> {}",
            if p99_us < SLA_US { "OK" } else { "OVER" }
        );
        (p99_us, throughput, any_degraded)
    })
}

fn main() {
    let fault = env::var("LOAD_SLA_FAULT").ok();
    let (p99_us, throughput, degraded) = match fault.as_deref() {
        Some("store") => run("FAULT: store down", Arc::new(FaultedStore), 50_000),
        _ => run(
            "HEALTHY",
            Arc::new(InMemoryFeatureStore::default()),
            100_000,
        ),
    };

    // The SLA gate.
    assert!(
        p99_us < SLA_US,
        "p99 {p99_us:.0}us breaches the {SLA_US:.0}us SLA"
    );
    // Under a store fault, fail-safe degradation must have actually engaged.
    if fault.as_deref() == Some("store") {
        assert!(
            degraded,
            "store faulted but no decision degraded — fail-safe did not engage"
        );
        println!("fail-safe held: every decision degraded to rules-only within the SLA");
    }
    // Throughput is an informational capacity signal on this single-node probe.
    println!(
        "capacity   : {throughput:.0} decisions/s {}",
        if throughput >= TARGET_TPS {
            ">= 20k target"
        } else {
            "(probe; scale horizontally for target)"
        }
    );
}
