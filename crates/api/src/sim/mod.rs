//! Simulation control API (T066; D-022, `term:simulation-harness`).
//!
//! An operator selects a fraud **typology scenario**; the API injects it into the real rules
//! decision path and **streams** live decision and metric updates over Server-Sent Events. The
//! scenario shaping drives the genuine engine (`engine::RulesDecider`), so the streamed verdicts are
//! real rule outcomes, not canned data. The visual layer is built separately from
//! `design/sim-brief.md` via design-creator.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::Query;
use axum::response::sse::{Event, Sse};
use axum::routing::get;
use axum::{Json, Router};
use engine::pb::DecisionRequest;
use engine::{Decider, RulesDecider};
use rules::{Blocklists, RulesConfig, RulesEngine};
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

/// How often a running-metrics snapshot is interleaved into the decision stream.
const METRICS_EVERY: usize = 10;
/// The blocklisted BIN the `blocklisted_bin` scenario uses.
const BLOCKED_BIN: &str = "400099";

/// A selectable fraud typology scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Typology {
    /// Baseline legitimate traffic.
    Legitimate,
    /// Card-testing: bursts of low-value auths from one device.
    CardTesting,
    /// Amount anomaly: a card's spend spikes far above its norm.
    HighAmount,
    /// A hard-blocklisted issuer BIN.
    BlocklistedBin,
}

impl Typology {
    /// All selectable typologies.
    #[must_use]
    pub fn all() -> [Typology; 4] {
        [
            Typology::Legitimate,
            Typology::CardTesting,
            Typology::HighAmount,
            Typology::BlocklistedBin,
        ]
    }

    /// The stable scenario id used in the API.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Typology::Legitimate => "legitimate",
            Typology::CardTesting => "card_testing",
            Typology::HighAmount => "high_amount",
            Typology::BlocklistedBin => "blocklisted_bin",
        }
    }

    /// Parse a typology from its id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Typology> {
        Typology::all().into_iter().find(|t| t.id() == id)
    }
}

/// A streamed decision update.
#[derive(Debug, Clone, Serialize)]
pub struct SimDecision {
    /// Transaction id.
    pub transaction_id: String,
    /// The action taken.
    pub action: String,
    /// The score.
    pub score: f64,
    /// Reason codes attached.
    pub reason_codes: Vec<String>,
}

/// A streamed running-metrics snapshot.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SimMetrics {
    /// Decisions processed so far.
    pub processed: u64,
    /// Approved count.
    pub approved: u64,
    /// Declined count.
    pub declined: u64,
    /// Decisions carrying at least one reason code (alerts).
    pub alerts: u64,
    /// Declined / processed.
    pub decline_rate: f64,
}

impl SimMetrics {
    fn record(&mut self, action: &str, alerted: bool) {
        self.processed += 1;
        match action {
            "APPROVE" => self.approved += 1,
            "DECLINE" => self.declined += 1,
            _ => {}
        }
        if alerted {
            self.alerts += 1;
        }
        self.decline_rate = self.declined as f64 / self.processed as f64;
    }
}

/// A streamed simulation event: a decision or a metrics snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimEvent {
    /// A decision update.
    Decision(SimDecision),
    /// A running-metrics snapshot.
    Metrics(SimMetrics),
}

fn sim_decider() -> RulesDecider {
    let config = RulesConfig {
        version: "sim".to_string(),
        blocklists: Blocklists {
            bins: vec![BLOCKED_BIN.to_string()],
            ..Default::default()
        },
        ..Default::default()
    };
    RulesDecider::new(Arc::new(RulesEngine::from_config(config)))
}

fn request(
    i: usize,
    bin: &str,
    pan_suffix: usize,
    amount_minor: i64,
    device: &str,
) -> DecisionRequest {
    DecisionRequest {
        idempotency_key: format!("sim-{i}"),
        transaction_id: format!("sim-txn-{i}"),
        amount_minor_units: amount_minor,
        currency: "USD".to_string(),
        vertical: "CARD".to_string(),
        channel: "CARD_NOT_PRESENT".to_string(),
        pan: format!("{bin}{pan_suffix:010}"),
        merchant: format!("mrc-{}", i % 50),
        device: device.to_string(),
        occurred_at_unix_ms: 1_780_000_000_000 + (i as i64) * 1_000,
        ..Default::default()
    }
}

