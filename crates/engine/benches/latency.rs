//! Rules-only hot-path latency harness (T016; D-003 latency SLA, `arch:decision-within-budget`).
//!
//! Measures p50/p99/p999 latency and sustained throughput of the rules-only decision path and
//! compares p99 to the 20 ms hot-path budget. The full model-backed engine load test at ~20k tx/s
//! is T065; this establishes the rules-only baseline.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use engine::pb::DecisionRequest;
use engine::{Decider, RulesDecider};
use rules::{RulesConfig, RulesEngine};

fn make_request(i: usize) -> DecisionRequest {
    // Spread across BINs/devices so velocity state is realistic but bounded.
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
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn main() {
    let decider = RulesDecider::new(Arc::new(RulesEngine::from_config(RulesConfig::default())));
    let pool: Vec<DecisionRequest> = (0..1000).map(make_request).collect();

    // Warm up (page-in, branch predictor, velocity maps).
    for r in pool.iter().cycle().take(5_000) {
        black_box(decider.decide(r));
    }

    let n = 200_000usize;
    let mut lat_ns = Vec::with_capacity(n);
    let wall = Instant::now();
    for i in 0..n {
        let r = &pool[i % pool.len()];
        let t = Instant::now();
        black_box(decider.decide(r));
        lat_ns.push(t.elapsed().as_nanos() as u64);
    }
    let elapsed = wall.elapsed();
    lat_ns.sort_unstable();

    let to_us = |ns: u64| ns as f64 / 1000.0;
    let mean_us = lat_ns.iter().sum::<u64>() as f64 / lat_ns.len() as f64 / 1000.0;
    let throughput = n as f64 / elapsed.as_secs_f64();
    let p99_us = to_us(percentile(&lat_ns, 99.0));
    let budget_us = 20_000.0; // D-003 hot-path p99 budget (20 ms)

    println!("== rules-only decision-path latency (T016) ==");
    println!("decisions  : {n}");
    println!("throughput : {throughput:.0} decisions/s");
    println!("mean       : {mean_us:.2} us");
    println!("min        : {:.2} us", to_us(lat_ns[0]));
    println!("p50        : {:.2} us", to_us(percentile(&lat_ns, 50.0)));
    println!("p99        : {p99_us:.2} us");
    println!("p999       : {:.2} us", to_us(percentile(&lat_ns, 99.9)));
    println!("max        : {:.2} us", to_us(lat_ns[lat_ns.len() - 1]));
    println!(
        "p99 vs SLA : {p99_us:.2} us / {budget_us:.0} us budget -> {}",
        if p99_us < budget_us { "OK" } else { "OVER" }
    );
}
