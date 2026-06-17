-- Immutable, append-only audit log (T015; D-008 persistence, D-016 replay).
-- The application role is granted INSERT/SELECT only — no UPDATE/DELETE — so records are immutable.
CREATE TABLE IF NOT EXISTS audit_log (
    id                 BIGSERIAL PRIMARY KEY,
    transaction_id     TEXT NOT NULL,
    decided_at_unix_ms BIGINT NOT NULL,
    action             TEXT NOT NULL,
    score              DOUBLE PRECISION NOT NULL,
    rule_version       TEXT NOT NULL,
    model_version      TEXT NOT NULL,
    payload            JSONB NOT NULL,
    written_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_log_txn_idx ON audit_log (transaction_id);
CREATE INDEX IF NOT EXISTS audit_log_decided_idx ON audit_log (decided_at_unix_ms);
