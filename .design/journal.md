# Design journal — node-sec dashboards

## Analyst case-management dashboard (Stage 1 mockup) — 2026-06-18

**Deliverable:** app surface (internal fraud-ops console). No auth/landing/marketing — dashboards only, by owner's instruction.

**Family — data-dense pro.** Domain-fit guard: fraud/security → conventional polish *is* the trust
signal; playful/brutalist would erode credibility. Data-dense pro (tight grid, tabular numerals,
dark-first) is the natural fit for a risk-prioritised case queue.

**Mode — Clean.** Function-first ops tool. The "wow" comes from the enacted hook, not decoration.

**Hook — the calibrated risk-spine.** The worklist's left lane is one continuous 0–1 risk axis with
reference gridlines (.25/.50/.75); each case is a bar + tick on that axis, rows uniform & dense for
comparison. The descending bar-ends trace the caseload as a *gradient* — the analyst sees where to
look before reading text. Severity is encoded three redundant ways — **bar length + tick position +
numeral** — so it is **not colour-dependent** (WCAG). Selecting a case opens detail
(alerts/evidence/reason-codes/graph-links); new cases stream onto the spine live (4.2 s) so the risk
landscape visibly shifts. Mechanism = **content-as-mechanism** (real risk data is the layout axis),
chosen deliberately over the over-used scroll-morph.

**Palette.** Dark-first deep near-black (`#0b0f14`, not `#000`); single warm-amber signal accent
(`#f0a52a` dark / `#b46a00` light), ≤10% of screen, redundant with position so colour is never the
sole severity cue. **Palette is a guess — no brand assets given.**

**Type.** Tabular-mono numerals are the signature voice (the data-dense "hero"); a neutral grotesque
for labels. Avoids Inter/Roboto as display voice. Font face is a guess (no brand).

**Perf.** LCP 784–896 ms, CLS 0.000, no 320 px overflow, axe 0 serious/critical — both themes ×
{320,768,1280}, hash-bound `verify-report.json`.

**Notable trade-offs / decisions:**
- `html { overflow: clip }` — full-height app shell with internal scroll regions; clips the
  off-canvas detail drawer so it can't inflate horizontal scroll width.
- Theme follows OS `prefers-color-scheme` by default; `[data-theme]` toggle overrides.
- Mobile (≤560) hides the lane tick-labels + gridlines (bars + numerals carry the signal); detail
  becomes a right slide-over drawer with a scrim.
- `.statepill` (state label + square) is borderline vs the AI-pill tell — critic ruled it
  non-blocking (inline flow label, lowercase, not a floating mono-caps carrier). Could flatten later.

**Gate:** verify.mjs MEASURED_PASS (20/0) + independent Tier-A critic → SHIP, blocking_fails []. 

### v2 — rebuilt in Next.js (App Router, static export) — 2026-06-18
Owner rejected the single-file HTML v1: bars uncomfortable to scan, gridlines vanished under the
bars, cramped RISK header, default scrollbar, flicker on live update + theme switch. Also wanted
**React/Next**, not a lone HTML file.

Rebuilt under `web/` (Next 15 + React 19, `output: export` → `out/`, served for the QA gate).
Fixes:
- **Calmer risk-spine** — a fixed-width contained meter (track + fill + knob) per row instead of
  full-width ragged bars; the knob marks risk position, numeral is primary. Less visual noise.
- **Gridlines on top** — the `.25/.50/.75` axis is a fixed overlay (`z-index:4`) over the list, so
  the lines are always visible and never hidden by the meter (the exact v1 complaint).
- **Header de-collided** — `.25` label dropped from the header (gridline kept), cap + `.50/.75`
  spaced; meter geometry is fixed so header ticks, gridlines and per-row meters align exactly.
- **Custom scrollbar** — themed `::-webkit-scrollbar` + `scrollbar-width:thin` + `scrollbar-gutter:stable`.
- **No flicker** — keyed React list → live insert mounts only the new row (the rest stay put, scroll
  kept); smooth `background-color`/`color` transitions on theme switch.

Gate: verify.mjs MEASURED_PASS 20/0 (axe 0 serious, no 320 overflow, CLS 0.000, LCP ≤808ms, both
themes×{320,768,1280}) + independent Tier-A critic → SHIP, blocking_fails []. v1 HTML kept as history.

