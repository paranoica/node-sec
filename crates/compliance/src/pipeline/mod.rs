//! The compliance pipeline — the async-path orchestrator that turns the compliance library
//! functions into one live flow (D-010, `term:compliance-layer`).
//!
//! For each dispositioned transaction it: (1) writes the immutable audit record (every decision →
//! exactly one record), (2) screens the subject against the watchlist, (3) runs AML monitoring over
//! the account's window, and (4) opens a risk-prioritised case on any alert. It runs off the hot
//! path, so a blocking audit sink is fine.

use std::cmp::Ordering as CmpOrdering;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::aml::{evaluate as evaluate_aml, AmlAlert, AmlConfig, AmlTransaction};
use crate::audit::{AuditError, AuditRecord, AuditSink};
use crate::cases::Case;
use crate::screening::{screen, ScreeningAlert, ScreeningConfig, Subject, WatchlistEntry};

/// A case in the review queue together with the alert summaries that opened it — the unit the
/// analyst dashboard renders.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenCase {
    /// The case.
    pub case: Case,
    /// Short alert summaries (e.g. `sanctions:Vladimir Petrov`, `aml:structuring`).
    pub alerts: Vec<String>,
}

/// One transaction's worth of work for the compliance pipeline.
#[derive(Debug, Clone)]
pub struct ComplianceInput {
    /// The decision to audit (written verbatim, immutably).
    pub audit_record: AuditRecord,
    /// The subject to screen (None → screening skipped, e.g. a non-named-entity transfer).
    pub subject: Option<Subject>,
    /// The account's monitored transaction window for AML.
    pub aml_window: Vec<AmlTransaction>,
    /// Subject identifier used for any opened case.
    pub subject_id: String,
    /// Risk score driving case priority in the review queue.
    pub risk: f64,
}

/// What the pipeline produced for one input.
#[derive(Debug, Clone, PartialEq)]
pub struct ComplianceOutcome {
    /// Sanctions / PEP / adverse-media alerts.
    pub screening_alerts: Vec<ScreeningAlert>,
    /// AML typology alerts.
    pub aml_alerts: Vec<AmlAlert>,
    /// The case opened (if any alert fired), already enqueued for review.
    pub case: Option<Case>,
}

impl ComplianceOutcome {
    /// Whether any alert fired.
    #[must_use]
    pub fn has_alerts(&self) -> bool {
        !self.screening_alerts.is_empty() || !self.aml_alerts.is_empty()
    }
}

/// The compliance pipeline over an audit sink. Holds the watchlist, configs, and the review queue
/// (risk-prioritised open cases the analyst dashboard reads).
pub struct CompliancePipeline<A: AuditSink> {
    audit: A,
    watchlist: Vec<WatchlistEntry>,
    screening: ScreeningConfig,
    aml: AmlConfig,
    cases: Mutex<Vec<OpenCase>>,
    next_case: AtomicU64,
}

impl<A: AuditSink> CompliancePipeline<A> {
    /// Build a pipeline over an audit sink and the watchlist, with default configs.
    #[must_use]
    pub fn new(audit: A, watchlist: Vec<WatchlistEntry>) -> Self {
        Self {
            audit,
            watchlist,
            screening: ScreeningConfig::default(),
            aml: AmlConfig::default(),
            cases: Mutex::new(Vec::new()),
            next_case: AtomicU64::new(1),
        }
    }

    /// Process one input: audit, screen, AML-monitor, and open a case on any alert.
    ///
    /// # Errors
    /// Propagates an [`AuditError`] if the audit write fails — auditing is the first, non-skippable
    /// step (a decision that cannot be recorded must not be silently dropped).
    pub fn process(&self, input: ComplianceInput) -> Result<ComplianceOutcome, AuditError> {
        // 1. Audit first — every decision produces exactly one immutable record.
        self.audit.write(&input.audit_record)?;

        // 2. Sanctions / PEP / adverse-media screening.
        let screening_alerts = input
            .subject
            .as_ref()
            .map(|s| screen(s, &self.watchlist, &self.screening))
            .unwrap_or_default();

        // 3. AML transaction monitoring over the account's window.
        let aml_alerts = evaluate_aml(&input.aml_window, &self.aml);

        // 4. Open a risk-prioritised case on any alert.
        let case = if !screening_alerts.is_empty() || !aml_alerts.is_empty() {
            let id = self.next_case.fetch_add(1, Ordering::Relaxed);
            let case = Case::new(format!("case-{id}"), input.subject_id, input.risk);
            let mut alerts: Vec<String> = screening_alerts
                .iter()
                .map(|a| format!("sanctions:{}", a.matched_name))
                .collect();
            alerts.extend(aml_alerts.iter().map(|a| format!("aml:{}", a.typology)));

            let mut cases = self.cases.lock().unwrap_or_else(|e| e.into_inner());
            cases.push(OpenCase {
                case: case.clone(),
                alerts,
            });
            // Keep the queue risk-prioritised (highest first) so the dashboard and reviewer agree.
            cases.sort_by(|a, b| {
                b.case
                    .risk
                    .partial_cmp(&a.case.risk)
                    .unwrap_or(CmpOrdering::Equal)
            });
            Some(case)
        } else {
            None
        };

        Ok(ComplianceOutcome {
            screening_alerts,
            aml_alerts,
            case,
        })
    }

