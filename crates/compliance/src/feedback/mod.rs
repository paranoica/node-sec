//! Investigator-label feedback loop (T054; D-011/D-012, `term:label`, `term:reject-inference`).
//!
//! When an investigator dispositions a case, [`label_from_disposition`] derives an investigator
//! label — `Closed → legit`, `Escalated`/`SarFiled → fraud` — and it is appended to the append-only
//! offline [`LabelStore`]. These labels feed the retraining dataset (joined offline by `ml/labels`,
//! whose record shape — `value`/`source`/`available_at_unix` — this mirrors).
//!
//! The complementary half — **reject inference**, debiasing the engine's own declines so training
//! is not skewed by transactions it never let through — lives in Python (`ml/feedback`).

use serde::Serialize;
use time::OffsetDateTime;

use crate::cases::{Case, CaseState};

/// The adjudicated outcome of a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Confirmed suspicious / fraudulent.
    Fraud,
    /// Cleared as legitimate.
    Legit,
}

impl Outcome {
    /// The offline-store string (matches `ml/labels/join.py`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Fraud => "fraud",
            Outcome::Legit => "legit",
        }
    }
}

/// An investigator label written to the offline store, feeding the retraining dataset.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InvestigatorLabel {
    /// Originating case id.
    pub case_id: String,
    /// The subject entity (join key into the retraining dataset).
    pub subject: String,
    /// `"fraud"` or `"legit"`.
    pub value: String,
    /// Label provenance — always `"investigator"` here.
    pub source: String,
    /// The analyst who dispositioned the case.
    pub decided_by: String,
    /// When the label became known (delayed-label censoring boundary).
    pub available_at_unix: i64,
}

/// Derive an investigator label from a dispositioned case.
///
/// Returns `None` while the case is not yet terminal (`Alert`/`Triage`/`Investigate`).
#[must_use]
pub fn label_from_disposition(case: &Case, at: OffsetDateTime) -> Option<InvestigatorLabel> {
    let value = match case.state {
        CaseState::Closed => Outcome::Legit,
        CaseState::Escalated | CaseState::SarFiled => Outcome::Fraud,
        CaseState::Alert | CaseState::Triage | CaseState::Investigate => return None,
    };
    Some(InvestigatorLabel {
        case_id: case.id.clone(),
        subject: case.subject.clone(),
        value: value.as_str().to_string(),
        source: "investigator".to_string(),
        decided_by: case.maker.clone().unwrap_or_default(),
        available_at_unix: at.unix_timestamp(),
    })
}

/// An append-only offline store of investigator labels feeding the retraining dataset.
pub trait LabelStore {
    /// Append a label.
    fn append(&mut self, label: InvestigatorLabel);
    /// All labels written so far.
    fn labels(&self) -> &[InvestigatorLabel];
}

/// An in-memory [`LabelStore`] (the simulated offline store).
#[derive(Debug, Default)]
pub struct InMemoryLabelStore {
    labels: Vec<InvestigatorLabel>,
}

impl InMemoryLabelStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl LabelStore for InMemoryLabelStore {
    fn append(&mut self, label: InvestigatorLabel) {
        self.labels.push(label);
    }
    fn labels(&self) -> &[InvestigatorLabel] {
        &self.labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn dispositioned(id: &str, to: CaseState) -> Case {
        let mut case = Case::new(id, "acct-9", 0.8);
        case.triage().unwrap();
        case.investigate("alice").unwrap();
        match to {
            CaseState::Closed => case.close().unwrap(),
            CaseState::Escalated => case.escalate().unwrap(),
            CaseState::SarFiled => case.file_sar("bob").unwrap(),
            _ => {}
        }
        case
    }

    #[test]
    fn feedback_closed_case_is_legit() {
        let case = dispositioned("c1", CaseState::Closed);
        let label = label_from_disposition(&case, datetime!(2026-06-17 00:00 UTC)).unwrap();
        assert_eq!(label.value, "legit");
        assert_eq!(label.source, "investigator");
        assert_eq!(label.decided_by, "alice");
    }

    #[test]
    fn feedback_filed_and_escalated_cases_are_fraud() {
        for state in [CaseState::SarFiled, CaseState::Escalated] {
            let case = dispositioned("c1", state);
            let label = label_from_disposition(&case, datetime!(2026-06-17 00:00 UTC)).unwrap();
            assert_eq!(label.value, "fraud");
        }
    }

    #[test]
    fn feedback_open_case_has_no_label() {
        let mut case = Case::new("c1", "acct-9", 0.5);
        case.triage().unwrap();
        case.investigate("alice").unwrap();
        assert!(label_from_disposition(&case, datetime!(2026-06-17 00:00 UTC)).is_none());
    }

    #[test]
    fn feedback_store_appends_and_feeds_dataset() {
        let mut store = InMemoryLabelStore::new();
        let case = dispositioned("c1", CaseState::SarFiled);
        let label = label_from_disposition(&case, datetime!(2026-06-17 00:00 UTC)).unwrap();
        store.append(label);
        assert_eq!(store.labels().len(), 1);
        assert_eq!(store.labels()[0].value, "fraud");
    }
}
