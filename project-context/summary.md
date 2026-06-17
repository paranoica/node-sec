# Project context — node-sec (distilled summary)

> **History, NOT a source of truth.** On any conflict, `docs/` wins (it is later and resolved).
> Captured 2026-06-17 from the genesis inception interview + a domain research pass.

## What the user asked for

A "really good" anti-fraud system for banks (cards) + crypto + P2P — mostly simulated, but realistic.
Hard goal stated twice: **maximum speed + reliability**. Language explored: C / C++ / Rust → resolved
to **Rust** for the hot path (predictable tail latency, memory safety for money), Python for ML.

## Interview decisions (the four forks that shaped the spec)

1. **Lead vertical:** card / payments first (richest signals, best documented), then P2P, then crypto.
2. **Latency SLA:** **p99 < 20 ms @ ~20k tx/s** (HFT-grade) — the user picked the most aggressive
   target; it drives in-process ONNX, local caches, fail-safe degradation.
3. **Surfaces:** the anti-fraud engine itself + **two prototype web dashboards** (analyst
   case-management + simulation control) → design-creator engaged later (S5/S6).
4. **Compliance:** **full** — audit log + reason codes + sanctions/PEP screening + AML/SAR simulation.

Depth: realistic stack in docker-compose (Redpanda, Redis, Postgres, Prometheus, Grafana), driven by
a synthetic transaction generator with injectable fraud typologies.

## Research provenance

The spec's typology/feature/rule coverage was sourced from a four-stream web research pass
(2023–2026): (1) card/payment fraud, (2) real-time fraud architecture + ML, (3) AML + graph
detection, (4) crypto + P2P fraud. Key references behind the glossary and AML rule shapes:
Stripe Radar / Sift / Feedzai engineering, the Fraud-Detection Handbook (PR-AUC vs ROC-AUC,
precision@k), FATF/FinCEN/OFAC + Wolfsberg typologies, Neo4j/TigerGraph graph-AML material,
Chainalysis/Elliptic/TRM crypto-crime reports, UK Finance / FCA APP-fraud + mule reports. These are
background; the authoritative restatement lives in `docs/glossary.md` and `docs/decisions.md`.

## Phase map (sprints)

- **S0 foundation** — workspace, domain types (money = integer minor units), docker-compose infra,
  generator v0, CI.
- **S1 card slice (rules-only)** — gRPC engine, hot-reloadable rules, card typologies, decisioning v0,
  audit log, latency harness.
- **S2 feature store** — async stream processor, online/offline stores, parity, fail-safe degradation.
- **S3 ML scoring** — LightGBM → ONNX in-process, fusion + expected-value, SHAP reason codes, delayed
  labels + holdout, evaluation harness, registry + drift.
- **S4 graph** — entity resolution, transaction graph, graph features, ring + motif detection, mules.
- **S5 compliance** — sanctions screening, AML rules, case lifecycle + four-eyes, SAR/STR, feedback
  loop, analyst dashboard API.
- **S6 verticals + prod** — P2P pack, crypto pack, load-test to SLA, simulation dashboard.

## Open decisions (recorded in docs/open-questions.md, with leanings)

- OQ-1 graph backend (lean: petgraph + Postgres) — blocks T040/T041.
- OQ-2 ML dataset source (lean: synthetic primary) — blocks T030.
- OQ-3 crypto sim depth (lean: synthetic ledger) — blocks T062.
- OQ-4 step-up depth (lean: outcome-only) — blocks T014/T061.
