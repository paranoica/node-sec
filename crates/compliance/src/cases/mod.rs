//! Case lifecycle, review queue, and four-eyes (T052; D-010, `arch:maker-checker`).
//!
//! An alert enters a state machine — `alert → triage → investigate → {close | escalate | file-sar}` —
//! and sits in a risk-prioritised review queue. Filing a SAR enforces **four-eyes**: the checker who
//! approves the filing must differ from the maker who investigated/drafted it.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fmt;

/// The state of a case in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseState {
    /// Freshly created from an alert.
    Alert,
    /// Deduplicated, enriched, prioritised.
    Triage,
    /// Assigned to an analyst, evidence being gathered.
    Investigate,
    /// Dispositioned: closed (no suspicion).
    Closed,
    /// Dispositioned: escalated to senior review.
    Escalated,
    /// Dispositioned: a SAR was filed.
    SarFiled,
}

/// An error transitioning a case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseError {
    /// The requested transition is not valid from the current state.
    InvalidTransition,
    /// A SAR filing was attempted with no maker recorded.
    NoMaker,
    /// Four-eyes violation: the checker is the same analyst as the maker.
    MakerEqualsChecker,
}

impl fmt::Display for CaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaseError::InvalidTransition => f.write_str("invalid case transition"),
            CaseError::NoMaker => f.write_str("no maker recorded for the case"),
            CaseError::MakerEqualsChecker => {
                f.write_str("four-eyes: checker must differ from maker")
            }
        }
    }
}

impl std::error::Error for CaseError {}

/// A compliance case following the alert through to disposition.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Stable case id.
    pub id: String,
    /// The subject entity.
    pub subject: String,
    /// Risk score driving review-queue priority.
    pub risk: f64,
    /// Current lifecycle state.
    pub state: CaseState,
    /// The analyst who investigated/drafted (the maker).
    pub maker: Option<String>,
    /// The analyst who approved the SAR filing (the checker), once filed.
    pub checker: Option<String>,
}

impl Case {
    /// Open a case from an alert.
    #[must_use]
    pub fn new(id: impl Into<String>, subject: impl Into<String>, risk: f64) -> Self {
        Self {
            id: id.into(),
            subject: subject.into(),
            risk,
            state: CaseState::Alert,
            maker: None,
            checker: None,
        }
    }

    /// `alert → triage`.
    ///
    /// # Errors
    /// [`CaseError::InvalidTransition`] unless the case is in `Alert`.
    pub fn triage(&mut self) -> Result<(), CaseError> {
        self.expect(CaseState::Alert)?;
        self.state = CaseState::Triage;
        Ok(())
    }

    /// `triage → investigate`, recording the maker.
    ///
    /// # Errors
    /// [`CaseError::InvalidTransition`] unless the case is in `Triage`.
    pub fn investigate(&mut self, analyst: impl Into<String>) -> Result<(), CaseError> {
        self.expect(CaseState::Triage)?;
        self.maker = Some(analyst.into());
        self.state = CaseState::Investigate;
        Ok(())
    }

    /// `investigate → closed` (no suspicion).
    ///
    /// # Errors
    /// [`CaseError::InvalidTransition`] unless the case is in `Investigate`.
    pub fn close(&mut self) -> Result<(), CaseError> {
        self.expect(CaseState::Investigate)?;
        self.state = CaseState::Closed;
        Ok(())
    }

    /// `investigate → escalated`.
    ///
    /// # Errors
    /// [`CaseError::InvalidTransition`] unless the case is in `Investigate`.
    pub fn escalate(&mut self) -> Result<(), CaseError> {
        self.expect(CaseState::Investigate)?;
        self.state = CaseState::Escalated;
        Ok(())
    }

    /// `investigate → sar-filed`, enforcing four-eyes (checker ≠ maker).
    ///
    /// # Errors
    /// [`CaseError::InvalidTransition`] if not investigating, [`CaseError::NoMaker`] if no maker,
    /// [`CaseError::MakerEqualsChecker`] if the checker is the maker.
    pub fn file_sar(&mut self, checker: impl Into<String>) -> Result<(), CaseError> {
        self.expect(CaseState::Investigate)?;
        let checker = checker.into();
        match &self.maker {
            None => Err(CaseError::NoMaker),
            Some(maker) if *maker == checker => Err(CaseError::MakerEqualsChecker),
            Some(_) => {
                self.checker = Some(checker);
                self.state = CaseState::SarFiled;
                Ok(())
            }
        }
    }

    fn expect(&self, state: CaseState) -> Result<(), CaseError> {
        if self.state == state {
            Ok(())
        } else {
            Err(CaseError::InvalidTransition)
        }
    }
}

/// A risk-prioritised review queue: the highest-risk case is served first.
#[derive(Debug, Default)]
pub struct ReviewQueue {
    heap: BinaryHeap<Prioritised>,
}

impl ReviewQueue {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a case.
    pub fn enqueue(&mut self, case: Case) {
        self.heap.push(Prioritised(case));
    }

    /// Take the highest-risk case, if any.
    pub fn pop(&mut self) -> Option<Case> {
        self.heap.pop().map(|p| p.0)
    }

    /// Number of queued cases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

/// Wrapper giving `Case` a risk-descending order for the max-heap.
#[derive(Debug)]
struct Prioritised(Case);

impl PartialEq for Prioritised {
    fn eq(&self, other: &Self) -> bool {
        self.0.risk == other.0.risk
    }
}
impl Eq for Prioritised {}
impl PartialOrd for Prioritised {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Prioritised {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .risk
            .partial_cmp(&other.0.risk)
            .unwrap_or(Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_lifecycle_happy_path() {
        let mut case = Case::new("c1", "acct-9", 0.7);
        assert_eq!(case.state, CaseState::Alert);
        case.triage().unwrap();
        case.investigate("alice").unwrap();
        assert_eq!(case.state, CaseState::Investigate);
        case.close().unwrap();
        assert_eq!(case.state, CaseState::Closed);
    }

    #[test]
    fn cases_invalid_transition_is_rejected() {
        let mut case = Case::new("c1", "acct-9", 0.5);
        // Cannot close straight from Alert.
        assert_eq!(case.close(), Err(CaseError::InvalidTransition));
    }

    #[test]
    fn cases_four_eyes_blocks_self_filing() {
        let mut case = Case::new("c1", "acct-9", 0.9);
        case.triage().unwrap();
        case.investigate("alice").unwrap();
        // Alice cannot file her own SAR.
        assert_eq!(case.file_sar("alice"), Err(CaseError::MakerEqualsChecker));
        // A different checker can.
        case.file_sar("bob").unwrap();
        assert_eq!(case.state, CaseState::SarFiled);
        assert_eq!(case.checker.as_deref(), Some("bob"));
    }

    #[test]
    fn cases_review_queue_serves_highest_risk_first() {
        let mut queue = ReviewQueue::new();
        queue.enqueue(Case::new("low", "a", 0.2));
        queue.enqueue(Case::new("high", "b", 0.9));
        queue.enqueue(Case::new("mid", "c", 0.5));
        assert_eq!(queue.pop().unwrap().id, "high");
        assert_eq!(queue.pop().unwrap().id, "mid");
        assert_eq!(queue.pop().unwrap().id, "low");
        assert!(queue.is_empty());
    }
}
