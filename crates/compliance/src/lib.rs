//! `compliance` — audit log, sanctions screening, AML monitoring, case lifecycle, SAR/STR.
//!
//! T015 delivers the immutable, replayable audit log (D-008, D-016); T050 the sanctions screening.
//! AML monitoring (T051), the case lifecycle (T052), and SAR/STR (T053) follow in sprint S5.
#![forbid(unsafe_code)]

pub mod audit;
pub mod screening;

pub use audit::{
    AuditError, AuditRecord, AuditSink, InMemoryAuditSink, PostgresAuditSink, ReproducedDecision,
};
pub use screening::{
    rescreen_on_delta, screen, ListKind, ScreeningAlert, ScreeningConfig, Subject, WatchlistEntry,
};
