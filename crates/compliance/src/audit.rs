//! Immutable, replayable audit log (D-008, D-016; `arch:audit-completeness`,
//! `arch:hot-path-read-only`).
//!
//! Every decision produces exactly one [`AuditRecord`] carrying the inputs, the fired-rule snapshot,
//! the verdict, and the rule/model versions — enough to reproduce the decision deterministically.
//! Records are append-only; the writer runs on the async path, never on the hot path. An
//! [`InMemoryAuditSink`] backs tests; [`PostgresAuditSink`] is the durable store.

use std::fmt;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One immutable audit record. Plain fields only, so it serialises losslessly and depends on no
/// engine internals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// The transaction this decision concerned.
    pub transaction_id: String,
    /// Decision time (epoch milliseconds).
    pub decided_at_unix_ms: i64,
    /// Amount in integer minor units.
    pub amount_minor_units: i64,
    /// ISO-4217 currency.
    pub currency: String,
    /// Vertical (CARD / P2P / CRYPTO).
    pub vertical: String,
    /// The rule ids that fired — the signal snapshot at decision time.
    pub fired_rules: Vec<String>,
    /// The selected action.
    pub action: String,
    /// The risk band.
    pub band: String,
    /// The risk score.
    pub score: f64,
    /// The reason codes attached to the decision.
    pub reason_codes: Vec<String>,
    /// The rule-config version that produced the decision.
    pub rule_version: String,
    /// The model version that produced the decision.
    pub model_version: String,
}

/// The verdict reproduced from an (immutable) record — identical to the decision as emitted.
#[derive(Debug, Clone, PartialEq)]
pub struct ReproducedDecision {
    /// The action.
    pub action: String,
    /// The risk band.
    pub band: String,
    /// The risk score.
    pub score: f64,
    /// The reason codes.
    pub reason_codes: Vec<String>,
}

impl AuditRecord {
    /// Reproduce the verdict from the stored record. Because the record is immutable and complete,
    /// this is exactly the decision as originally emitted (deterministic replay).
    #[must_use]
    pub fn reproduce(&self) -> ReproducedDecision {
        ReproducedDecision {
            action: self.action.clone(),
            band: self.band.clone(),
            score: self.score,
            reason_codes: self.reason_codes.clone(),
        }
    }
}

/// Error writing an audit record.
#[derive(Debug)]
pub enum AuditError {
    /// Database error.
    Db(postgres::Error),
    /// Serialising the record payload failed.
    Serialize(serde_json::Error),
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditError::Db(e) => write!(f, "audit db error: {e}"),
            AuditError::Serialize(e) => write!(f, "audit serialize error: {e}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<postgres::Error> for AuditError {
    fn from(e: postgres::Error) -> Self {
        AuditError::Db(e)
    }
}

impl From<serde_json::Error> for AuditError {
    fn from(e: serde_json::Error) -> Self {
        AuditError::Serialize(e)
    }
}

/// A destination for audit records. The write runs on the async path, so a blocking sink is fine.
pub trait AuditSink: Send + Sync {
    /// Append one record.
    ///
    /// # Errors
    /// Implementation-specific (DB / serialisation failures).
    fn write(&self, record: &AuditRecord) -> Result<(), AuditError>;
}

/// Collects records in memory — for tests and replay tooling.
#[derive(Debug, Default)]
pub struct InMemoryAuditSink {
    records: Mutex<Vec<AuditRecord>>,
}

impl InMemoryAuditSink {
    /// Snapshot of everything written so far.
    #[must_use]
    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().expect("audit sink poisoned").clone()
    }
}

impl AuditSink for InMemoryAuditSink {
    fn write(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.records
            .lock()
            .expect("audit sink poisoned")
            .push(record.clone());
        Ok(())
    }
}

/// The durable, append-only Postgres audit store.
pub struct PostgresAuditSink {
    client: Mutex<postgres::Client>,
}

impl PostgresAuditSink {
    /// Connect to Postgres (no TLS — local/simulation).
    ///
    /// # Errors
    /// [`AuditError::Db`] if the connection fails.
    pub fn connect(conn_str: &str) -> Result<Self, AuditError> {
        let client = postgres::Client::connect(conn_str, postgres::NoTls)?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }

    /// Create the append-only `audit_log` table if it does not exist.
    ///
    /// # Errors
    /// [`AuditError::Db`] on failure.
    pub fn migrate(&self) -> Result<(), AuditError> {
        self.client
            .lock()
            .expect("audit sink poisoned")
            .batch_execute(include_str!("../../../migrations/0001_audit_log.sql"))?;
        Ok(())
    }
}

impl AuditSink for PostgresAuditSink {
    fn write(&self, record: &AuditRecord) -> Result<(), AuditError> {
        let payload = serde_json::to_string(record)?;
        self.client.lock().expect("audit sink poisoned").execute(
            "INSERT INTO audit_log \
             (transaction_id, decided_at_unix_ms, action, score, rule_version, model_version, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)",
            &[
                &record.transaction_id,
                &record.decided_at_unix_ms,
                &record.action,
                &record.score,
                &record.rule_version,
                &record.model_version,
                &payload,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AuditRecord {
        AuditRecord {
            transaction_id: "txn-1".to_string(),
            decided_at_unix_ms: 1_780_000_000_000,
            amount_minor_units: 4_999,
            currency: "USD".to_string(),
            vertical: "CARD".to_string(),
            fired_rules: vec!["blocklist.bin".to_string()],
            action: "DECLINE".to_string(),
            band: "VERY_HIGH".to_string(),
            score: 0.99,
            reason_codes: vec!["BLOCKLIST_BIN".to_string()],
            rule_version: "rules-2026-06-17".to_string(),
            model_version: "none".to_string(),
        }
    }

    #[test]
    fn audit_replay_reproduces_the_decision() {
        let original = sample();
        // The record is what gets persisted; reading it back must reproduce the verdict exactly.
        let json = serde_json::to_string(&original).unwrap();
        let restored: AuditRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored, original,
            "the stored record must round-trip losslessly"
        );
        assert_eq!(restored.reproduce(), original.reproduce());
        assert_eq!(restored.reproduce().action, "DECLINE");
        assert_eq!(
            restored.reproduce().reason_codes,
            vec!["BLOCKLIST_BIN".to_string()]
        );
    }

    #[test]
    fn in_memory_sink_writes_exactly_one_record_per_decision() {
        let sink = InMemoryAuditSink::default();
        sink.write(&sample()).unwrap();
        assert_eq!(sink.records().len(), 1);
        assert_eq!(sink.records()[0], sample());
    }

    #[test]
    #[ignore = "requires a running Postgres (docker compose up postgres); run with --ignored"]
    fn postgres_sink_persists_a_record() {
        let conn = std::env::var("NODESEC_PG").unwrap_or_else(|_| {
            "host=localhost port=55432 user=nodesec password=nodesec dbname=nodesec".to_string()
        });
        let sink = PostgresAuditSink::connect(&conn).expect("connect");
        sink.migrate().expect("migrate");
        sink.write(&sample()).expect("write");
    }
}
