# Decisions — node-sec (anti-fraud decision engine)

> **Source of truth (highest wins):** this file → `architecture.md` → `glossary.md` → `open-questions.md`.
> `project-context/` is historical, NOT a source of truth.
> _Last updated: 2026-06-17._

<!-- @anchor decision:two-path-architecture refs:term:two-path-architecture,term:hot-path,term:async-update-path,term:decision-engine -->
### D-001 — Two-path (CQRS) architecture
- **Context:** a fraud verdict must return under a hard latency SLA, but maintaining velocity, graph, and entity state is write-heavy and bursty. Coupling them would let state writes blow the decision budget.
- **Decision:** split the system into a synchronous decision path (the decision engine / hot path, returns a verdict) and an async update path (a stream processor recomputing features and state from the event backbone). The decision request never blocks on a state write.
- **Consequences:** introduces the event backbone (D-007) and the online/offline feature store (D-006); the hot path becomes read-only over precomputed state; eventual consistency of features is accepted and bounded by feature freshness.

<!-- @anchor decision:polyglot-rust-python refs:term:hot-path,term:in-process-inference,term:risk-model -->
### D-002 — Polyglot: Rust hot path, Python for ML
- **Context:** the hot path needs predictable tail latency (no GC pauses) and memory safety for money logic; model training needs the Python ML ecosystem. Neither language serves both well.
- **Decision:** implement the hot path and async update path in Rust (tokio); train risk models and run batch graph analytics in Python. The two meet only at artifact boundaries (ONNX model, materialised features) — never a Python call on the hot path.
- **Consequences:** mandates ONNX as the model handoff (D-004); a Cargo workspace of focused crates; a Python training pipeline producing versioned artifacts.

<!-- @anchor decision:latency-sla refs:term:latency-sla,term:fail-safe-degradation,term:hot-path -->
### D-003 — Latency SLA and fail-safe budget
- **Context:** the engine is the inline gate on transaction authorisation; a timeout is worse than a degraded answer, and tail latency (p99/p999) is what authorisation flows actually feel.
- **Decision:** bind the hot path to p99 < 20 ms at ~20,000 tx/s sustained. Every dependency call (online feature store, model) gets a per-call timeout below budget; on breach the hot path applies fail-safe degradation (rules-only / last-known-good features) and still returns within the SLA.
- **Consequences:** drives in-process inference (D-004), aggressive local caching of features (D-006), backpressure and load-shedding to rules-only, and a load-test gate in the final phase.

<!-- @anchor decision:onnx-inference refs:term:onnx,term:in-process-inference,term:risk-model -->
### D-004 — In-process ONNX inference
- **Context:** a network hop to a model server adds an RTT and a failure mode the latency SLA cannot absorb.
- **Decision:** export trained models to ONNX and run them in-process inside the decision engine via an ONNX Runtime binding; no model-server network call on the hot path.
- **Consequences:** the model is pinned by version in the model registry (D-013) and referenced in every decision for deterministic replay (D-016); training (Python) and serving (Rust) share only the ONNX artifact and the feature contract.

<!-- @anchor decision:decisioning-fusion refs:term:decision,term:action,term:risk-score,term:risk-band,term:rules-engine,term:expected-value-decisioning,term:step-up -->
### D-005 — Rule+model fusion and expected-value action selection
- **Context:** rules give determinism, instant deployability, and hard regulatory blocks; models generalise. Neither alone is sufficient, and a fixed score cutoff ignores business cost.
- **Decision:** the policy layer fuses deterministic rule outcomes (hard overrides win; soft signals feed the score) with the calibrated risk score, maps the result to a risk band, and selects the action by expected-value decisioning over a cost matrix. Actions are approve / decline / step-up / review / hold.
- **Consequences:** requires score calibration (D-012); bands and the cost matrix are config per vertical/merchant; step-up outcomes (pass/fail/abandon) are themselves a label source.