### v2.1 — owner-feedback iteration (forced dark · spine cleanup · status dot) — 2026-06-18
Owner reviewed the rendered v2 ("в целом норм думаю") with targeted corrections. Applied:
- **Per-row gridlines removed entirely** (the `.axis` overlay). They were the recurring complaint
  across v1→v2 (vanish under bars → clash on top of bars). The header scale `.25/.50/.75` is now
  the *only* calibration legend; the descending meter bar-ends carry the spine. Tier-A critic
  re-confirmed the hook still reads without them (severity = bar length + knob position + numeral).
- **Forced dark** — theme toggle + all light-theme CSS/icon machinery removed (owner wants one
  theme). `tokens.json` `meta.themes` updated to the single forced-dark contract. Light colour
  ramp retained in tokens as a dormant contract, not shipped.
- **Brand dot = connection status** — replaced the "live · streaming" text label. Orange pulse =
  live, amber = connecting, red = down; wired to WS/SSE `readyState` in production, mock stays live.
- **Detail header restructured** — was `[big number] [case/subject/state/risk·tier]`. Now a vertical
  risk block: **`RISK` (27px word) → `0.96` (52px number) → tier**, with case/subject/state to its
  right; the redundant `risk · tier` line dropped. Owner asked for the big word + number moved into
  the empty space beneath it.
- Removed "tier" word from the queue tier column.
- **Header label simplified** — the `.25/.50/.75` numeric scale (the last axis legend after gridlines
  went) is replaced by a single `RISK` column header matching SUBJECT/TIER/ALERTS. Per-row numerals
  carry the exact 0-1 value, so the spine stays calibrated without a separate axis. Owner request.

Gate: verify.mjs MEASURED_PASS 20/0 (axe 0 serious, no 320 overflow, CLS 0.000, LCP 768–832ms,
forced dark ×{320,768,1280}) + independent **Tier-A** critic (fresh-context subagent) → **SHIP**,
blocking_fails []. Token conformance confirmed in built CSS; no anti-slop TIER-1 tells.

### v3 — analyst workbench: disposition + four-eyes + notes + filters — 2026-06-18
Owner caught the real gap: the console was a **viewer, not a workbench** ("я зашёл, посмотрел — и чо
дальше?"). A fraud analyst doesn't admire a case, they *resolve* it. Added the act-side:
- **Disposition / action layer** (sticky bar in the detail): `Assign to me` · `Confirm fraud` ·
  `Clear (false positive)` · `Escalate` · `Block card` (card subjects only). Each drives a case
  status: `alert/triage/investigate → in review → confirmed fraud / cleared / escalated`.
- **Four-eyes (maker-checker)** on the high-impact actions: `Confirm fraud` and `File SAR` submit to
  a `Pending four-eyes` state; a **different** analyst must `Approve as reviewer` (caption states the
  server rejects self-approval). Mirrors the bedrock compliance invariant (`crates/compliance/{cases,
  sar,audit}`). Disposition + pending banners stack (fraud confirmed *and* SAR awaiting a reviewer).
- **Notes → immutable audit**: system + analyst notes with an add-note input; every action also drops
  a system note. Closed-case rows desaturate their risk meter so the spine still reads open-vs-closed.
- **Queue filters**: segmented `All / Mine / Open / Closed` + free-text search (subject / case-id) +
  alert-type dropdown. The "open cases" stat counts un-disposed cases.
- **Custom dropdown** — replaced the native `<select>` (its open option list is OS-drawn and
  unstylable) with an own listbox component (outside-click / Escape close, arrow-key nav, amber
  selected + check). Owner caught that the native popup leaked default chrome; the Tier-A critic had
  missed it (it only inspected the closed control) — logged as a critic-miss.
- **Evidence mock fixed** — `kind` and `detail` were paired at random ("VELOCITY → OFAC SDN", dupes);
  now each evidence fact is a coherent `{kind, detail}` pair.

Gate: verify.mjs MEASURED_PASS 20/0 (axe 0 serious, no 320 overflow, CLS 0.000, LCP ≤808ms) +
Tier-A critic → SHIP (caveat: critic passed the dropdown that the owner then rejected — fixed after).
Interaction flows (four-eyes pending→approve, SAR, notes, filters, custom dropdown) driven + screenshotted.

## Simulation control dashboard — pending (next, also Next.js) — BOTH hooks: SLA-wall (hero) + live-pipeline (section)
