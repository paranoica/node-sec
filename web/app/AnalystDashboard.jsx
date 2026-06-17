"use client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { initialQueue, makeCase, byRisk, tierOf, catOf, CATS, fmtMoney } from "../lib/data";

const fmtAgo = (m) => (m < 60 ? `${m}m` : `${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`);

const STATUS_FILTERS = [
  { id: "all", label: "All" },
  { id: "mine", label: "Mine" },
  { id: "open", label: "Open" },
  { id: "closed", label: "Closed" },
];

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
  if (name === "chev") return <svg viewBox="0 0 24 24" {...p} strokeWidth="2"><path d="M9 6l6 6-6 6" /></svg>;
  if (name === "close") return <svg viewBox="0 0 24 24" {...p}><path d="M6 6l12 12M18 6L6 18" /></svg>;
  if (name === "search") return <svg viewBox="0 0 24 24" {...p}><circle cx="11" cy="11" r="7" /><path d="M21 21l-4.3-4.3" /></svg>;
  if (name === "flag") return <svg viewBox="0 0 24 24" {...p}><path d="M5 21V4M5 5h12l-2.2 4 2.2 4H5" /></svg>;
  if (name === "check") return <svg viewBox="0 0 24 24" {...p}><path d="M4 12.5l5 5L20 6.5" /></svg>;
  if (name === "up") return <svg viewBox="0 0 24 24" {...p}><path d="M12 19V6M6 11l6-6 6 6" /></svg>;
  if (name === "ban") return <svg viewBox="0 0 24 24" {...p}><circle cx="12" cy="12" r="8.5" /><path d="M6 6l12 12" /></svg>;
  if (name === "user") return <svg viewBox="0 0 24 24" {...p}><circle cx="12" cy="8" r="3.5" /><path d="M5 20a7 7 0 0 1 14 0" /></svg>;
  if (name === "pen") return <svg viewBox="0 0 24 24" {...p}><path d="M4 20h4L19 9l-4-4L4 16z" /></svg>;
  if (name === "shield") return <svg viewBox="0 0 24 24" {...p}><path d="M12 3l7 3v5c0 4-3 7-7 8-4-1-7-4-7-8V6z" /><path d="M9 12l2 2 4-4" /></svg>;
  return <svg viewBox="0 0 24 24" {...p} strokeWidth="1.4"><path d="M3 5h18M3 12h18M3 19h10" /></svg>;
}