<!-- @anchor decision:feature-store refs:term:feature-store,term:online-feature-store,term:offline-feature-store,term:online-offline-parity,term:point-in-time-correctness -->
### D-006 — Feature store with online/offline parity
- **Context:** the hot path needs sub-ms feature reads; training needs the same features over history without leakage.
- **Decision:** one feature definition materialises into an online store (Redis, read by the hot path, updated by the async path) and an offline store (training, with point-in-time-correct joins). Online/offline parity is an enforced invariant.
- **Consequences:** the async update path owns all expensive aggregation; the hot path only reads precomputed features plus cheap request-time deviation features; train/serve skew becomes a monitored failure, not a silent one.

<!-- @anchor decision:event-backbone refs:term:event-backbone,term:entity-key,term:async-update-path -->
### D-007 — Redpanda event backbone, partitioned by entity key
- **Context:** the async path must absorb write-side bursts without backpressuring the decision path, and aggregates must be computed with locality.
- **Decision:** use Redpanda (Kafka-compatible) as the durable event backbone; partition events by entity key so per-entity aggregates and graph updates are local to a partition.
- **Consequences:** ordering is per-entity-key only; the stream processor maintains per-entity windowed state; backpressure is absorbed by the log, protecting the latency SLA.

<!-- @anchor decision:persistence-postgres refs:term:audit-log,term:transaction,term:case-lifecycle,term:label -->
### D-008 — Postgres for transactions, audit, cases, labels
- **Context:** decisions, cases, labels, rule config, and graph persistence need durable, queryable, transactional storage with strong consistency.
- **Decision:** use Postgres as the system of record for transactions, the immutable audit log, cases and their lifecycle, labels (investigator + chargeback), rule configuration, and persisted graph entities.
- **Consequences:** the audit log is append-only with retention; the offline feature store derives from the transaction history; case state transitions are transactional.

<!-- @anchor decision:graph-subsystem refs:term:entity-resolution,term:identity-graph,term:transaction-graph,term:fraud-ring,term:mule-account -->
### D-009 — Graph subsystem (entity resolution + transaction graph)
- **Context:** networked fraud (rings, mules) is invisible to per-transaction scoring; P2P and crypto verticals depend on linkage.
- **Decision:** build an identity graph (entity resolution: normalise → block → match → cluster) and a time-stamped transaction graph; compute graph features (centrality, community, cycles, fan-in/fan-out, shortest-path-to-bad, temporal motifs) in batch and materialise them back into the online feature store; expose a few cheap graph signals on the hot path.
- **Consequences:** graph features become scoring inputs and mule-account / fraud-ring signals; the graph backend is decided in D-021 (in-process petgraph + Postgres).

<!-- @anchor decision:compliance-layer refs:term:compliance-layer,term:audit-log,term:sanctions-screening,term:aml-monitoring,term:sar,term:case-lifecycle,term:four-eyes,term:reason-code -->
### D-010 — Full compliance layer
- **Context:** a "bank-grade" system is judged as much on auditability and regulatory controls as on detection.
- **Decision:** implement a full compliance layer — immutable audit log of every decision; reason-code explainability; sanctions/PEP/adverse-media screening; AML transaction monitoring with typology-tagged alerts; a case lifecycle with four-eyes (maker ≠ checker); and SAR/STR generation with deadlines, continuing-activity follow-ups, and a tipping-off prohibition.
- **Consequences:** the case lifecycle and audit trail are first-class subsystems; the analyst dashboard (D-019) consumes them; CTR vs SAR are distinct outputs.

<!-- @anchor decision:explainability refs:term:reason-code,term:risk-model -->
### D-011 — SHAP-derived reason codes
- **Context:** declines and SARs must be explainable to customers, disputes, and regulators; analysts need to triage fast.
- **Decision:** every decision attaches reason codes derived from the dominant rule hits and SHAP feature contributions of the risk model.
- **Consequences:** the model must expose per-feature contributions at serve time; reason codes are a stable enumerated vocabulary, versioned alongside the model.

<!-- @anchor decision:labels-imbalance refs:term:label,term:chargeback,term:delayed-label,term:class-imbalance,term:cost-sensitive-learning,term:score-calibration,term:reject-inference,term:random-control-holdout -->
### D-012 — Labels and class-imbalance strategy
- **Context:** fraud is ~0.1–1% of traffic; authoritative chargeback labels arrive weeks later; blocked transactions never get an outcome; the engine's own decisions bias future training.
- **Decision:** train cost-sensitively with calibrated outputs; treat investigator labels (fast, noisy) and chargeback labels (slow, authoritative) as two streams; maintain a random control holdout for unbiased labels; apply reject inference to estimate outcomes for blocked transactions.
- **Consequences:** the delayed-label clock shapes retraining (D-013) and evaluation (D-014); the holdout has a measured business cost that must be bounded.

