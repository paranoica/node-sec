# Architecture — node-sec (anti-fraud decision engine)

> Decided structure. See `decisions.md` for the WHY, `glossary.md` for terms.
> _Last updated: 2026-06-17._

## Components

- **decision-engine** (Rust, hot path) — gRPC service; receives a transaction event, looks up online features, derives request-time features, evaluates the rules engine, runs in-process ONNX inference, fuses to a risk score, selects an action by expected-value decisioning, emits the decision + audit record. Owns the latency SLA.
- **ingest** (Rust) — the event-backbone producer/consumer boundary; validates and publishes transaction events to Redpanda, keyed by entity key.
- **stream-processor** (Rust, async update path) — consumes the event backbone, maintains per-entity windowed state, computes velocity/aggregate features, and materialises them to the online feature store; writes the offline feature store.
- **feature-store** — online (Redis) + offline (Postgres/columnar); one feature definition, two materialisations, with online/offline parity.
- **rules-engine** (Rust) — evaluates hot-reloadable rule config + blocklists; emits hard overrides and soft signals with reason codes and typology tags.
- **model** (Python training → ONNX → Rust serving) — trains LightGBM with cost-sensitive learning and calibration; exports ONNX; the registry versions artifacts; Rust runs in-process inference and extracts SHAP-based reason codes.
- **graph** (Python batch + Rust serving) — entity resolution → identity graph; transaction graph; centrality/community/cycle/motif features materialised back to the online store.
- **compliance** (Rust + Postgres) — sanctions screening, AML monitoring, the case lifecycle, SAR/STR generation with four-eyes, and the audit trail.
- **simulator** (Rust) — synthetic transaction generator with fraud-pattern injection, ground-truth + delayed-label emission.
- **dashboards** (web, design-creator) — analyst case-management surface + simulation control surface.
- **observability** — Prometheus + Grafana over both paths.

## Data flows

- **Decision (sync):** transaction → decision-engine → [online feature read + request-time derivation + rules + ONNX score → policy/expected-value] → action + reason codes returned to the caller. The decision event (carrying its full audit record) is then **emitted to the event backbone** and persisted to the audit log by the async path — never an inline blocking write on the hot path (`arch:hot-path-read-only`).
- **State update (async):** transaction event → Redpanda (keyed by entity key) → stream-processor → online feature store (+ offline store); graph batch consumes history → graph features → online store.
- **Compliance (mixed):** real-time sanctions screening inline on the sync path; AML monitoring + graph alerts on the async path → review queue → case lifecycle → close / escalate / SAR.
- **Feedback:** investigator labels + chargeback labels → offline store → retraining → model registry → shadow → champion.

## Data model

> Entities are defined in `glossary.md` (`term:*`). Here: relations, cardinality, status transitions.

- **transaction** (`term:transaction`) — references one card/account, device, IP, merchant or counterparty; amount in integer minor units + currency; immutable. One transaction → exactly one decision.
- **decision** (`term:decision`) — `action ∈ {approve, decline, step-up, review, hold}`; references the transaction, the rule-config version, the model version, the feature snapshot, the risk band, and reason codes. Immutable.
- **case** (`term:case-lifecycle`) — status transitions: `alert → triage → investigate → (close | escalate | file-sar) → dispositioned`. A case aggregates ≥1 alert about one subject entity.
- **label** (`term:label`) — `source ∈ {investigator, chargeback}`, `value ∈ {fraud, legit}`, with an as-of timestamp; many labels per transaction over time (delayed labels supersede).
- **entity / graph node** (`term:identity-graph`) — account/card/device/IP/merchant/counterparty/address nodes; edges = transfers (transaction graph) or shared-identifier links (identity graph), time-stamped and weighted.
- **rule** (`term:rule`) — versioned config row: predicate parameters, severity, typology tag, reason code.

## Invariants

<!-- @anchor arch:hot-path-read-only -->
- **Hot-path read-only** — the decision engine performs no synchronous blocking write on the hot path; all state mutation flows through the event backbone to the async update path.

<!-- @anchor arch:decision-within-budget -->
- **Decision within budget** — every decision returns within the latency SLA; any dependency exceeding its per-call timeout triggers fail-safe degradation (rules-only / last-known-good features) rather than a timeout.

<!-- @anchor arch:audit-completeness -->
- **Audit completeness** — every decision produces exactly one immutable audit record containing the inputs, feature snapshot, rule and model versions, score, and reason codes — sufficient for deterministic replay.

<!-- @anchor arch:feature-parity -->
- **Feature parity** — each feature has a single definition materialised identically into the online and offline stores; a divergence is train/serve skew and is a defect, not a tuning choice.

<!-- @anchor arch:idempotent-decision -->
- **Idempotent decision** — a decision is idempotent per idempotency key: a retried request returns the original decision and causes no additional state update.

<!-- @anchor arch:partition-by-entity -->
- **Partition by entity** — events on the backbone are partitioned by entity key; ordering guarantees hold per entity key only, and per-entity windowed state is computed within a partition.

<!-- @anchor arch:maker-checker -->
- **Maker ≠ checker** — a SAR/STR cannot be approved or filed by the analyst who drafted it; the compliance layer enforces four-eyes on every filing.

<!-- @anchor arch:money-integer -->
- **Money is integer** — monetary amounts are represented as integer minor units with an explicit currency; floating-point money is forbidden anywhere in the system.

<!-- @anchor arch:versioned-decision -->
- **Versioned decision** — every decision records the exact rule-config version and model version used, so behaviour is reproducible and auditable across deploys.

<!-- @anchor arch:vertical-agnostic-core -->
- **Vertical-agnostic core** — the engine core contains no vertical-specific logic; card, P2P, and crypto behaviour lives entirely in vertical packs plugged in through the feature, rule, and action interfaces.
