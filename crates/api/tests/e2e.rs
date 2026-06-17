//! End-to-end pipeline test: a generated transaction flows through the engine decision and the
//! compliance pipeline into the audit log — the connected system, not isolated units.
//!
//! The in-memory variant always runs (a real gate). The `#[ignore]`d variant drives the same chain
//! into a **live Postgres** audit sink (`scripts/integration-test.sh` / `--ignored` + `NODESEC_PG`).

use std::sync::Arc;

use compliance::{
    AmlTransaction, AuditRecord, ComplianceInput, CompliancePipeline, Direction, InMemoryAuditSink,
    ListKind, Subject, WatchlistEntry,
};
use engine::pb::{DecisionRequest, DecisionResponse};
use engine::{Decider, RulesDecider};
use rules::{Blocklists, RulesConfig, RulesEngine};
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

fn request(bin: &str, amount_minor: i64) -> DecisionRequest {
    DecisionRequest {
        idempotency_key: format!("e2e-{bin}-{amount_minor}"),
        transaction_id: format!("txn-{bin}-{amount_minor}"),
        amount_minor_units: amount_minor,
        currency: "USD".to_string(),
        vertical: "CARD".to_string(),
        channel: "CARD_NOT_PRESENT".to_string(),
        pan: format!("{bin}0000000001"),
        merchant: "mrc-1".to_string(),
        device: "dev-1".to_string(),
        occurred_at_unix_ms: 1_780_000_000_000,
        ..Default::default()
    }
}

/// Map an emitted decision back to its immutable audit record (the async path's first step).
fn to_audit_record(req: &DecisionRequest, resp: &DecisionResponse) -> AuditRecord {
    AuditRecord {
        transaction_id: resp.transaction_id.clone(),
        decided_at_unix_ms: req.occurred_at_unix_ms,
        amount_minor_units: req.amount_minor_units,
        currency: req.currency.clone(),
        vertical: req.vertical.clone(),
        fired_rules: resp.reason_codes.clone(),
        action: resp.action.clone(),
        band: resp.band.clone(),
        score: resp.score,
        reason_codes: resp.reason_codes.clone(),
        rule_version: resp.rule_version.clone(),
        model_version: resp.model_version.clone(),
    }
}

fn subject(name: &str) -> Subject {
    Subject {
        name: name.to_string(),
        dob: None,
        nationality: None,
        national_id: None,
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

fn cash_in(amount: i64, at: OffsetDateTime) -> AmlTransaction {
    AmlTransaction {
        amount_minor: amount,
        direction: Direction::In,
        counterparty: "src".to_string(),
        geography: "US".to_string(),
        at,
    }
}

fn blocklisting_decider() -> RulesDecider {
    let cfg = RulesConfig {
        version: "e2e".to_string(),
        blocklists: Blocklists {
            bins: vec!["400099".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };
    RulesDecider::new(Arc::new(RulesEngine::from_config(cfg)))
}

fn structuring_window() -> Vec<AmlTransaction> {
    let t = datetime!(2026-06-17 00:00 UTC);
    vec![
        cash_in(950_000, t),
        cash_in(950_000, t + Duration::hours(1)),
        cash_in(950_000, t + Duration::hours(2)),
    ]
}

#[test]
fn e2e_decision_to_compliance_to_audit() {
    let decider = blocklisting_decider();
    let pipeline =
        CompliancePipeline::new(InMemoryAuditSink::default(), vec![sdn("Vladimir Petrov")]);

    // 1. A clean card payment → approve, audited, no alert.
    let r = request("400100", 5_000);
    let resp = decider.decide(&r);
    assert_eq!(resp.action, "APPROVE");
    let out = pipeline
        .process(ComplianceInput {
            audit_record: to_audit_record(&r, &resp),
            subject: None,
            aml_window: vec![],
            subject_id: "acct-clean".to_string(),
            risk: resp.score,
        })
        .unwrap();
    assert!(!out.has_alerts());

    // 2. A blocklisted BIN → hard decline, still audited.
    let r = request("400099", 5_000);
    let resp = decider.decide(&r);
    assert_eq!(resp.action, "DECLINE");
    pipeline
        .process(ComplianceInput {
            audit_record: to_audit_record(&r, &resp),
            subject: None,
            aml_window: vec![],
            subject_id: "acct-blocked".to_string(),
            risk: resp.score,
        })
        .unwrap();

    // 3. A sanctioned subject → screening alert opens a case.
    let r = request("400100", 5_000);
    let resp = decider.decide(&r);
    let out = pipeline
        .process(ComplianceInput {
            audit_record: to_audit_record(&r, &resp),
            subject: Some(subject("Vladimir Petrov")),
            aml_window: vec![],
            subject_id: "acct-sanctioned".to_string(),
            risk: 0.95,
        })
        .unwrap();
    assert!(!out.screening_alerts.is_empty());
    assert!(out.case.is_some());

    // 4. A structuring window → AML alert opens a case.
    let r = request("400100", 950_000);
    let resp = decider.decide(&r);
    let out = pipeline
        .process(ComplianceInput {
            audit_record: to_audit_record(&r, &resp),
            subject: None,
            aml_window: structuring_window(),
            subject_id: "acct-structuring".to_string(),
            risk: 0.8,
        })
        .unwrap();
    assert!(out.aml_alerts.iter().any(|a| a.typology == "structuring"));
    assert!(out.case.is_some());

    // The whole chain held: every decision audited, two alerts opened two cases, the decline is in
    // the replayable record, and the higher-risk (sanctions, 0.95) case is served first.
    let records = pipeline.audit_sink().records();
    assert_eq!(records.len(), 4, "every decision must be audited");
    assert_eq!(records[1].action, "DECLINE");
    assert_eq!(pipeline.pending_cases(), 2);
    assert_eq!(
        pipeline.next_case_for_review().unwrap().subject,
        "acct-sanctioned"
    );
}

#[test]
#[ignore = "requires a running Postgres (scripts/integration-test.sh); run with --ignored + NODESEC_PG"]
fn e2e_persists_audit_to_live_postgres() {
    let conn = std::env::var("NODESEC_PG").unwrap_or_else(|_| {
        "host=127.0.0.1 port=55432 user=nodesec password=nodesec dbname=nodesec".to_string()
    });
    let sink = compliance::PostgresAuditSink::connect(&conn).expect("connect");
    sink.migrate().expect("migrate");
    let pipeline = CompliancePipeline::new(sink, vec![sdn("Vladimir Petrov")]);
    let decider = blocklisting_decider();

    // Drive the full chain into a real Postgres audit log: a sanctioned decision is decided, the
    // verdict is persisted immutably, and the screening alert opens a case.
    let r = request("400100", 5_000);
    let resp = decider.decide(&r);
    let out = pipeline
        .process(ComplianceInput {
            audit_record: to_audit_record(&r, &resp),
            subject: Some(subject("Vladimir Petrov")),
            aml_window: structuring_window(),
            subject_id: "acct-e2e".to_string(),
            risk: 0.95,
        })
        .expect("compliance pipeline must persist to live Postgres");

    assert!(
        out.case.is_some(),
        "a sanctioned + structuring decision must open a case"
    );
    assert!(!out.screening_alerts.is_empty());
    assert!(out.aml_alerts.iter().any(|a| a.typology == "structuring"));
}
