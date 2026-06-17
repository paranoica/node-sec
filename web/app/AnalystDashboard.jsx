"use client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { initialQueue, makeCase, byRisk, tierOf } from "../lib/data";

function useReducedMotion() {
  const [r, setR] = useState(false);
  useEffect(() => { setR(matchMedia("(prefers-reduced-motion:reduce)").matches); }, []);
  return r;
}

function CountUp({ value, reduced }) {
  const [shown, setShown] = useState(reduced ? value : 0);
  useEffect(() => {
    if (reduced) { setShown(value); return; }
    let raf, t0;
    const step = (now) => {
      t0 ??= now;
      const p = Math.min(1, (now - t0) / 540);
      setShown(value * (1 - Math.pow(1 - p, 3)));
      if (p < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [value, reduced]);
  return <span className="bignum mono">{shown.toFixed(2)}</span>;
}

function Icon({ name }) {
  const p = { fill: "none", stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round", strokeLinejoin: "round" };
  if (name === "moon") return <svg viewBox="0 0 24 24" {...p}><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z" /></svg>;
  if (name === "sun") return <svg viewBox="0 0 24 24" {...p}><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></svg>;
  if (name === "chev") return <svg viewBox="0 0 24 24" {...p} strokeWidth="2"><path d="M9 6l6 6-6 6" /></svg>;
  if (name === "close") return <svg viewBox="0 0 24 24" {...p}><path d="M6 6l12 12M18 6L6 18" /></svg>;
  return <svg viewBox="0 0 24 24" {...p} strokeWidth="1.4"><path d="M3 5h18M3 12h18M3 19h10" /></svg>;
}

function Row({ c, selected, fresh, onSelect, refCb }) {
  const tier = tierOf(c.risk);
  return (
    <li
      ref={refCb}
      role="option"
      tabIndex={selected ? 0 : -1}
      data-id={c.case_id}
      data-tier={tier.id}
      aria-selected={selected}
      aria-label={`${c.subject}, risk ${c.risk.toFixed(2)}, ${tier.label}, ${c.alerts.length} alerts`}
      className={`row mono${fresh ? " fresh" : ""}`}
      style={{ "--risk": c.risk }}
      onClick={() => onSelect(c.case_id, true)}
    >
      <span className="lane" aria-hidden="true">
        <span className="meter"><span className="fill" /><span className="knob" /></span>
        <span className="num">{c.risk.toFixed(2)}</span>
      </span>
      <span className="subject">
        <span className="sid">{c.subject}</span>
        <span className="meta">{c.case_id} · {c.state}</span>
      </span>
      <span className="tier"><b>{tier.label}</b></span>
      <span className={`acount${c.alerts.length ? "" : " zero"}`}>{c.alerts.length || "—"}</span>
      <span className="chev" aria-hidden="true"><Icon name="chev" /></span>
    </li>
  );
}

function Detail({ c, reduced }) {
  if (!c) {
    return (
      <div className="detail-empty">
        <Icon name="list" />
        <div>Select a case to review its alerts, evidence and links.</div>
      </div>
    );
  }
  const tier = tierOf(c.risk);
  const Empty = ({ children }) => <span className="none">{children}</span>;
  return (
    <div className="dpanel" key={c.case_id}>
      <div className="dhead">
        <div className="riskblock">
          <div className="risklabel">RISK</div>
          <CountUp value={c.risk} reduced={reduced} />
          <div className="risktier" data-tier={tier.id}>{tier.label}</div>
        </div>
        <div className="who">
          <div className="case mono">{c.case_id}</div>
          <div className="subj">{c.subject}</div>
          <span className="statepill"><span className="sq" aria-hidden="true" />{c.state}</span>
        </div>
      </div>
      <div className="sect"><h3>Alerts</h3>
        {c.alerts.length
          ? <div className="chips">{c.alerts.map((a) => <span className="chip alert" key={a}>{a}</span>)}</div>
          : <Empty>No alerts on this case.</Empty>}
      </div>
      <div className="sect"><h3>Evidence</h3>
        {c.evidence.length
          ? c.evidence.map((e, i) => <div className="ev" key={i}><span className="k">{e.kind}</span><span className="d">{e.detail}</span></div>)
          : <Empty>No supporting evidence recorded.</Empty>}
      </div>
      <div className="sect"><h3>Reason codes</h3>
        {c.reason_codes.length
          ? <div className="codes">{c.reason_codes.map((x) => <span className="code mono" key={x}>{x}</span>)}</div>
          : <Empty>No model reason codes.</Empty>}
      </div>
      <div className="sect"><h3>Graph links</h3>
        {c.graph_links.length
          ? c.graph_links.map((g, i) => (
              <div className="glink" key={i}>
                <span className="rel">{g.relation}</span>
                <span className="cp mono">{g.counterparty}</span>
                <span className="wbar" style={{ "--w": g.weight }}><i /></span>
                <span className="w mono">{g.weight.toFixed(2)}</span>
              </div>
            ))
          : <Empty>No connected entities.</Empty>}
      </div>
    </div>
  );
}

export function AnalystDashboard() {
  const reduced = useReducedMotion();
  const [loading, setLoading] = useState(true);
  const [cases, setCases] = useState([]);
  const [selectedId, setSelectedId] = useState(null);
  const [fresh, setFresh] = useState(() => new Set());
  const [drawerOpen, setDrawerOpen] = useState(false);
  // connection status — drives the brand dot. Wired to the WS/SSE readyState in production; the
  // mock stream runs, so it stays "live". (live | connecting | down)
  const [status] = useState("live");
  const rowRefs = useRef(new Map());

  // initial "load" → skeleton, then data
  useEffect(() => {
    const q = initialQueue(22);
    const t = setTimeout(() => { setCases(q); setSelectedId(q[0].case_id); setLoading(false); }, reduced ? 0 : 620);
    return () => clearTimeout(t);
  }, [reduced]);

  // live: a fresh case streams onto the spine; the keyed list reconciles (no flicker, scroll kept)
  useEffect(() => {
    if (reduced || loading) return;
    const iv = setInterval(() => {
      if (document.hidden) return;
      const c = makeCase();
      setCases((prev) => [...prev, c].sort(byRisk).slice(0, 44));
      setFresh((prev) => new Set(prev).add(c.case_id));
      setTimeout(() => setFresh((prev) => { const n = new Set(prev); n.delete(c.case_id); return n; }), 1700);
    }, 4600);
    return () => clearInterval(iv);
  }, [reduced, loading]);

  const select = useCallback((id, open) => {
    setSelectedId(id);
    if (open && matchMedia("(max-width:860px)").matches) setDrawerOpen(true);
  }, []);

  const onKeyDown = useCallback((e) => {
    if (!["ArrowDown", "ArrowUp"].includes(e.key)) return;
    e.preventDefault();
    const i = cases.findIndex((c) => c.case_id === selectedId);
    const ni = e.key === "ArrowDown" ? Math.min(cases.length - 1, i + 1) : Math.max(0, i < 0 ? 0 : i - 1);
    const id = cases[ni]?.case_id;
    if (id) { setSelectedId(id); rowRefs.current.get(id)?.focus(); }
  }, [cases, selectedId]);

  const STATUS = { live: "Live · streaming", connecting: "Connecting…", down: "Disconnected" };
  const selected = useMemo(() => cases.find((c) => c.case_id === selectedId) || null, [cases, selectedId]);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className={`status ${status}`} role="img" aria-label={STATUS[status]} title={STATUS[status]} />
          <b>node-sec</b><span className="sub">analyst console</span>
        </div>
        <div className="spacer" />
        <div className="stat"><b className="mono">{cases.length}</b><small>open cases</small></div>
      </header>

      <div className="main">
        <section className="queue" aria-label="Risk-prioritised review queue">
          <div className="qhead">
            <div className="lane-head"><span className="cap">Risk</span></div>
            <div className="col">Subject</div>
            <div className="col s">Tier</div>
            <div className="col n">Alerts</div>
            <div style={{ flex: "none", width: 18 }} />
          </div>

          <div className="listwrap">
            {loading ? (
              <div>{Array.from({ length: 9 }).map((_, i) => (
                <div className="sk" key={i}><i style={{ width: "62%" }} /><i style={{ width: "16%", marginLeft: "auto" }} /></div>
              ))}</div>
            ) : cases.length ? (
              <ul className="list" role="listbox" aria-label="Cases by risk" tabIndex={0} onKeyDown={onKeyDown}>
                {cases.map((c) => (
                  <Row
                    key={c.case_id}
                    c={c}
                    selected={c.case_id === selectedId}
                    fresh={fresh.has(c.case_id)}
                    onSelect={select}
                    refCb={(el) => { if (el) rowRefs.current.set(c.case_id, el); else rowRefs.current.delete(c.case_id); }}
                  />
                ))}
              </ul>
            ) : (
              <div className="queue-empty"><div>No open cases.</div><div style={{ fontSize: 11 }}>The queue is clear — new alerts appear here.</div></div>
            )}
          </div>
        </section>

        <aside className={`detail${drawerOpen ? " open" : ""}`} aria-label="Case detail">
          <button className="iconbtn dclose" onClick={() => setDrawerOpen(false)} aria-label="Close case detail"><Icon name="close" /></button>
          <Detail c={selected} reduced={reduced} />
        </aside>
        <div className={`scrim${drawerOpen ? " open" : ""}`} onClick={() => setDrawerOpen(false)} />
      </div>
    </div>
  );
}
