//! SAR/STR and CTR generation (T053; D-010, `term:sar`, `term:ctr`).
//!
//! When a case is dispositioned to file, [`generate_sar`] emits a structured Suspicious Activity
//! Report carrying a **filing deadline** and a scheduled **continuing-activity** follow-up review.
//! Two regulatory invariants are baked in:
//!
//! * **Tipping-off** — a SAR is confidential; the subject is never notified. The report's
//!   [`disclose_to_subject`](SuspiciousActivityReport::disclose_to_subject) is unconditionally
//!   `false`, and no subject-facing notice is ever produced.
//! * **CTR independence** — a Currency Transaction Report ([`maybe_ctr`]) is produced purely from
//!   cash movement crossing the threshold, regardless of whether any suspicion was found.
//!
//! Deadlines and thresholds are data (`config/sar/`), not code (D-014).

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

/// Deadlines and thresholds for SAR/CTR generation (data, not code).
#[derive(Debug, Clone, Deserialize)]
pub struct SarConfig {
    /// Days from detection within which the SAR must be filed.
    pub filing_window_days: i64,
    /// Days after filing at which continuing activity is reviewed for a follow-up SAR.
    pub continuing_activity_days: i64,
    /// Cash-movement threshold that triggers a CTR (minor units).
    pub ctr_threshold_minor: i64,
}

impl Default for SarConfig {
    fn default() -> Self {
        Self {
            filing_window_days: 30,         // FinCEN-style 30-day filing window
            continuing_activity_days: 90,   // follow-up review on continuing activity
            ctr_threshold_minor: 1_000_000, // $10,000
        }
    }
}

/// The facts a SAR is generated from (drawn from the dispositioned case).
#[derive(Debug, Clone)]
pub struct SarInput {
    /// Originating case id.
    pub case_id: String,
    /// The subject entity.
    pub subject: String,
    /// Hypothesised typologies (e.g. `structuring`, `funnel`).
    pub typologies: Vec<String>,
    /// Aggregate suspicious amount (minor units).
    pub total_amount_minor: i64,
    /// When the suspicious activity was detected.
    pub detected_at: OffsetDateTime,
}

/// A structured Suspicious Activity Report.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SuspiciousActivityReport {
    /// Originating case id.
    pub case_id: String,
    /// The subject entity.
    pub subject: String,
    /// Hypothesised typologies.
    pub typologies: Vec<String>,
    /// Aggregate suspicious amount (minor units).
    pub total_amount_minor: i64,
    /// Structured narrative (subject, activity, typologies, amount, dates).
    pub narrative: String,
    /// When the activity was detected.
    #[serde(with = "time::serde::rfc3339")]
    pub detected_at: OffsetDateTime,
    /// The regulatory deadline to file.
    #[serde(with = "time::serde::rfc3339")]
    pub filing_deadline: OffsetDateTime,
    /// When continuing activity is reviewed for a follow-up filing.
    #[serde(with = "time::serde::rfc3339")]
    pub continuing_activity_review: OffsetDateTime,
    /// Tipping-off guard: a SAR is confidential and is never disclosed to the subject.
    pub disclose_to_subject: bool,
}

/// Generate a SAR from a dispositioned case, with deadline and continuing-activity follow-up.
#[must_use]
pub fn generate_sar(input: &SarInput, config: &SarConfig) -> SuspiciousActivityReport {
    let filing_deadline = input.detected_at + Duration::days(config.filing_window_days);
    let continuing_activity_review =
        filing_deadline + Duration::days(config.continuing_activity_days);
    let narrative = format!(
        "Subject {subject} exhibited suspicious activity (typologies: {typologies}) \
         totalling {amount} minor units, detected at {detected}. Confidential — \
         not for disclosure to the subject.",
        subject = input.subject,
        typologies = input.typologies.join(", "),
        amount = input.total_amount_minor,
        detected = input.detected_at,
    );
    SuspiciousActivityReport {
        case_id: input.case_id.clone(),
        subject: input.subject.clone(),
        typologies: input.typologies.clone(),
        total_amount_minor: input.total_amount_minor,
        narrative,
        detected_at: input.detected_at,
        filing_deadline,
        continuing_activity_review,
        disclose_to_subject: false,
    }
}

/// A Currency Transaction Report, produced from cash movement alone.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CurrencyTransactionReport {
    /// The subject entity.
    pub subject: String,
    /// The reportable cash amount (minor units).
    pub cash_amount_minor: i64,
    /// When the cash moved.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

/// Produce a CTR when cash movement reaches the threshold — independently of any suspicion finding.
#[must_use]
pub fn maybe_ctr(
    subject: &str,
    cash_amount_minor: i64,
    at: OffsetDateTime,
    config: &SarConfig,
) -> Option<CurrencyTransactionReport> {
    (cash_amount_minor >= config.ctr_threshold_minor).then(|| CurrencyTransactionReport {
        subject: subject.to_string(),
        cash_amount_minor,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn input() -> SarInput {
        SarInput {
            case_id: "case-1".to_string(),
            subject: "acct-9".to_string(),
            typologies: vec!["structuring".to_string(), "funnel".to_string()],
            total_amount_minor: 4_750_000,
            detected_at: datetime!(2026-06-17 00:00 UTC),
        }
    }

    #[test]
    fn sar_has_filing_deadline_and_continuing_followup() {
        let report = generate_sar(&input(), &SarConfig::default());
        assert_eq!(report.filing_deadline, datetime!(2026-07-17 00:00 UTC));
        // Continuing-activity review is scheduled after the filing deadline.
        assert!(report.continuing_activity_review > report.filing_deadline);
        assert_eq!(
            report.continuing_activity_review,
            datetime!(2026-10-15 00:00 UTC)
        );
    }

    #[test]
    fn sar_never_discloses_to_subject_tipping_off() {
        let report = generate_sar(&input(), &SarConfig::default());
        assert!(!report.disclose_to_subject);
    }

    #[test]
    fn sar_narrative_includes_typologies_and_amount() {
        let report = generate_sar(&input(), &SarConfig::default());
        assert!(report.narrative.contains("structuring"));
        assert!(report.narrative.contains("funnel"));
        assert!(report.narrative.contains("4750000"));
    }

    #[test]
    fn ctr_produced_above_threshold_without_suspicion() {
        let at = datetime!(2026-06-17 00:00 UTC);
        let cfg = SarConfig::default();
        // At/above the threshold a CTR is produced regardless of any suspicion finding.
        assert!(maybe_ctr("acct-9", 1_000_000, at, &cfg).is_some());
        assert!(maybe_ctr("acct-9", 2_500_000, at, &cfg).is_some());
        // Below the threshold, none.
        assert!(maybe_ctr("acct-9", 999_999, at, &cfg).is_none());
    }
}
