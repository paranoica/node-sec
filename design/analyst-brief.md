# Design brief — Analyst review-queue dashboard (T055)

> Structured constraints for a design-creator handoff. Constraints only — no narrative, no copy,
> no visual prescriptions beyond what the data and tasks require.

## Surface
- Single-page web dashboard. Analyst-facing, internal tool (authenticated staff only).
- Two regions: (1) prioritised case list, (2) case detail for the selected case.

## Data contract (read-only)
Source: `GET /queue` → JSON array, pre-sorted by `risk` descending. One object per case:

| field | type | use |
|---|---|---|
| `case_id` | string | stable identifier, dedupe key |
| `subject` | string | the entity under review |
| `risk` | number 0–1 | queue order + severity emphasis |
| `state` | enum: `alert`,`triage`,`investigate`,`closed`,`escalated`,`sar_filed` | status indicator |
| `alerts` | string[] | list of triggered alerts (typology/screening/rule tags) |
| `evidence` | `{kind,detail}[]` | supporting signals |
| `reason_codes` | string[] | model reason codes |
| `graph_links` | `{counterparty,relation,weight}[]` | connected entities |

The API is read-only; this surface performs no writes.

## Task constraints (what the analyst must do)
- Scan the queue top-down: highest `risk` must be unmistakably first and most prominent.
- Triage without leaving the list: `state` and `alerts` legible at the row level.
- Drill into one case: detail region shows all of `evidence`, `reason_codes`, `graph_links`.
- Compare across cases quickly: row layout must be uniform and dense.

## Hard constraints
- Risk ordering is authoritative — never reorder client-side away from `risk` descending.
- Render every field; empty arrays are valid and must show an explicit empty state, never a gap
  that reads as "loading".
- No subject-facing actions, no PII beyond `subject`/`counterparty` identifiers present in the
  payload. Do not synthesise data not in the contract.
- Severity encoding must not rely on colour alone (WCAG AA; non-colour cue required).
- Above-the-fold target: first ~15 rows without scroll at 1366×768.
- Performance: list render budget < 100 ms for 500 cases; detail switch < 50 ms.

## Non-goals
- No case mutation / disposition UI (separate surface, four-eyes governed).
- No charts/timeseries in this iteration.
- No auth screens (handled by the platform shell).
