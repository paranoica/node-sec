# node-sec

A real-time anti-fraud decision engine for payments, P2P transfers, and crypto — simulated
end-to-end, built for **speed + reliability** (p99 < 20 ms @ ~20k tx/s).

> Status: in development. Source of truth: `docs/`. Plan: `PLAN.md`. Rules: `AGENTS.md`.

## What it does

- Scores every transaction in real time and returns an **action** — approve / decline / step-up /
  review / hold — by fusing a deterministic rules engine with a calibrated ML model.
- Splits work into a **synchronous decision path** (Rust hot path, in-process ONNX inference) and an
  **asynchronous update path** (stream processor maintaining velocity, graph, and entity state).
- Detects networked fraud (rings, mules) over an identity + transaction **graph**, and runs a full
  **compliance layer**: immutable audit log, sanctions/PEP screening, AML monitoring, and SAR/STR
  with four-eyes.
- Is exercised by a **simulation harness** that injects parameterised fraud typologies with
  ground-truth and delayed (chargeback) labels.

Verticals ship in order: **card** (end-to-end first), then **P2P**, then **crypto**.

## Tech stack

Rust (tokio) hot path · Python (LightGBM → ONNX) for training & batch graph · Redpanda · Redis ·
Postgres · Prometheus + Grafana · gRPC. Two prototype web dashboards (analyst + simulation) via
design-creator.

## Getting started

_Filled in as the backlog delivers it._ Infra comes up via `docker compose up` (Redpanda, Redis,
Postgres, Prometheus, Grafana); the Rust workspace builds with `cargo build --workspace`; the Python
training tree lives under `ml/`. See `PLAN.md` for the current sprint.

## How this project is organised

- `docs/` — decisions, architecture, glossary, open questions (the spec; source of truth).
- `PLAN.md` — the task plan (generated; change task state via the backlog tool, not by hand).
- `AGENTS.md` / `CLAUDE.md` — the rules every coding session follows.
- `crates/` — the Rust workspace (engine, stream, features, rules, model, graph, compliance, …).
- `ml/` — the Python training, evaluation, and batch-graph code.