    /// Take the highest-risk pending case off the review queue, if any.
    pub fn next_case_for_review(&self) -> Option<Case> {
        let mut cases = self.cases.lock().unwrap_or_else(|e| e.into_inner());
        if cases.is_empty() {
            None
        } else {
            Some(cases.remove(0).case) // sorted highest-risk first
        }
    }

    /// A non-draining, risk-ordered snapshot of the open cases (what the analyst dashboard reads).
    #[must_use]
    pub fn open_cases(&self) -> Vec<OpenCase> {
        self.cases.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Number of cases waiting in the review queue.
    #[must_use]
    pub fn pending_cases(&self) -> usize {
        self.cases.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Borrow the audit sink (e.g. to read back records in tests/replay).
    pub fn audit_sink(&self) -> &A {
        &self.audit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aml::Direction;
    use crate::audit::InMemoryAuditSink;
    use crate::screening::ListKind;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};

    fn audit_record(txn_id: &str, action: &str) -> AuditRecord {
        AuditRecord {
            transaction_id: txn_id.to_string(),
            decided_at_unix_ms: 1_780_000_000_000,
            amount_minor_units: 4_999,
            currency: "USD".to_string(),
            vertical: "CARD".to_string(),
            fired_rules: vec![],
            action: action.to_string(),
            band: "LOW".to_string(),
            score: 0.1,
            reason_codes: vec![],
            rule_version: "v".to_string(),
            model_version: "none".to_string(),
        }
    }

    fn sdn(name: &str) -> WatchlistEntry {
        WatchlistEntry {
            name: name.to_string(),
            dob: None,
            nationality: None,
            national_id: None,
            list: ListKind::OfacSdn,
        }
    }

    fn input(
        txn_id: &str,
        subject: Option<Subject>,
        aml_window: Vec<AmlTransaction>,
    ) -> ComplianceInput {
        ComplianceInput {
            audit_record: audit_record(txn_id, "APPROVE"),
            subject,
            aml_window,
            subject_id: "acct-9".to_string(),
            risk: 0.7,
        }
    }

    fn cash_in(amount: i64, at: OffsetDateTime) -> AmlTransaction {
        AmlTransaction {
            amount_minor: amount,
            direction: Direction::In,
            counterparty: "src".to_string(),
            geography: "US".to_string(),
            at,
        }
    }

    #[test]
    fn pipeline_audits_every_decision() {
        let pipeline = CompliancePipeline::new(InMemoryAuditSink::default(), vec![]);
        pipeline.process(input("t1", None, vec![])).unwrap();
        pipeline.process(input("t2", None, vec![])).unwrap();
        assert_eq!(pipeline.audit_sink().records().len(), 2);
    }

    #[test]
    fn pipeline_opens_case_on_a_sanctions_hit() {
        let pipeline =
            CompliancePipeline::new(InMemoryAuditSink::default(), vec![sdn("Vladimir Petrov")]);
        let subject = Subject {
            name: "Vladimir Petrov".to_string(),
            dob: None,
            nationality: None,
            national_id: None,
        };
        let outcome = pipeline
            .process(input("t1", Some(subject), vec![]))
            .unwrap();
        assert!(!outcome.screening_alerts.is_empty());
        assert!(outcome.case.is_some());
        assert_eq!(pipeline.pending_cases(), 1);
    }

    #[test]
    fn pipeline_opens_case_on_an_aml_alert() {
        let t = datetime!(2026-06-17 00:00 UTC);
        // Three sub-CTR deposits → structuring.
        let window = vec![
            cash_in(950_000, t),
            cash_in(950_000, t + Duration::hours(1)),
            cash_in(950_000, t + Duration::hours(2)),
        ];
        let pipeline = CompliancePipeline::new(InMemoryAuditSink::default(), vec![]);
        let outcome = pipeline.process(input("t1", None, window)).unwrap();
        assert!(outcome
            .aml_alerts
            .iter()
            .any(|a| a.typology == "structuring"));
        assert!(outcome.case.is_some());
    }

    #[test]
    fn pipeline_clean_decision_opens_no_case() {
        let pipeline = CompliancePipeline::new(InMemoryAuditSink::default(), vec![]);
        let outcome = pipeline.process(input("t1", None, vec![])).unwrap();
        assert!(!outcome.has_alerts());
        assert!(outcome.case.is_none());
        assert_eq!(pipeline.pending_cases(), 0);
    }

    #[test]
    fn pipeline_review_queue_serves_highest_risk_first() {
        let pipeline =
            CompliancePipeline::new(InMemoryAuditSink::default(), vec![sdn("Bad Actor")]);
        let subject = |name: &str| Subject {
            name: name.to_string(),
            dob: None,
            nationality: None,
            national_id: None,
        };
        let mut low = input("low", Some(subject("Bad Actor")), vec![]);
        low.risk = 0.2;
        let mut high = input("high", Some(subject("Bad Actor")), vec![]);
        high.risk = 0.9;
        high.subject_id = "acct-high".to_string();
        pipeline.process(low).unwrap();
        pipeline.process(high).unwrap();
        assert_eq!(
            pipeline.next_case_for_review().unwrap().subject,
            "acct-high"
        );
    }
}
