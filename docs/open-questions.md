# Open questions — node-sec (anti-fraud decision engine)

> Unresolved decisions. Each is anchored with the `decision:` slug it will graduate to in
> `decisions.md`. A task may list an open `decision:` in its `spec_refs`; `spec-analyze` then flags
> that task as resting on an unresolved decision (not execution-ready until resolved).
> _Last updated: 2026-06-17._

_None open._ The four inception-time questions were resolved into `decisions.md`, each keeping its
slug so the tasks that traced it stayed valid:

- `decision:graph-backend` → **D-021** (in-process petgraph + Postgres).
- `decision:ml-dataset-source` → **D-022** (synthetic primary, public-set validation optional).
- `decision:crypto-sim-depth` → **D-023** (synthetic on-chain ledger).
- `decision:step-up-depth` → **D-024** (outcome-only step-up).

New unknowns that surface during implementation go here as `TODO(decision: …)` with a `decision:`
anchor, and graduate to `decisions.md` when resolved.
