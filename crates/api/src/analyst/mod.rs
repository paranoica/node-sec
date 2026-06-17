//! Analyst dashboard read-API (T055; D-022, `term:review-queue`).
//!
//! Assembles the risk-prioritised review queue into analyst-facing [`CaseView`]s — each case
//! enriched with its alerts, evidence, reason codes, and graph links — and serves them read-only
//! over `GET /queue`. The view-model assembly ([`build_queue`]) is pure and unit-tested; the HTTP
//! layer ([`router`]) is a thin axum surface over a snapshot.

use std::sync::Arc;

use axum::{extract::State, routing::get, Json, Router};
use compliance::{Case, CaseState, OpenCase};
use serde::Serialize;

/// A piece of evidence attached to a case.
#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    /// Evidence kind (e.g. `velocity`, `device`, `sanctions`).
    pub kind: String,
    /// Human-readable detail.
    pub detail: String,
}

/// A link to a connected entity in the identity/transaction graph.
#[derive(Debug, Clone, Serialize)]
pub struct GraphLink {
    /// The connected counterparty/entity.
    pub counterparty: String,
    /// The relationship (e.g. `shared-device`, `funds-to`).
    pub relation: String,
    /// Edge weight / strength.
    pub weight: f64,
}

/// The enriched, analyst-facing view of one case.
#[derive(Debug, Clone, Serialize)]
pub struct CaseView {
    /// Case id.
    pub case_id: String,
    /// Subject entity.
    pub subject: String,
    /// Risk score (drives queue order).
    pub risk: f64,
    /// Lifecycle state.
    pub state: String,
    /// Alert summaries (typologies, screening hits, rule fires).
    pub alerts: Vec<String>,
    /// Supporting evidence.
    pub evidence: Vec<Evidence>,
    /// Model reason codes.
    pub reason_codes: Vec<String>,
    /// Connected entities in the graph.
    pub graph_links: Vec<GraphLink>,
}

/// The inputs assembled into a [`CaseView`] — a case plus its enrichments.
#[derive(Debug, Clone)]
pub struct CaseBundle {
    /// The case.
    pub case: Case,
    /// Alert summaries.
    pub alerts: Vec<String>,
    /// Supporting evidence.
    pub evidence: Vec<Evidence>,
    /// Model reason codes.
    pub reason_codes: Vec<String>,
    /// Connected entities in the graph.
    pub graph_links: Vec<GraphLink>,
}

fn state_str(state: CaseState) -> &'static str {
    match state {
        CaseState::Alert => "alert",
        CaseState::Triage => "triage",
        CaseState::Investigate => "investigate",
        CaseState::Closed => "closed",
        CaseState::Escalated => "escalated",
        CaseState::SarFiled => "sar_filed",
    }
}

impl From<CaseBundle> for CaseView {
    fn from(bundle: CaseBundle) -> Self {
        CaseView {
            case_id: bundle.case.id,
            subject: bundle.case.subject,
            risk: bundle.case.risk,
            state: state_str(bundle.case.state).to_string(),
            alerts: bundle.alerts,
            evidence: bundle.evidence,
            reason_codes: bundle.reason_codes,
            graph_links: bundle.graph_links,
        }
    }
}