<!-- @anchor decision:model-lifecycle refs:term:model-registry,term:champion-challenger,term:shadow-scoring,term:concept-drift,term:psi -->
### D-013 — Model lifecycle (registry, champion-challenger, drift)
- **Context:** models drift as tactics change, and a new model must never be promoted blind.
- **Decision:** version every model in a registry; promote via champion-challenger with shadow scoring on live traffic; monitor concept drift via PSI on feature and score distributions; trigger retraining on drift thresholds, not a blind calendar.
- **Consequences:** decisions reference the serving model version; promotion is gated on measured superiority; drift dashboards feed the retraining trigger.

<!-- @anchor decision:metrics refs:term:pr-auc,term:recall-at-fpr,term:precision-at-n,term:alert-to-fraud-ratio -->
### D-014 — Evaluation metrics (accuracy banned)
- **Context:** under extreme imbalance, accuracy and ROC-AUC mislead; the operating point is bounded by review capacity.
- **Decision:** evaluate with PR-AUC (primary), recall at fixed FPR, precision at N (review capacity), the alert-to-fraud ratio, and the dollar trade-off of fraud caught vs false-positive friction. Accuracy is not a reported metric.
- **Consequences:** the evaluation harness and offline reports standardise on these; thresholds/bands are tuned to the cost matrix and review capacity, not to accuracy.

<!-- @anchor decision:rules-as-data refs:term:rules-engine,term:rule,term:blocklist -->
### D-015 — Rules are data (hot-reloadable)
- **Context:** analysts must respond to emerging attacks in minutes without a code deploy.
- **Decision:** rules and blocklists are versioned configuration loaded (and hot-reloaded) by the rules engine, not compiled code; each rule carries parameters, severity, a typology tag, and a reason code.
- **Consequences:** a rule-config schema and a safe reload path on the hot path; rule changes are themselves audited.

<!-- @anchor decision:idempotency-determinism refs:term:idempotency-key,term:deterministic-replay,term:audit-log -->
### D-016 — Idempotency and deterministic replay
- **Context:** clients retry; audits and disputes require reproducing a past decision exactly.
- **Decision:** decision requests carry an idempotency key (same key → same decision, no double state update); every decision logs the full feature snapshot and rule/model versions so it can be deterministically replayed.
- **Consequences:** the audit log stores enough to replay; retries under backpressure are safe; replay is a test and a debugging tool.

<!-- @anchor decision:observability refs:term:observability,term:latency-sla -->
### D-017 — Observability as a first-class concern
- **Context:** "reliability" is only credible if tail latency and decision behaviour are continuously visible.
- **Decision:** instrument the hot path and async path with Prometheus metrics (p50/p99/p999 latency, throughput, decision mix, score/feature drift) and Grafana dashboards; trace representative decisions.
- **Consequences:** the latency SLA is measured continuously, not just in load tests; drift metrics double as a model-health signal.

<!-- @anchor decision:simulation refs:term:simulation-harness,term:synthetic-transaction-generator,term:fraud-pattern-injection,term:ground-truth-label,term:delayed-label -->
### D-018 — Simulation harness with injectable typologies
- **Context:** there is no real transaction feed; the system must be exercised and evaluated against known fraud.
- **Decision:** build a simulation harness — a synthetic transaction generator emitting legitimate baseline traffic plus parameterised fraud-pattern injection per typology, with ground-truth labels and simulated delayed labels (chargebacks).
- **Consequences:** ground-truth enables the evaluation harness (D-014); the generator's entity population feeds graph and velocity features; a simulation dashboard drives scenarios (D-019).

