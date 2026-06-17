-- Immutable, append-only audit log (T015; D-008 persistence, D-016 replay).
-- Immutability is ENFORCED below by a BEFORE UPDATE/DELETE trigger (blocks mutation regardless of
-- role). Defence-in-depth: the deploying operator SHOULD additionally grant the app role only
-- INSERT/SELECT on audit_log. A comment alone is not a control — the trigger is.
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

-- Append-only enforcement: reject any UPDATE or DELETE on audit_log at the database level.
CREATE OR REPLACE FUNCTION audit_log_block_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append-only: % is not permitted', TG_OP;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS audit_log_immutable ON audit_log;
CREATE TRIGGER audit_log_immutable
    BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_block_mutation();
