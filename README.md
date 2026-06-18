# node-sec

**A real-time anti-fraud decision engine** for card payments, P2P transfers, and crypto —
simulated end-to-end, engineered for **speed + reliability**: `p99 < 20 ms @ ~20 000 tx/s`, with
**fail-safe degradation** (it would rather shed load than fail open to APPROVE), a full
**compliance layer** (immutable audit · sanctions/PEP · AML · SAR/STR with four-eyes), and two
**prototype operator dashboards**.

It fuses a deterministic, hot-reloadable **rules engine** with a calibrated **ML model**
(LightGBM → ONNX, scored in-process on the hot path) and a **graph** subsystem for networked fraud
(rings, mules), and is exercised by a **simulation harness** that injects parameterised fraud
typologies with ground-truth and delayed (chargeback) labels.

```
   transaction ──► DECISION ENGINE (Rust hot path, p99<20ms) ──► action + reason codes
                   │  online features · request-time derivation · rules · ONNX score
                   │  expected-value action selection · fail-safe degrade
                   └─ decision event ──► event backbone ──► async update path + audit log
```

---

> ## ⚠️ Status: prototype / skeleton — built to be developed further
>
> This is a **functional reference implementation** — the architecture is production-*shaped* and
> the core logic, invariants, and gates are real and tested. It is **not a production deployment**.
> All external feeds (card networks, chains, watchlists) are **simulated**; the ML model is trained
> on **synthetic** data, so its real-world fraud-catch rate is **unproven**; the latency SLA is
> validated by micro-benchmark + a chaos test, **not** by a full-scale load test on production
> hardware; and the operational layer (alerting, DR, canary, secrets, real integrations) is
> scaffolding, not hardened. See **[Maturity & roadmap](#maturity--what-it-would-take-for-production)**
> for an honest accounting. Treat this as a **skeleton you can extend**, not a system to point at
> live money.

---

## Table of contents

- [What it does](#what-it-does)
- [Architecture](#architecture)
- [The hard invariants](#the-hard-invariants)
- [Repository layout](#repository-layout)
- [Tech stack](#tech-stack)
- [The decision API](#the-decision-api)
- [Subsystems](#subsystems)
- [The dashboards](#the-dashboards)
- [Getting started](#getting-started)
- [Testing & gates](#testing--gates)
- [How the project is built (methodology)](#how-the-project-is-built-methodology)
- [Maturity & roadmap](#maturity--what-it-would-take-for-production)
- [License](#license)

---

## What it does

- **Scores every transaction in real time** and returns an **action** — `APPROVE` / `DECLINE` /
  `STEP_UP` / `REVIEW` / `HOLD` — by fusing deterministic rule outcomes (hard overrides win; soft
  signals feed the score) with a calibrated ML risk score, mapping to a risk band, and selecting the
  action by **expected-value decisioning** over a cost matrix.
- **Splits work into two paths (CQRS):** a synchronous **decision path** (the Rust hot path,
  in-process ONNX inference, read-only over precomputed state) and an asynchronous **update path** (a
  stream processor maintaining velocity, aggregate, graph, and entity state). *The decision request
  never blocks on a state write.*
- **Detects networked fraud** — rings, mules, fan-in/fan-out, peel chains — over an identity +
  transaction **graph**.
- **Runs a full compliance layer** — immutable audit log of every decision, reason-code
  explainability, sanctions/PEP screening, AML transaction monitoring with typology-tagged alerts, a
  case lifecycle, and **SAR/STR generation with four-eyes** (maker ≠ checker) and a tipping-off guard.
- **Degrades fail-safe** — any dependency exceeding its per-call timeout (e.g. a slow feature store)
  drops the decision to rules-only / last-known-good features and still answers **within** the SLA —
  it never blocks, and never fails open to APPROVE.
- **Is driven by a simulation harness** that injects parameterised fraud typologies with ground-truth
  and simulated delayed (chargeback) labels — there is no real transaction feed.

Verticals ship in order: **card** (built end-to-end first) → **P2P** → **crypto**, each as a
self-contained **vertical pack** plugged into a vertical-agnostic core.

## Architecture

Two paths, decoupled by an event backbone (decisions D-001, D-002, D-003):

```
                          ┌──────────────────────── SYNC: decision path (hot) ────────────────────────┐
   client ── gRPC ──►  DecisionService.Decide
                          │  1. online feature read (Redis, per-call budget 5ms)  ── timeout ─┐
                          │  2. request-time derivation (deviation features)                  │ fail-safe
                          │  3. rules engine (hot-reloadable config + blocklists)             ▼ degrade
                          │  4. in-process ONNX inference (LightGBM tree-ensemble)     rules-only / LKG
                          │  5. fuse → risk band → expected-value action selection            │
                          │  6. reason codes (rule hits + SHAP contributions)                 │
                          └──► action + reason codes ──► caller           (returns within p99 < 20ms)
                                       │
                                       └── decision event (full audit record) ──┐
                                                                                 ▼
   ┌─────────────────────────── ASYNC: update path ──────────────────────────────────────────────────┐
   transaction events ──► Redpanda (partitioned by entity key) ──► stream processor
                          │  per-entity windowed aggregates (1m/5m/1h/24h/7d/30d) ──► online store (Redis)
                          │  immutable audit log append                           ──► Postgres (system of record)
                          │  AML monitoring · graph updates · case alerts         ──► review queue
                          └──► offline feature store (training, point-in-time correct)
   batch (Python): entity resolution → identity graph → graph features (centrality/community/motifs) ──► online store
   feedback: investigator + chargeback labels → offline store → retrain → registry → shadow → champion
```

**Components** (full detail in [`docs/architecture.md`](docs/architecture.md), the *why* in
[`docs/decisions.md`](docs/decisions.md)):

| Component | Lang | Role |
|---|---|---|
| **decision-engine** | Rust | gRPC hot path; owns the latency SLA; rules + ONNX + policy + audit-record emit |
| **ingest** | Rust | event-backbone producer/consumer boundary; validates + publishes, keyed by entity |
| **stream-processor** | Rust | async update path; per-entity windowed aggregates → online store |
| **feature-store** | Redis + Postgres | one definition, two materialisations (online/offline) with parity |
| **rules-engine** | Rust | hot-reloadable rule config + blocklists; hard overrides + soft signals |
| **model** | Python → ONNX → Rust | LightGBM training (cost-sensitive + calibrated) → ONNX → in-process serving |
| **graph** | Python batch + Rust | entity resolution, transaction graph, ring/mule signals |
| **compliance** | Rust + Postgres | audit, sanctions, AML, case lifecycle, SAR/STR, four-eyes |
| **simulator** | Rust | synthetic traffic + fraud-pattern injection + ground-truth/delayed labels |
| **dashboards** | Web (Next.js) | analyst case-management + simulation control surfaces |
| **observability** | Prometheus + Grafana | p50/p99/p999 latency, throughput, decision mix, drift |

## The hard invariants

These are enforced design rules, not aspirations — anchored in `docs/architecture.md`, asserted in
code (`#![forbid(unsafe_code)]` in every core crate, `overflow-checks = true` on the release
profile), and checked by the code-review gate on every high-risk change:

| Invariant | Meaning |
|---|---|
| `arch:money-integer` | Money is **integer minor units + an explicit currency**. There is no floating-point money constructor anywhere. |
| `arch:hot-path-read-only` | The decision engine does **no** synchronous blocking write; all state mutation flows through the event backbone. |
| `arch:decision-within-budget` | Every decision returns within the SLA; a dependency over budget triggers fail-safe degradation, never a timeout. |
| `arch:audit-completeness` | Every decision emits **exactly one** immutable audit record sufficient for deterministic replay. |
| `arch:idempotent-decision` | A decision is idempotent per idempotency key: a retry returns the original decision with no extra state update. |
| `arch:partition-by-entity` | Backbone events are partitioned by entity key; ordering holds per-entity-key only. |
| `arch:maker-checker` | A SAR/STR cannot be approved/filed by the analyst who drafted it — four-eyes on every filing. |
| `arch:versioned-decision` | Every decision records the exact rule-config and model versions used, for reproducible replay. |
| `arch:feature-parity` | Each feature has one definition materialised identically online and offline; divergence is a defect. |
| `arch:vertical-agnostic-core` | The core contains no vertical-specific logic; card/P2P/crypto live entirely in vertical packs. |

## Repository layout

```
node-sec/
├── crates/                  # Rust workspace (hot path + async update path)
│   ├── domain/              # core types: money (integer), transaction, entity, decision  [arch:money-integer]
│   ├── engine/              # the synchronous decision engine — the gRPC hot path
│   ├── ingest/              # event-backbone producer/consumer boundary (Redpanda adapter)
│   ├── stream/              # async update path: per-entity windowed feature aggregation
│   ├── features/            # feature-store reads + request-time derivation (hot-path online read)
│   ├── rules/               # deterministic, hot-reloadable rules engine + blocklists
│   ├── model/               # in-process ONNX inference + SHAP-derived reason codes
│   ├── graph/               # entity resolution, transaction graph, ring/mule signals (petgraph)
│   ├── compliance/          # audit log · sanctions · AML · case lifecycle · SAR/STR (four-eyes)
│   ├── simulator/           # synthetic transaction generator + fraud-pattern injection
│   ├── api/                 # read-only dashboard APIs (analyst review queue, sim control)
│   └── verticals/{p2p,crypto}/   # vertical packs — depend INTO the core, never out of it
├── ml/                      # Python tree (uv): training, eval, batch graph — meets Rust only at ONNX/feature artifacts
│   ├── training/            # synthetic dataset + LightGBM training (cost-sensitive, calibrated)
│   ├── export/              # → ONNX (to_onnx.py); golden-parity checked against Rust serving
│   ├── explain/             # SHAP → reason-code vocabulary
│   ├── labels/ feedback/    # label joins, reject inference, delayed-label dataset
│   ├── eval/ registry/      # PR-AUC / recall@FPR / precision@N metrics; PSI drift
│   └── artifacts/           # fraud_lgbm.onnx + parity.json (the model handoff)
├── web/                     # two prototype dashboards (Next.js 15, static export)  →  / and /sim/
├── proto/                   # decision.proto — the gRPC contract
├── config/                  # hot-reloadable rule / AML / SAR config (rules-as-data)
├── deploy/                  # engine.toml (SLA + backpressure knobs), prometheus.yml
├── migrations/              # Postgres DDL (incl. the append-only audit-log immutability trigger)
├── scripts/                 # healthcheck · integration-test · chaos
├── docs/                    # decisions · architecture · glossary · open-questions  (SOURCE OF TRUTH)
├── docker-compose.yml       # local infra: Redpanda · Redis · Postgres · Prometheus · Grafana
└── PLAN.md / genesis.tasks.json / AGENTS.md   # the build plan + rules (see methodology)
```

## Tech stack

- **Hot path / async path:** Rust (tokio), `tonic` (gRPC), `axum`, `dashmap` (sharded per-entity
  state), `petgraph` (in-process graph), `arc-swap` (hot config reload). `rustfmt` + `clippy -D
  warnings`; release profile `overflow-checks = true` (money math panics loudly off the hot path
  rather than wrapping silently).
- **ML / batch:** Python 3.12+, LightGBM, scikit-learn, ONNX / onnxruntime, networkx, managed with
  `uv`; `ruff` + `pytest`.
- **Infrastructure:** **Redpanda** (Kafka-compatible event backbone), **Redis** (online feature
  store), **Postgres** (system of record: transactions, audit, cases, labels), **Prometheus +
  Grafana** (observability).
- **Dashboards:** Next.js 15 / React 19, static export, no backend runtime (mock data shaped like the
  real `api` endpoints — swap to live is a one-line `fetch`). Built via the **design-creator** engine.

## The decision API

`proto/decision.proto` — `DecisionService.Decide`, idempotent per `idempotency_key` (D-016):

```protobuf
message DecisionRequest {
  string idempotency_key    = 1;   // required — makes the call idempotent
  string transaction_id     = 2;
  int64  amount_minor_units = 3;   // integer minor units  [arch:money-integer]
  string currency           = 4;   // ISO-4217: USD, EUR, GBP, JPY
  string vertical           = 5;   // CARD | P2P | CRYPTO
  // … channel, pan, merchant, device, occurred_at …
  uint32 mcc                = 11;  // enrichment → wakes MCC/AVS/CVV/geo rules + high_risk_mcc feature
  optional bool avs_match   = 12;
  optional bool cvv_match   = 13;
  string geo_country        = 15;  string ip = 14;  double geo_lat = 16; // geo_lon = 17
}

message DecisionResponse {
  string action             = 2;   // APPROVE | DECLINE | STEP_UP | REVIEW | HOLD
  double score              = 3;
  string band               = 4;   // LOW | MEDIUM | HIGH | VERY_HIGH
  repeated string reason_codes = 5;
  string rule_version       = 6;   // … + model_version (versioned-decision, replayable)
}
```

## Subsystems

- **Rules engine** (`crates/rules`, config `config/rules/`) — rules **are data** (D-015): velocity,
  card-testing, BIN-attack, decline-retry-storm, impossible-travel, amount-anomaly, MCC, AVS/CVV,
  blocklists — each carrying a severity, typology tag, and reason code, **hot-reloaded** without a
  restart. Hard overrides win; soft signals feed the score.
- **ML model** (`ml/` → `crates/model`) — LightGBM trained cost-sensitively with calibrated outputs
  on the synthetic generator's labelled traffic (D-022), exported to ONNX, scored **in-process** on
  the hot path (no network hop), with SHAP-derived reason codes verified against a Python golden
  (`ml/artifacts/parity.json`). Evaluated with **PR-AUC / recall@FPR / precision@N** — accuracy is
  banned under this class imbalance (D-014). Lifecycle: registry → champion-challenger → shadow → PSI
  drift (D-013).
- **Graph** (`crates/graph` + `ml/graph`) — entity resolution (normalise → block → match → cluster)
  → identity graph; time-stamped transaction graph; batch graph features (centrality, community,
  cycles, fan-in/fan-out, motifs) materialised back to the online store; cheap signals on the hot
  path. In-process `petgraph` + Postgres adjacency — **no graph DB on the hot path** (D-021).
- **Compliance** (`crates/compliance`, config `config/{aml,sar}/`) — append-only **audit log** (with
  a Postgres immutability trigger, `migrations/0001_audit_log.sql`); **sanctions/PEP** screening
  (fuzzy name match); **AML** monitoring (structuring, funnel, round-tripping typologies); the **case
  lifecycle** (`alert → triage → investigate → close | escalate | file-sar`); **SAR/STR** with
  deadlines, four-eyes, and a tipping-off prohibition.
- **Simulation harness** (`crates/simulator`) — a reproducible synthetic generator over a fixed
  entity population (so velocity/graph features are meaningful), emitting legitimate baseline traffic
  plus injectable fraud typologies with ground-truth and simulated delayed (chargeback) labels
  (D-018). The crypto vertical runs on a **synthetic on-chain ledger** with injectable
  mixer/peel-chain/sanctioned-cluster patterns (D-023).

## The dashboards

Two **prototype** operator surfaces under `web/` (Next.js, static export, mock data shaped like the
`crates/api` endpoints). Built with the **design-creator** engine — data-dense, dark, tabular-mono;
each passed `verify.mjs` (Playwright + axe + Core-Web-Vitals) at **MEASURED_PASS 20/0** plus an
independent **Tier-A critic → SHIP**.

- **Analyst console** (`/`) — a risk-prioritised case workbench. The queue is a **calibrated
  risk-spine** (each case a meter on one 0–1 lane; the descending bar-ends read the caseload as a
  gradient before any text). Selecting a case shows the **disputed transaction(s) + a behavioural
  timeline** (money as integer minor units), alerts, evidence, reason codes, and graph links — then
  the analyst **dispositions** it: `Assign to me · Confirm fraud · Clear · Escalate · Block card`,
  with **four-eyes** on Confirm-fraud / File-SAR (a *different* analyst approves; the server rejects
  self-approval), notes → immutable audit, and queue filters (status / alert-type / search).
- **Simulation control** (`/sim/`) — a load-harness operator console. Hook 1 — **the SLA wall**: a
  live `p99 × throughput` plane where the engine's operating point streams toward a hard
  `p99<20ms @ 20k` boundary; crank the load and the point climbs the queueing curve to the wall,
  turns red, and the engine **fail-safe sheds load** (`shedding N/s`, DEGRADE) to hold p99 inside the
  SLA. Hook 2 — **the live pipeline**: `rules → features → ML → graph → compliance` with per-stage
  throughput and back-pressure (the bottleneck stage shows a red `queue +N/s`). Controls: run/pause,
  traffic scenario, downstream-fault injection, target-load slider.

## Getting started

> Requires: a recent **Rust** toolchain (workspace pins `rust-version = 1.90`), **Docker** +
> `docker compose`, **uv** (for the Python tree), and **Node** (for the dashboards).

```bash
# 1. Local infrastructure — Redpanda · Redis · Postgres · Prometheus · Grafana
docker compose up -d
scripts/healthcheck.sh            # waits for every service to report healthy

# 2. Build + test the Rust workspace
cargo build --workspace
cargo test --all
cargo clippy --all-targets -- -D warnings    # CI runs this with -D warnings
cargo fmt --all -- --check

# 3. Python ML tree (training, eval, batch graph)
cd ml && uv sync --dev
uv run ruff check . && uv run pytest -q
#   training → ONNX export lands the model at ml/artifacts/fraud_lgbm.onnx
cd ..

# 4. Dashboards (prototypes)
cd web && npm install
npm run dev                       # http://localhost:3000  ( / = analyst, /sim/ = simulation )
#   or:  npm run build            # static export → web/out/
```

**Service ports** (overridable via `.env`, see `.env.example`): Redpanda `9092` (admin `9644`, proxy
`8082`) · Redis `6379` · Postgres `5432` · Prometheus `9090` · Grafana `3000`.

**SLA / backpressure knobs** live in `deploy/engine.toml` (p99 budget 20 ms, target 20k tx/s,
feature-read budget 5 ms → `degrade_rules_only` on fault, bounded admission `max_in_flight = 4096`).

## Testing & gates

| Gate | Command | What it checks |
|---|---|---|
| **CI** (`.github/workflows/ci.yml`) | push / PR | Rust: fmt + `clippy -D warnings` + `cargo test --all` + release build · Python: `ruff` + `pytest` |
| **Unit / component** | `cargo test --all`, `uv run pytest -q` | per-crate + ML logic, ONNX golden parity |
| **Integration** | `scripts/integration-test.sh` | the `#[ignore]`d tests against **live** dockerised Postgres + Redis (audit immutability trigger, online-store round-trip) |
| **Chaos / fail-safe** | `scripts/chaos.sh` | faults the feature store under load; asserts fail-safe degradation holds **and** p99 < 20 ms still met |
| **Load / SLA** | `cargo bench -p engine load_sla` | the p99 < 20 ms @ ~20k tx/s budget (returns non-zero on breach) |
| **Code review** | `code-review` skill | full no-mercy gate on high-risk surfaces (hot path · money · compliance) |

> Heads-up on the integration/chaos/load gates: if you run a VPN with a kill-switch (e.g.
> Windscribe), the host↔container path can hang — allow the docker subnet in both directions
> (`iptables -I INPUT/OUTPUT 1 -s/-d 172.16.0.0/12 -j ACCEPT`). `scripts/integration-test.sh`
> handles this.

## How the project is built (methodology)

This repo is driven by a file-based agent methodology (the **bedrock / genesis** skills):

- **Source of truth** (highest wins): `docs/decisions.md` → `architecture.md` → `glossary.md` →
  `open-questions.md`. Never invent a missing decision — it's recorded as `TODO(decision: …)`.
- **Task state** lives in `genesis.tasks.json` (single source); `PLAN.md` is **generated** — change
  state only via `python3 .claude/skills/genesis/scripts/backlog.py --root . …`, never by hand.
- **Gates:** design → design-creator; code audit → code-review (implement → review → apply fixes →
  re-review); the hot path / money / compliance are **always** full-review surfaces.
- The universal rules every coding agent follows are in **`AGENTS.md`**.

The build ran as 7 sprints, **all 42 tasks complete**:

```
S0 foundation (5) · S1 card-slice (7) · S2 feature-store (5) · S3 ml-scoring (7)
S4 graph (5)      · S5 compliance (6) · S6 verticals-prod (7)
```

## Maturity — what it would take for production

Honest accounting (the whole point of the prototype banner up top). **Design quality** is high
(~70–80%); **total effort to a production deployment** is more like ~25–35%, because the simulated
boundaries + ops + ML-efficacy proof + scale-hardening are the majority of real-world cost.

**Solid / production-shaped:** the two-path architecture, the integer-money + checked-arithmetic
discipline, in-process ONNX (no Python on the hot path), fail-safe degrade (never fail-open), the
four-eyes + immutable-audit compliance model, and the review gates that caught real bugs.

**The gap to production (roughly by importance):**

1. **ML efficacy is unproven.** The model is trained on synthetic data — it passes tests, but
   *whether it catches real fraud is unknown*. Needs real labelled data, precision/recall on it,
   drift monitoring, shadow deployment — and may need retraining/rework, not just validation.
2. **All external feeds are simulated** (card networks, chains, watchlists). Real integrations bring
   their own failure modes, rate limits, auth, data quality, and fuzzy-match tuning.
3. **Scale not validated live.** `p99 < 20 ms @ 20k tx/s` is architectural confidence + micro-bench +
   chaos test, not a sustained load test under real distributions on production hardware.
4. **Operational maturity is scaffolding** — SLO alerting, runbooks, distributed tracing, DR /
   backup-restore drills, canary/rollback, secret management, SBOM/CVE-in-CI are not there.
5. **Resilience tested shallowly** — end-to-end backpressure, per-downstream circuit breakers,
   exactly/at-least-once on the backbone, poison-message + replay/recovery under real fault injection.
6. **Regulatory hardening** — the compliance *model* is right; production needs live watchlist feeds,
   jurisdiction rules, examiner-auditable trails, data residency, and actual regulator sign-off.

**Bottom line:** the skeleton is correct and extensible, but "production" for a money-moving,
compliance-bound fraud engine is a high bar — and the single honest blocker is **#1: nobody has
proven it catches fraud yet.** Running it against real situations (data, integrations, load) is the
gate that reveals how much of the above is left to *build*, not merely test.

## License

MIT (`license = "MIT"`, workspace package metadata). Prototype / educational — see the status banner;
not for production use against live funds without the hardening above.
