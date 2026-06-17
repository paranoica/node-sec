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
// The operating point + its trail are driven by requestAnimationFrame (imperative SVG attrs, not
// React state) so the dot eases toward each new target and the comet-tail line is laid down in real
// time as it moves — never a teleport that the trail only catches up to after it lands.
function SlaWall({ f, seed, reduced }) {
  const W = 600, H = 340, PADB = 30, PADT = 16, PADL = 46, PADR = 18;
  const pw = W - PADL - PADR, ph = H - PADT - PADB;
  const sx = (tps) => PADL + (Math.min(tps, TPS_MAX) / TPS_MAX) * pw;
  const sy = (ms) => PADT + ph - (Math.min(ms, P99_MAX) / P99_MAX) * ph;
  const cornerX = sx(SLA.tps), cornerY = sy(SLA.p99), bottom = PADT + ph;
  const xticks = [5000, 10000, 15000, 20000, 25000];
  const yticks = [10, 20, 30];

  const dotRef = useRef(null), haloRef = useRef(null), trailRef = useRef(null);
  const pos = useRef({ tps: f.admitted, p99: f.p99 });
  const pts = useRef(seed.map((p) => ({ tps: p.tps, p99: p.p99 })));
  const target = useRef({ tps: f.admitted, p99: f.p99, danger: false });
  useEffect(() => { target.current = { tps: f.admitted, p99: f.p99, danger: f.degrade || f.p99 >= SLA.p99 }; }, [f]);

  useEffect(() => {
    if (reduced) return;
    let raf;
    const render = () => {
      const t = target.current, p = pos.current;
      p.tps += (t.tps - p.tps) * 0.16; // critically-damped ease toward the live target
      p.p99 += (t.p99 - p.p99) * 0.16;
      const px = sx(p.tps), py = sy(p.p99);
      const arr = pts.current, last = arr[arr.length - 1];
      if (!last || Math.abs(sx(last.tps) - px) > 0.5 || Math.abs(sy(last.p99) - py) > 0.5) {
        arr.push({ tps: p.tps, p99: p.p99 });
        if (arr.length > 220) arr.splice(0, arr.length - 220);
      }
      trailRef.current?.setAttribute("points", arr.map((q) => `${sx(q.tps).toFixed(1)},${sy(q.p99).toFixed(1)}`).join(" "));
      for (const r of [dotRef.current, haloRef.current]) {
        if (!r) continue;
        r.setAttribute("cx", px.toFixed(1)); r.setAttribute("cy", py.toFixed(1));
        r.classList.toggle("danger", t.danger);
      }
      raf = requestAnimationFrame(render);
    };
    raf = requestAnimationFrame(render);
    return () => cancelAnimationFrame(raf);
  }, [reduced]);

  const danger0 = f.degrade || f.p99 >= SLA.p99;
  const px0 = sx(f.admitted), py0 = sy(f.p99);
  const initPts = pts.current.map((q) => `${sx(q.tps).toFixed(1)},${sy(q.p99).toFixed(1)}`).join(" ");
  return (
    <svg className="plane" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Operating point ${fmtInt(f.admitted)} tx/s at p99 ${f.p99} ms; SLA wall 20k tx/s, 20 ms${danger0 ? "; degraded" : ""}`}>
      <rect x={PADL} y={cornerY} width={cornerX - PADL} height={bottom - cornerY} className="safe" />
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
      <path className="wall" d={`M${PADL} ${cornerY} H${cornerX} V${bottom}`} />
      <text x={cornerX - 8} y={cornerY - 7} className="walllbl" textAnchor="end">SLA · p99 &lt; 20ms @ 20k</text>
      <polyline ref={trailRef} className="trail" points={initPts} />
      <circle ref={haloRef} className={`pt-halo${danger0 ? " danger" : ""}`} cx={px0} cy={py0} r="12" />
      <circle ref={dotRef} className={`pt${danger0 ? " danger" : ""}`} cx={px0} cy={py0} r="5.5" />
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
  const INIT = { running: true, targetTps: 12000, scenario: "baseline", fault: false };
  const [ctl, setCtl] = useState(INIT);
  const [f, setF] = useState(() => frame(INIT, 0));
  const [elapsed, setElapsed] = useState(0);
  const seedRef = useRef(seedTrail(INIT));
  const ctlRef = useRef(ctl);
  ctlRef.current = ctl;

  // live telemetry tick — synthesises the SSE frame stream (the trail/point is rAF-driven in SlaWall)
  useEffect(() => {
    if (reduced) return;
    const iv = setInterval(() => {
      if (document.hidden) return;
      const c = ctlRef.current;
      const nf = frame(c, Math.random() * 2 - 1);
      setF(nf);
      if (c.running) setElapsed((e) => e + 1);
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
          <SlaWall f={f} seed={seedRef.current} reduced={reduced} />
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