/// Shape the i-th request for a typology scenario.
fn shape_request(typology: Typology, i: usize) -> DecisionRequest {
    match typology {
        // Varied entities, normal amounts.
        Typology::Legitimate => request(
            i,
            &format!("4{:05}", 10_000 + i % 50),
            i % 100_000,
            1_000 + (i as i64 % 40_000),
            &format!("dev-{}", i % 200),
        ),
        // One device, many distinct low-value auths on one BIN.
        Typology::CardTesting => request(i, "400123", i, 100, "dev-attacker"),
        // One card primed with a normal mean, then spending spikes 100x above it.
        Typology::HighAmount => {
            let amount = if i < 5 { 5_000 } else { 900_000 };
            request(i, "400500", 7, amount, "dev-victim")
        }
        // A hard-blocklisted issuer BIN.
        Typology::BlocklistedBin => {
            request(i, BLOCKED_BIN, i % 100_000, 4_999, &format!("dev-{i}"))
        }
    }
}

/// Run a typology scenario and produce the stream of decision + metrics events.
#[must_use]
pub fn run_scenario(typology: Typology, count: usize) -> Vec<SimEvent> {
    let decider = sim_decider();
    let mut metrics = SimMetrics::default();
    let mut events = Vec::new();

    for i in 0..count {
        let response = decider.decide(&shape_request(typology, i));
        metrics.record(&response.action, !response.reason_codes.is_empty());
        events.push(SimEvent::Decision(SimDecision {
            transaction_id: response.transaction_id,
            action: response.action,
            score: response.score,
            reason_codes: response.reason_codes,
        }));
        if (i + 1) % METRICS_EVERY == 0 {
            events.push(SimEvent::Metrics(metrics.clone()));
        }
    }
    events.push(SimEvent::Metrics(metrics));
    events
}

/// A scenario descriptor returned by `GET /sim/scenarios`.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioInfo {
    /// Stable id.
    pub id: String,
    /// Typology label.
    pub typology: Typology,
}

#[derive(Debug, Deserialize)]
struct StreamParams {
    typology: Option<String>,
    count: Option<usize>,
}

async fn scenarios() -> Json<Vec<ScenarioInfo>> {
    Json(
        Typology::all()
            .into_iter()
            .map(|t| ScenarioInfo {
                id: t.id().to_string(),
                typology: t,
            })
            .collect(),
    )
}

async fn stream(
    Query(params): Query<StreamParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let typology = params
        .typology
        .as_deref()
        .and_then(Typology::from_id)
        .unwrap_or(Typology::Legitimate);
    let count = params.count.unwrap_or(50).min(1000);

    let events = run_scenario(typology, count).into_iter().map(|event| {
        Ok(Event::default()
            .json_data(&event)
            .unwrap_or_else(|_| Event::default().data("serialize-error")))
    });
    Sse::new(tokio_stream::iter(events))
}

/// The simulation control router: scenario listing + the live SSE stream.
pub fn router() -> Router {
    Router::new()
        .route("/sim/scenarios", get(scenarios))
        .route("/sim/stream", get(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics_of(events: &[SimEvent]) -> SimMetrics {
        events
            .iter()
            .rev()
            .find_map(|e| match e {
                SimEvent::Metrics(m) => Some(m.clone()),
                SimEvent::Decision(_) => None,
            })
            .expect("a final metrics snapshot")
    }

    #[test]
    fn sim_blocklisted_bin_declines_all() {
        let metrics = metrics_of(&run_scenario(Typology::BlocklistedBin, 20));
        assert_eq!(metrics.processed, 20);
        assert_eq!(metrics.declined, 20);
        assert!((metrics.decline_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sim_legitimate_mostly_approves() {
        let metrics = metrics_of(&run_scenario(Typology::Legitimate, 50));
        assert!(metrics.approved > 0);
        assert!(
            metrics.decline_rate < 0.1,
            "decline_rate {}",
            metrics.decline_rate
        );
    }

    #[test]
    fn sim_card_testing_raises_alerts() {
        // A burst of low-value auths on one device must trip the velocity rules.
        let metrics = metrics_of(&run_scenario(Typology::CardTesting, 30));
        assert!(metrics.alerts > 0, "card-testing produced no alerts");
    }

    #[test]
    fn sim_high_amount_raises_alerts() {
        // After priming the card's mean, the spikes must trip the amount-anomaly rule.
        let metrics = metrics_of(&run_scenario(Typology::HighAmount, 20));
        assert!(metrics.alerts > 0, "high-amount produced no alerts");
    }

    #[test]
    fn sim_emits_metric_snapshots() {
        let events = run_scenario(Typology::Legitimate, 25);
        let snapshots = events
            .iter()
            .filter(|e| matches!(e, SimEvent::Metrics(_)))
            .count();
        // One every 10 decisions plus a final snapshot.
        assert!(
            snapshots >= 3,
            "expected interleaved snapshots, got {snapshots}"
        );
    }

    #[test]
    fn sim_scenarios_list_is_complete() {
        assert_eq!(Typology::all().len(), 4);
        assert_eq!(
            Typology::from_id("card_testing"),
            Some(Typology::CardTesting)
        );
        assert_eq!(Typology::from_id("nope"), None);
    }
}