<!-- @anchor decision:dashboards refs:term:review-queue,term:simulation-harness -->
### D-019 — Two web dashboards (analyst + simulation), via design-creator
- **Context:** analysts need a case-management surface; operators need to drive and observe the simulation. Both are real visual web surfaces.
- **Decision:** build two prototype web dashboards — an analyst case-management surface (review queue, investigation, graph/link view, label capture) and a simulation control surface (scenario injection, live metrics). Their visual layer is produced via design-creator from a structured design-brief in a later phase; the engine exposes the APIs they consume.
- **Consequences:** triggers a design-brief handoff (structured constraints only); the engine's case and metrics APIs are designed for these consumers.

<!-- @anchor decision:verticals refs:term:card-vertical,term:p2p-vertical,term:crypto-vertical,term:vertical-pack -->
### D-020 — Shared core, vertical packs, card-first
- **Context:** card, P2P, and crypto fraud share an engine but differ in entities, features, rules, typologies, and actions.
- **Decision:** build a shared engine core and express each domain as a vertical pack (feature definitions, rules, typologies, decision actions) that plugs in. Order: card vertical first (end-to-end slice, phases 1–3), then P2P, then crypto.
- **Consequences:** the core's interfaces (event schema, feature contract, rule/action plug points) must be vertical-agnostic; P2P and crypto packs land in the final phase; crypto needs an on-chain ledger simulation (D-023).

<!-- @anchor decision:graph-backend refs:term:identity-graph,term:transaction-graph -->
### D-021 — Graph backend: in-process petgraph + Postgres
- **Context:** real-time graph signals need near-hot-path access, while full graph analytics are batch; a dedicated graph database (Neo4j/Memgraph) adds an external dependency and a network hop the latency SLA cannot absorb. _(Resolved from open question OQ-1.)_
- **Decision:** hold and traverse the identity and transaction graph in-process with the Rust `petgraph` library for real-time signals, and persist it in Postgres (adjacency tables); defer a dedicated graph engine until scale demands it.
- **Consequences:** batch graph features (centrality, community, motifs) are computed in Python over the Postgres-persisted graph and materialised to the online feature store; the hot path reads only precomputed graph features — no graph database on the hot path.

<!-- @anchor decision:ml-dataset-source refs:term:risk-model,term:ground-truth-label,term:synthetic-transaction-generator,term:label -->
### D-022 — ML dataset source: synthetic primary, public-set validation optional
- **Context:** there is no production transaction feed; models need labelled data with known fraud, plus an optional reality check. _(Resolved from open question OQ-2.)_
- **Decision:** train and evaluate primarily on the synthetic generator's labelled output (ground-truth plus simulated delayed labels); optionally validate against a public dataset (IEEE-CIS, AMLSim/AMLworld, Elliptic2) as a reality check — never as the primary training source.
- **Consequences:** the generator's typology coverage bounds what the model can learn, so generator richness is a first-order concern; public-set validation is an optional task, not a build dependency.

<!-- @anchor decision:crypto-sim-depth refs:term:on-chain-ledger,term:crypto-vertical,term:address-clustering,term:taint-tracing -->
### D-023 — Crypto simulation depth: synthetic ledger
- **Context:** the crypto vertical needs an on-chain substrate for clustering, tracing, and exposure scoring; replaying a real chain adds ingestion complexity and licensing concerns. _(Resolved from open question OQ-3.)_
- **Decision:** simulate a synthetic account/UTXO on-chain ledger generated by the simulator, with injectable mixer / peel-chain / sanctioned-cluster patterns; do not replay a real chain sample in the MVP.
- **Consequences:** address clustering, taint tracing, and exposure scoring operate on the synthetic ledger; a real-chain replay remains a future extension behind the same interfaces.

<!-- @anchor decision:step-up-depth refs:term:step-up,term:action -->
### D-024 — Step-up depth: outcome-only
- **Context:** step-up is a decision action, not a protocol the engine implements; simulating a full 3DS/OTP handshake adds surface without changing detection logic. _(Resolved from open question OQ-4.)_
- **Decision:** model the step-up challenge by its outcome only — `pass | fail | abandon` — fed back as a signal and a label; do not implement a full 3DS/OTP out-of-band protocol.
- **Consequences:** the simulator emits challenge outcomes with configurable distributions; the P2P pack's out-of-band verification reuses the same outcome model.