// Fully custom dropdown — a native <select> can't style its open option list (the OS draws it),
// so we render our own listbox. Closes on outside-click / Escape; arrow-key navigable.
function Dropdown({ value, options, onChange, label }) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return;
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open, options, value]);
  const cur = options.find((o) => o.value === value) || options[0];
  const choose = (v) => { onChange(v); setOpen(false); };
  const onKey = (e) => {
    if (e.key === "Escape") { setOpen(false); return; }
    if (e.key === "ArrowDown") { e.preventDefault(); if (!open) setOpen(true); else setActive((a) => Math.min(options.length - 1, a + 1)); return; }
    if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(0, a - 1)); return; }
    if (e.key === "Enter" || e.key === " ") { e.preventDefault(); if (open) choose(options[active].value); else setOpen(true); }
  };
  return (
    <div className={`dropdown${open ? " open" : ""}`} ref={ref}>
      <button type="button" className="ddbtn" aria-haspopup="listbox" aria-expanded={open} aria-label={label}
        onClick={() => setOpen((o) => !o)} onKeyDown={onKey}>
        <span>{cur.label}</span>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg>
      </button>
      {open && (
        <ul className="ddlist" role="listbox" aria-label={label}>
          {options.map((o, i) => (
            <li key={o.value} role="option" aria-selected={o.value === value}
              className={`ddopt${o.value === value ? " sel" : ""}${i === active ? " active" : ""}`}
              onMouseEnter={() => setActive(i)} onClick={() => choose(o.value)}>
              <span>{o.label}</span>{o.value === value && <Icon name="check" />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Row({ c, selected, fresh, onSelect, refCb }) {
  const tier = tierOf(c.risk);
  const stateLabel = c.disposition ? c.disposition : c.owner === "you" ? "in review" : c.state;
  return (
    <li
      ref={refCb}
      role="option"
      tabIndex={selected ? 0 : -1}
      data-id={c.case_id}
      data-tier={tier.id}
      aria-selected={selected}
      aria-label={`${c.subject}, risk ${c.risk.toFixed(2)}, ${tier.label}, ${c.alerts.length} alerts, ${stateLabel}`}
      className={`row mono${fresh ? " fresh" : ""}${c.disposition ? " closed" : ""}`}
      style={{ "--risk": c.risk }}
      onClick={() => onSelect(c.case_id, true)}
    >
      <span className="lane" aria-hidden="true">
        <span className="meter"><span className="fill" /><span className="knob" /></span>
        <span className="num">{c.risk.toFixed(2)}</span>
      </span>
      <span className="subject">
        <span className="sid">{c.subject}{c.owner === "you" && <i className="owntick" title="owned by you" />}</span>
        <span className="meta">{c.case_id} · {stateLabel}</span>
      </span>
      <span className="tier"><b>{tier.label}</b></span>
      <span className={`acount${c.alerts.length ? "" : " zero"}`}>{c.alerts.length || "—"}</span>
      <span className="chev" aria-hidden="true"><Icon name="chev" /></span>
    </li>
  );
}

function Banner({ c }) {
  // disposition and a pending four-eyes can coexist (e.g. fraud confirmed, SAR awaiting a reviewer)
  return (
    <>
      {c.disposition === "fraud" &&
        <div className="banner fraud"><Icon name="flag" /><b>Confirmed fraud</b><span>signed off by you + reviewer (four-eyes){c.sarFiled ? " · SAR filed" : ""}{c.blocked ? " · card blocked" : ""}</span></div>}
      {c.disposition === "clear" &&
        <div className="banner clear"><Icon name="check" /><b>Cleared — false positive</b><span>fed to the model FP-feedback loop</span></div>}
      {c.disposition === "escalated" &&
        <div className="banner esc"><Icon name="up" /><b>Escalated to compliance</b><span>handed to the senior fraud / AML desk</span></div>}
      {c.pending &&
        <div className="banner pend"><Icon name="shield" /><b>Pending four-eyes</b><span>{c.pending.action === "sar" ? "SAR" : "fraud decision"} submitted by you — awaiting a second analyst</span></div>}
      {!c.disposition && !c.pending && c.owner === "you" &&
        <div className="ownerchip"><Icon name="user" />owned by you</div>}
    </>
  );
}

function Actions({ c, act }) {
  if (c.pending) {
    return (
      <div className="dactions">
        <button className="btn primary" onClick={() => act.approve(c.case_id)}><Icon name="shield" />Approve as reviewer</button>
        <button className="btn ghost" onClick={() => act.cancel(c.case_id)}>Cancel</button>
        <p className="foureyes">You submitted this — four-eyes requires a <b>different</b> approver; the server rejects self-approval.</p>
      </div>
    );
  }
  if (c.disposition) {
    return (
      <div className="dactions">
        {c.disposition === "fraud" && !c.sarFiled && <button className="btn primary" onClick={() => act.fileSar(c.case_id)}><Icon name="flag" />File SAR</button>}
        <button className="btn ghost" onClick={() => act.reopen(c.case_id)}>Reopen case</button>
      </div>
    );
  }
  return (
    <div className="dactions">
      {c.owner !== "you" && <button className="btn assign" onClick={() => act.assign(c.case_id)}><Icon name="user" />Assign to me</button>}
      <button className="btn danger" onClick={() => act.confirmFraud(c.case_id)}><Icon name="flag" />Confirm fraud</button>
      <button className="btn" onClick={() => act.clear(c.case_id)}><Icon name="check" />Clear</button>
      <button className="btn" onClick={() => act.escalate(c.case_id)}><Icon name="up" />Escalate</button>
      {c.subject.startsWith("card") && (
        <button className={`btn${c.blocked ? " on" : ""}`} onClick={() => act.block(c.case_id)}><Icon name="ban" />{c.blocked ? "Unblock" : "Block card"}</button>
      )}
    </div>
  );
}

function Notes({ c, onAdd }) {
  const [text, setText] = useState("");
  const submit = (e) => {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    onAdd(c.case_id, t);
    setText("");
  };
  return (
    <div className="sect">
      <h3>Notes <span className="audit">· immutable audit</span></h3>
      {c.notes.length
        ? c.notes.map((nn, i) => (
            <div className={`note${nn.by === "system" ? " sys" : ""}`} key={i}>
              <span className="nmeta">{nn.by} · {nn.t}</span>
              <span className="ntext">{nn.text}</span>
            </div>
          ))
        : <span className="none">No notes yet.</span>}
      <form className="noteform" onSubmit={submit}>
        <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Add an investigation note…" aria-label="Add an investigation note" />
        <button className="btn sm" type="submit" disabled={!text.trim()}><Icon name="pen" />Add</button>
      </form>
    </div>
  );
}

function Detail({ c, reduced, act }) {
  if (!c) {
    return (
      <div className="detail-empty">
        <Icon name="list" />
        <div>Select a case to review its alerts, evidence and links — then take a disposition.</div>
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
          <div className="risktier">{tier.label}</div>
        </div>
        <div className="who">
          <div className="case mono">{c.case_id}</div>
          <div className="subj">{c.subject}</div>
          <span className="statepill"><span className="sq" aria-hidden="true" />{c.state}</span>
        </div>
      </div>

      <Banner c={c} />

      <div className="sect"><h3>Transactions</h3>
        {c.txns.length
          ? <div className="txns">{c.txns.map((t, i) => (
              <div className={`txn${t.flag ? " flag" : ""}`} key={i}>
                <span className="tamt mono">{fmtMoney(t.amount)}</span>
                <span className="tmeta">
                  <span className="tm">{t.merchant}</span>
                  <span className="tsub">{t.mcc} · {t.channel}{t.avs !== "—" ? ` · AVS ${t.avs} CVV ${t.cvv}` : ""}</span>
                </span>
                <span className={`tauth ${t.auth}`}>{t.auth}</span>
                <span className="tago mono">{fmtAgo(t.mins)}</span>
              </div>
            ))}</div>
          : <Empty>No transactions on this case.</Empty>}
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

      <Notes c={c} onAdd={act.addNote} />
      <Actions c={c} act={act} />
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
  const [filter, setFilter] = useState({ status: "all", cat: "all", q: "" });
  // connection status — drives the brand dot. Wired to the WS/SSE readyState in production; the
  // mock stream runs, so it stays "live". (live | connecting | down)
  const [status] = useState("live");
  const rowRefs = useRef(new Map());

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

  // single mutation seam — every action patches one case by id; production POSTs to /cases/:id/*
  const patch = useCallback((id, fn) => {
    setCases((prev) => prev.map((c) => (c.case_id === id ? { ...c, ...fn(c) } : c)));
  }, []);
  const sysNote = (text) => ({ by: "system", t: "just now", text });

  const act = useMemo(() => ({
    assign: (id) => patch(id, (c) => ({ owner: "you", state: c.state === "alert" ? "investigate" : c.state })),
    confirmFraud: (id) => patch(id, (c) => ({ owner: c.owner || "you", pending: { action: "fraud", by: "you" } })),
    fileSar: (id) => patch(id, () => ({ pending: { action: "sar", by: "you" } })),
    approve: (id) => patch(id, (c) => {
      if (!c.pending) return {};
      if (c.pending.action === "fraud") return { disposition: "fraud", pending: null, notes: [...c.notes, sysNote("Confirmed fraud — approved by reviewer (four-eyes)")] };
      return { sarFiled: true, pending: null, notes: [...c.notes, sysNote("SAR filed — approved by reviewer (four-eyes)")] };
    }),
    cancel: (id) => patch(id, () => ({ pending: null })),
    clear: (id) => patch(id, (c) => ({ owner: c.owner || "you", disposition: "clear", pending: null, notes: [...c.notes, sysNote("Cleared as false positive — fed to model FP feedback")] })),
    escalate: (id) => patch(id, (c) => ({ owner: c.owner || "you", disposition: "escalated", pending: null, notes: [...c.notes, sysNote("Escalated to compliance")] })),
    block: (id) => patch(id, (c) => ({ blocked: !c.blocked, notes: [...c.notes, sysNote(c.blocked ? "Card unblocked" : "Card blocked / funds held")] })),
    reopen: (id) => patch(id, () => ({ disposition: null, pending: null })),
    addNote: (id, text) => patch(id, (c) => ({ notes: [...c.notes, { by: "you", t: "just now", text }] })),
  }), [patch]);

  const select = useCallback((id, open) => {
    setSelectedId(id);
    if (open && matchMedia("(max-width:860px)").matches) setDrawerOpen(true);
  }, []);

  const visible = useMemo(() => cases.filter((c) => {
    if (filter.status === "mine" && c.owner !== "you") return false;
    if (filter.status === "open" && c.disposition) return false;
    if (filter.status === "closed" && !c.disposition) return false;
    if (filter.cat !== "all" && !c.alerts.some((a) => catOf(a) === filter.cat)) return false;
    if (filter.q) {
      const q = filter.q.toLowerCase();
      if (!c.subject.toLowerCase().includes(q) && !c.case_id.toLowerCase().includes(q)) return false;
    }
    return true;
  }), [cases, filter]);

  const onKeyDown = useCallback((e) => {
    if (!["ArrowDown", "ArrowUp"].includes(e.key)) return;
    e.preventDefault();
    const i = visible.findIndex((c) => c.case_id === selectedId);
    const ni = e.key === "ArrowDown" ? Math.min(visible.length - 1, i + 1) : Math.max(0, i < 0 ? 0 : i - 1);
    const id = visible[ni]?.case_id;
    if (id) { setSelectedId(id); rowRefs.current.get(id)?.focus(); }
  }, [visible, selectedId]);

  const STATUS = { live: "Live · streaming", connecting: "Connecting…", down: "Disconnected" };
  const selected = useMemo(() => cases.find((c) => c.case_id === selectedId) || null, [cases, selectedId]);
  const openCount = useMemo(() => cases.filter((c) => !c.disposition).length, [cases]);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className={`status ${status}`} role="img" aria-label={STATUS[status]} title={STATUS[status]} />
          <b>node-sec</b><span className="sub">analyst console</span>
        </div>
        <div className="spacer" />
        <div className="stat"><b className="mono">{openCount}</b><small>open cases</small></div>
      </header>

      <div className="main">
        <section className="queue" aria-label="Risk-prioritised review queue">
          <div className="filterbar">
            <div className="seg" role="group" aria-label="Filter by status">
              {STATUS_FILTERS.map((s) => (
                <button key={s.id} className={filter.status === s.id ? "on" : ""} aria-pressed={filter.status === s.id}
                  onClick={() => setFilter((f) => ({ ...f, status: s.id }))}>{s.label}</button>
              ))}
            </div>
            <div className="fspacer" />
            <label className="fsearch">
              <Icon name="search" />
              <input value={filter.q} onChange={(e) => setFilter((f) => ({ ...f, q: e.target.value }))}
                placeholder="subject / case-id" aria-label="Search cases" />
            </label>
            <Dropdown label="Filter by alert type" value={filter.cat}
              options={[{ value: "all", label: "All alerts" }, ...CATS.map((c) => ({ value: c, label: c.toUpperCase() }))]}
              onChange={(v) => setFilter((f) => ({ ...f, cat: v }))} />
          </div>

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
            ) : visible.length ? (
              <ul className="list" role="listbox" aria-label="Cases by risk" tabIndex={0} onKeyDown={onKeyDown}>
                {visible.map((c) => (
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
              <div className="queue-empty"><div>No cases match.</div><div style={{ fontSize: 11 }}>Adjust the status, alert-type or search filter.</div></div>
            )}
          </div>
        </section>

        <aside className={`detail${drawerOpen ? " open" : ""}`} aria-label="Case detail">
          <button className="iconbtn dclose" onClick={() => setDrawerOpen(false)} aria-label="Close case detail"><Icon name="close" /></button>
          <Detail c={selected} reduced={reduced} act={act} />
        </aside>
        <div className={`scrim${drawerOpen ? " open" : ""}`} onClick={() => setDrawerOpen(false)} />
      </div>
    </div>
  );
}
