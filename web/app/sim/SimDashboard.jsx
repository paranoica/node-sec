"use client";
import { useEffect, useRef, useState } from "react";
import { Icon, Dropdown } from "../ui";
import { frame, seedTrail, SCENARIOS, SLA, TPS_MAX, P99_MAX } from "../../lib/sim";

const fmtInt = (n) => Math.round(n).toLocaleString("en-US");
const fmtK = (n) => (n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 1 : 1) + "k" : String(Math.round(n)));
const pct = (x) => (x * 100).toFixed(x < 0.1 ? 1 : 0) + "%";

function useReducedMotion() {
  const [r, setR] = useState(false);
  useEffect(() => { setR(matchMedia("(prefers-reduced-motion:reduce)").matches); }, []);
  return r;
}

// ---- the hero: a live p99 × throughput plane with the SLA drawn as a hard L-shaped wall ----
function SlaWall({ f, trail }) {
  const W = 600, H = 340, PADL = 46, PADB = 30, PADT = 16, PADR = 18;
  const pw = W - PADL - PADR, ph = H - PADT - PADB;
  const sx = (tps) => PADL + (Math.min(tps, TPS_MAX) / TPS_MAX) * pw;
  const sy = (ms) => PADT + ph - (Math.min(ms, P99_MAX) / P99_MAX) * ph;
  const cornerX = sx(SLA.tps), cornerY = sy(SLA.p99), bottom = PADT + ph;
  const px = sx(f.admitted), py = sy(f.p99);
  const danger = f.degrade || f.p99 >= SLA.p99;
  const xticks = [5000, 10000, 15000, 20000, 25000];
  const yticks = [10, 20, 30];
  const pts = trail.map((p) => `${sx(p.tps).toFixed(1)},${sy(p.p99).toFixed(1)}`).join(" ");
  return (
    <svg className="plane" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Operating point ${fmtInt(f.admitted)} tx/s at p99 ${f.p99} ms; SLA wall 20k tx/s, 20 ms${danger ? "; degraded" : ""}`}>
      {/* safe zone (inside the wall) */}
      <rect x={PADL} y={cornerY} width={cornerX - PADL} height={bottom - cornerY} className="safe" />
      {/* gridlines + axis labels */}
      {xticks.map((t) => (
        <g key={`x${t}`}>
          <line x1={sx(t)} y1={PADT} x2={sx(t)} y2={bottom} className="grid" />
          <text x={sx(t)} y={H - 9} className="axl" textAnchor="middle">{fmtK(t)}</text>
        </g>
      ))}
      {yticks.map((t) => (
        <g key={`y${t}`}>
          <line x1={PADL} y1={sy(t)} x2={W - PADR} y2={sy(t)} className="grid" />
          <text x={PADL - 8} y={sy(t) + 3} className="axl" textAnchor="end">{t}</text>
        </g>
      ))}
      <text x={14} y={PADT + 4} className="axcap" transform={`rotate(-90 14 ${PADT + 4})`} textAnchor="end">p99 ms</text>
      {/* the SLA wall: top edge (p99=20) + right edge (tps=20k) of the safe box */}
      <path className="wall" d={`M${PADL} ${cornerY} H${cornerX} V${bottom}`} />
      <text x={cornerX - 8} y={cornerY - 7} className="walllbl" textAnchor="end">SLA · p99 &lt; 20ms @ 20k</text>
      {/* live trail + current operating point */}
      <polyline className="trail" points={pts} />
      <circle className={`pt-halo${danger ? " danger" : ""}`} cx={px} cy={py} r="12" />
      <circle className={`pt${danger ? " danger" : ""}`} cx={px} cy={py} r="5.5" />
    </svg>
  );
}

function Vitals({ f }) {
  const near = f.p99 >= SLA.p99 * 0.85;
  return (
    <div className="vitals">
      <div className="vbig">
        <span className="vlabel">throughput</span>
        <span className="vnum mono">{fmtInt(f.admitted)}<i>tx/s</i></span>
        {f.shed > 0 && <span className="vshed mono">shedding {fmtInt(f.shed)}/s</span>}
      </div>
      <div className="lat">
        {[["p50", f.p50], ["p95", f.p95], ["p99", f.p99]].map(([k, v]) => (
          <div className={`latc${k === "p99" ? (f.p99 >= SLA.p99 ? " over" : near ? " near" : "") : ""}`} key={k}>
            <span className="lk">{k}</span><span className="lv mono">{v}<i>ms</i></span>
          </div>
        ))}
      </div>
      <div className="mix">
        <span className="vlabel">decisions</span>
        <div className="mixbar" role="img" aria-label={`approve ${pct(f.mix.approve)}, review ${pct(f.mix.review)}, decline ${pct(f.mix.decline)}`}>
          <i className="seg ap" style={{ width: pct(f.mix.approve) }} />
          <i className="seg rv" style={{ width: pct(f.mix.review) }} />
          <i className="seg dc" style={{ width: pct(f.mix.decline) }} />
        </div>
        <div className="mixleg">
          <span><i className="dot ap" />approve {pct(f.mix.approve)}</span>
          <span><i className="dot rv" />review {pct(f.mix.review)}</span>
          <span><i className="dot dc" />decline {pct(f.mix.decline)}</span>
        </div>
      </div>
      <div className={`degrade${f.degrade ? " on" : ""}`}>
        <span className="ddot" />
        {f.degrade ? <span><b>DEGRADE</b> · load-shedding to hold SLA</span> : <span><b>NOMINAL</b> · within SLA, no shedding</span>}
      </div>
    </div>
  );
}

function Pipeline({ f }) {
  return (
    <div className="pipeline" aria-label="Decision pipeline throughput by stage">
      {f.stages.map((s, i) => (
        <div className="pstage-wrap" key={s.stage}>
          <div className={`pstage${s.queue ? " jam" : ""}`}>
            <span className="psname">{s.stage}</span>
            <span className="pstps mono">{fmtInt(s.tps)}<i>/s</i></span>
            <span className="psq">{s.queue ? <><span className="qdot" />queue +{fmtInt(s.queue)}/s</> : "clear"}</span>
          </div>
          {i < f.stages.length - 1 && <span className="parrow" aria-hidden="true"><Icon name="chev" /></span>}
        </div>
      ))}
    </div>
  );
}

export function SimDashboard() {
  const reduced = useReducedMotion();
  const [ctl, setCtl] = useState({ running: true, targetTps: 12000, scenario: "baseline", fault: false });
  const [f, setF] = useState(() => frame({ running: true, targetTps: 12000, scenario: "baseline", fault: false }, 0));
  const [trail, setTrail] = useState(() => seedTrail({ running: true, targetTps: 12000, scenario: "baseline", fault: false }));
  const [elapsed, setElapsed] = useState(0);
  const ctlRef = useRef(ctl);
  ctlRef.current = ctl;

  // live telemetry tick — synthesises the SSE frame stream
  useEffect(() => {
    if (reduced) return;
    const iv = setInterval(() => {
      if (document.hidden) return;
      const c = ctlRef.current;
      const jit = Math.random() * 2 - 1;
      const nf = frame(c, jit);
      setF(nf);
      if (c.running) {
        setTrail((prev) => [...prev, { tps: nf.admitted, p99: nf.p99 }].slice(-48));
        setElapsed((e) => e + 1);
      }
    }, 820);
    return () => clearInterval(iv);
  }, [reduced]);

  // controls change → reflect immediately (don't wait for the next tick)
  useEffect(() => { setF(frame(ctl, 0)); }, [ctl]);

  const set = (patch) => setCtl((c) => ({ ...c, ...patch }));
  const status = !ctl.running ? "down" : f.degrade ? "connecting" : "live";
  const elapsedStr = `${String(Math.floor(elapsed / 60)).padStart(2, "0")}:${String(elapsed % 60).padStart(2, "0")}`;

  return (
    <div className="app sim">
      <header className="topbar">
        <div className="brand">
          <span className={`status ${status}`} role="img" aria-label={ctl.running ? (f.degrade ? "degraded" : "running") : "paused"} />
          <b>node-sec</b><span className="sub">simulation control</span>
        </div>
        <div className="spacer" />
        <div className="stat"><b className="mono">{elapsedStr}</b><small>run time</small></div>
      </header>

      <div className="simbar">
        <button className={`btn ${ctl.running ? "" : "primary"}`} onClick={() => set({ running: !ctl.running })} aria-pressed={ctl.running}>
          <Icon name={ctl.running ? "pause" : "play"} />{ctl.running ? "Pause" : "Run"}
        </button>
        <Dropdown label="Traffic scenario" value={ctl.scenario} options={SCENARIOS} onChange={(v) => set({ scenario: v })} />
        <button className={`btn${ctl.fault ? " on danger" : ""}`} onClick={() => set({ fault: !ctl.fault })} aria-pressed={ctl.fault}>
          <Icon name="zap" />Downstream fault
        </button>
        <div className="tpsctl">
          <label htmlFor="tps">target load</label>
          <input id="tps" className="range" type="range" min="0" max={TPS_MAX} step="500"
            value={ctl.targetTps} onChange={(e) => set({ targetTps: +e.target.value })}
            style={{ "--fill": `${(ctl.targetTps / TPS_MAX) * 100}%` }}
            aria-label="Target throughput tx/s" />
          <span className="tpsval mono">{fmtK(ctl.targetTps)}<i>tx/s</i></span>
        </div>
      </div>

      <div className="simmain">
        <section className="hero" aria-label="SLA headroom — p99 latency vs throughput">
          <div className="herohd">
            <h2>SLA wall</h2>
            <div className="headroom mono">
              <span className={f.p99 >= SLA.p99 ? "hr over" : "hr"}>{(SLA.p99 - f.p99).toFixed(1)}<i>ms</i></span>
              <span className={f.admitted >= f.wallTps ? "hr over" : "hr"}>{fmtK(Math.max(0, f.wallTps - f.admitted))}<i>tx/s</i></span>
              <small>headroom to the wall</small>
            </div>
          </div>
          <SlaWall f={f} trail={trail} />
        </section>
        <aside className="side" aria-label="Live vitals"><Vitals f={f} /></aside>
      </div>

      <section className="pipewrap" aria-label="Decision pipeline">
        <h3>Decision pipeline <span className="phint">· per-stage throughput &amp; back-pressure</span></h3>
        <Pipeline f={f} />
      </section>
    </div>
  );
}
