# Design brief — Simulation control dashboard (T066)

> Structured constraints for a design-creator handoff. Constraints only — no narrative, no copy,
> no visual prescriptions beyond what the data and tasks require.

## Surface
- Single-page web control panel. Operator-facing, internal tool.
- Two regions: (1) scenario control, (2) a live, streaming results view.

## Data contract
Scenario list — `GET /sim/scenarios` → JSON array of `{id, typology}`:
`legitimate`, `card_testing`, `high_amount`, `blocklisted_bin`.

Live stream — `GET /sim/stream?typology=<id>&count=<n>` → **Server-Sent Events**. Two event shapes,
discriminated by `type`:

| `type` | fields | use |
|---|---|---|
| `decision` | `transaction_id`, `action` (`APPROVE`/`DECLINE`/`STEP_UP`/`REVIEW`/`HOLD`), `score` (0–1), `reason_codes[]` | append to the live decision feed |
| `metrics` | `processed`, `approved`, `declined`, `alerts`, `decline_rate` | update the running summary (interleaved every 10 decisions + final) |

## Task constraints (what the operator must do)
- Pick one typology and a volume, then start the run.
- Watch decisions arrive live, distinguishing `APPROVE` from `DECLINE`/alerts at a glance.
- Track the running summary (decline rate, alert count) updating during the run.
- Re-run with a different typology to compare engine response.

## Hard constraints
- The decision feed is **append-only and streaming**; new events arrive continuously — design for
  a moving feed, not a static table fetch. Keep the latest events in view.
- `decision` vs `metrics` events share one stream; render them into their two distinct regions.
- Action severity (`DECLINE`/`HOLD` vs `APPROVE`) must be distinguishable without colour alone
  (WCAG AA; non-colour cue required).
- A high-volume run emits events faster than a human reads — cap/virtualise the rendered feed and
  show that older events scrolled off (never imply every event was shown when it was truncated).
- Handle stream end and reconnect; a dropped SSE connection must be visible, not silent.
- No PII beyond the synthetic identifiers in the payload.

## Non-goals
- No editing of rules/thresholds from this surface (read-only control of the simulator).
- No historical run storage / comparison across sessions in this iteration.
- No auth screens (handled by the platform shell).
