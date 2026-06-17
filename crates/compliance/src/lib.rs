//! `compliance` — audit log, sanctions screening, AML monitoring, case lifecycle, SAR/STR.
//!
//! T015 delivers the immutable, replayable audit log (D-008, D-016). Sanctions screening (T050),
//! AML monitoring (T051), the case lifecycle (T052), and SAR/STR (T053) land in sprint S5.
#![forbid(unsafe_code)]

pub mod audit;

pub use audit::{
    AuditError, AuditRecord, AuditSink, InMemoryAuditSink, PostgresAuditSink, ReproducedDecision,
};
