//! `compliance` — audit log, sanctions screening, AML monitoring, case lifecycle, SAR/STR.
//!
//! T015 delivers the immutable, replayable audit log (D-008, D-016); T050 the sanctions screening.
//! AML monitoring (T051), the case lifecycle (T052), and SAR/STR (T053) follow in sprint S5.
#![forbid(unsafe_code)]

pub mod aml;
pub mod audit;
pub mod cases;
pub mod feedback;
pub mod pipeline;
pub mod sar;
pub mod screening;

pub use pipeline::{ComplianceInput, ComplianceOutcome, CompliancePipeline, OpenCase};

pub use aml::{evaluate as evaluate_aml, AmlAlert, AmlConfig, AmlTransaction, Direction};
pub use audit::{
    AuditError, AuditRecord, AuditSink, InMemoryAuditSink, PostgresAuditSink, ReproducedDecision,
};
pub use cases::{Case, CaseError, CaseState, ReviewQueue};
pub use feedback::{
    label_from_disposition, InMemoryLabelStore, InvestigatorLabel, LabelStore, Outcome,
};
pub use sar::{
    generate_sar, maybe_ctr, CurrencyTransactionReport, SarConfig, SarInput,
    SuspiciousActivityReport,
};
pub use screening::{
    rescreen_on_delta, screen, ListKind, ScreeningAlert, ScreeningConfig, Subject, WatchlistEntry,
};
