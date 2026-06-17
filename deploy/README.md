# Deployment & production-readiness (T065)

The hot path holds a **p99 < 20 ms** decision budget at **~20k tx/s** (D-003,
`arch:decision-within-budget`). This directory holds the production knobs; two tests validate them.

## Validate

```sh
# Load test to SLA (healthy store) — fails non-zero if p99 breaches 20 ms.
cargo bench -p engine --bench load_sla

# Chaos: fault the online store under load — fail-safe must hold and the SLA must still be met.
scripts/chaos.sh
```

Reference numbers on an 8-core dev box:

| scenario        | throughput   | p99     | degraded | SLA |
|-----------------|--------------|---------|----------|-----|
| healthy store   | ~25k tx/s    | ~2.4 ms | no       | OK  |
| store faulted   | ~43k tx/s    | ~1.1 ms | yes      | OK  |

(Under fault the store-read path short-circuits to rules-only, so it is *faster*, not slower — the
point of fail-safe degradation: a degraded answer within budget beats a timeout.)

## How the SLA is held

- **Bounded feature read** — the online store read carries a per-call budget (`read_budget_ms`, 5 ms)
  well under the 20 ms end-to-end SLA. A slow/failed read degrades to the rules-only decision
  (`DEGRADED_RULES_ONLY`) instead of blocking the hot path. End-to-end p99 ≤ rules p99 + read budget
  **by construction**, regardless of store health.
- **Backpressure** — admission is bounded (`max_in_flight`, `accept_queue`); past the bound requests
  are shed with a retriable status rather than queued unboundedly, so the tail stays bounded under
  overload.
- **Fail-safe, not fail-open** — degradation drops to rules-only, never to a blind `APPROVE`.

See [`engine.toml`](engine.toml) for the knobs.
