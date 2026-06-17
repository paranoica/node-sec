// Mock SSE /sim/stream telemetry. In production this is a Server-Sent-Events feed from the load
// harness; here we synthesise one frame per tick from the operator controls. The model is a simple
// M/M/1-ish queue: p99 latency blows up as utilisation ρ → 1, and the engine FAIL-SAFE sheds load
// to hold p99 inside the SLA rather than letting latency cross the wall (never fail-open to APPROVE).

export const SCENARIOS = [
  { value: "baseline", label: "Baseline traffic" },
  { value: "fraud-surge", label: "Fraud surge" },
  { value: "card-testing", label: "Card-testing attack" },
  { value: "degrade", label: "Downstream degrade" },
];
export const STAGES = ["rules", "features", "ML", "graph", "compliance"];
export const SLA = { tps: 20000, p99: 20 }; // the wall: p99 < 20 ms @ 20k tx/s
export const TPS_MAX = 25000;
export const P99_MAX = 30;

// sustainable service capacity (tx/s within SLA) per scenario; an injected fault cuts it hard
function capacity(scenario, fault) {
  const base = { baseline: 23000, "fraud-surge": 20500, "card-testing": 21500, degrade: 14000 }[scenario] ?? 23000;
  return fault ? base * 0.6 : base;
}
// p99 (ms) vs utilisation ρ — ~3ms idle, climbs steeply past ρ≈0.85 (queueing)
function p99For(rho) {
  const r = Math.min(0.97, Math.max(0, rho));
  return 3 + 2.4 * (r * r) / (1 - r);
}
// utilisation that yields a given p99 (inverse of p99For) — used for the shed threshold + wall
function rhoForP99(T) {
  const c = (T - 3) / 2.4;
  return (-c + Math.sqrt(c * c + 4 * c)) / 2;
}
const BOTTLENECK = { baseline: "ML", "fraud-surge": "ML", "card-testing": "graph", degrade: "compliance" };
const MIX = {
  baseline: { decline: 0.018, review: 0.05 },
  "fraud-surge": { decline: 0.14, review: 0.17 },
  "card-testing": { decline: 0.22, review: 0.08 },
  degrade: { decline: 0.03, review: 0.07 },
};

// one telemetry frame from the controls; jitter ∈ [-1,1] (0 → deterministic first paint)
export function frame(ctl, jitter = 0) {
  const { running, targetTps, scenario, fault } = ctl;
  const cap = capacity(scenario, fault);
  const wallTps = Math.round(cap * rhoForP99(SLA.p99));
  if (!running) {
    return { running: false, offered: 0, admitted: 0, shed: 0, rho: 0, p50: 0, p95: 0, p99: 3,
      degrade: false, mix: { approve: 0, decline: 0, review: 0 }, wallTps,
      stages: STAGES.map((s) => ({ stage: s, tps: 0, queue: 0, bottleneck: s === BOTTLENECK[scenario] })) };
  }
  const offered = Math.max(0, Math.round(targetTps * (1 + 0.02 * jitter)));
  const maxAdmit = cap * rhoForP99(SLA.p99 - 1); // fail-safe: shed to hold p99 ~19ms (inside the wall)
  let admitted = offered, degrade = false;
  if (offered > maxAdmit) { admitted = Math.round(maxAdmit); degrade = true; }
  const shed = offered - admitted;
  const rho = admitted / cap;
  const p99 = Math.min(P99_MAX, +(p99For(rho) * (1 + 0.012 * jitter)).toFixed(1));
  const p95 = +(p99 * 0.72).toFixed(1);
  const p50 = +(p99 * 0.41).toFixed(1);
  const mb = MIX[scenario];
  const mix = { approve: +(1 - mb.decline - mb.review).toFixed(3), decline: mb.decline, review: mb.review };
  const stages = STAGES.map((s, i) => {
    const tps = Math.round(admitted * (1 - 0.0025 * i));
    const queue = s === BOTTLENECK[scenario] && rho > 0.72 ? Math.round((rho - 0.72) * cap * 0.5) : 0;
    return { stage: s, tps, queue, bottleneck: s === BOTTLENECK[scenario] };
  });
  return { running: true, offered, admitted, shed, rho, p50, p95, p99, degrade, mix, stages, wallTps };
}

// seed a short ramp so the SLA plane reads as a live trail on first paint (deterministic)
export function seedTrail(ctl, n = 18) {
  const out = [];
  for (let i = 1; i <= n; i++) {
    const f = frame({ ...ctl, targetTps: ctl.targetTps * (0.3 + 0.7 * (i / n)) }, 0);
    out.push({ tps: f.admitted, p99: f.p99 });
  }
  return out;
}