/// Assemble the analyst queue: enrich each case and order it by descending risk.
#[must_use]
pub fn build_queue(bundles: Vec<CaseBundle>) -> Vec<CaseView> {
    let mut views: Vec<CaseView> = bundles.into_iter().map(CaseView::from).collect();
    views.sort_by(|a, b| {
        b.risk
            .partial_cmp(&a.risk)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    views
}

/// Shared, read-only queue snapshot served by the API.
type Snapshot = Arc<Vec<CaseView>>;

/// Build the analyst read-API router over a queue snapshot.
pub fn router(snapshot: Vec<CaseView>) -> Router {
    Router::new()
        .route("/queue", get(queue))
        .with_state(Arc::new(snapshot))
}

async fn queue(State(snapshot): State<Snapshot>) -> Json<Vec<CaseView>> {
    Json((*snapshot).clone())
}

/// Map a live compliance open case (a case + the alert summaries that opened it) to the analyst
/// view. Evidence / reason-codes / graph-links are filled by enrichment lookups; from the pipeline
/// alone we surface the case and its alerts.
#[must_use]
pub fn case_view(open: &OpenCase) -> CaseView {
    CaseView {
        case_id: open.case.id.clone(),
        subject: open.case.subject.clone(),
        risk: open.case.risk,
        state: state_str(open.case.state).to_string(),
        alerts: open.alerts.clone(),
        evidence: vec![],
        reason_codes: vec![],
        graph_links: vec![],
    }
}

/// A provider that yields the current risk-ordered queue snapshot, called per request so the
/// dashboard always reflects the live compliance pipeline.
pub type CaseProvider = Arc<dyn Fn() -> Vec<CaseView> + Send + Sync>;

/// Build the analyst read-API router over a **live** case provider (e.g. the compliance pipeline's
/// open cases) instead of a static snapshot.
pub fn router_live(provider: CaseProvider) -> Router {
    Router::new()
        .route("/queue", get(queue_live))
        .with_state(provider)
}

async fn queue_live(State(provider): State<CaseProvider>) -> Json<Vec<CaseView>> {
    Json(provider())
}

/// A small demo snapshot for `curl`-ing the endpoint locally.
#[must_use]
pub fn demo_snapshot() -> Vec<CaseView> {
    let mut high = Case::new("case-1001", "acct-9", 0.94);
    let _ = high.triage();
    let _ = high.investigate("alice");
    let mut low = Case::new("case-1002", "acct-3", 0.31);
    let _ = low.triage();
    build_queue(vec![
        CaseBundle {
            case: high,
            alerts: vec!["aml:structuring".into(), "sanctions:near-match".into()],
            evidence: vec![Evidence {
                kind: "velocity".into(),
                detail: "7 sub-CTR deposits in 24h".into(),
            }],
            reason_codes: vec!["R02_VELOCITY".into(), "R07_GEO_RISK".into()],
            graph_links: vec![GraphLink {
                counterparty: "acct-44".into(),
                relation: "funds-to".into(),
                weight: 0.8,
            }],
        },
        CaseBundle {
            case: low,
            alerts: vec!["rules:amount-anomaly".into()],
            evidence: vec![],
            reason_codes: vec!["R11_AMOUNT".into()],
            graph_links: vec![],
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn bundle(id: &str, risk: f64) -> CaseBundle {
        CaseBundle {
            case: Case::new(id, "acct-9", risk),
            alerts: vec!["aml:structuring".into()],
            evidence: vec![Evidence {
                kind: "velocity".into(),
                detail: "x".into(),
            }],
            reason_codes: vec!["R02_VELOCITY".into()],
            graph_links: vec![GraphLink {
                counterparty: "acct-2".into(),
                relation: "funds-to".into(),
                weight: 0.5,
            }],
        }
    }

    #[test]
    fn analyst_queue_is_risk_prioritised() {
        let queue = build_queue(vec![
            bundle("low", 0.2),
            bundle("high", 0.9),
            bundle("mid", 0.5),
        ]);
        let order: Vec<&str> = queue.iter().map(|v| v.case_id.as_str()).collect();
        assert_eq!(order, ["high", "mid", "low"]);
    }

    #[test]
    fn analyst_view_exposes_required_fields() {
        let queue = build_queue(vec![bundle("c1", 0.7)]);
        let value: Value = serde_json::to_value(&queue[0]).unwrap();
        for key in [
            "alerts",
            "evidence",
            "reason_codes",
            "graph_links",
            "risk",
            "state",
        ] {
            assert!(value.get(key).is_some(), "missing field {key}");
        }
    }
}
