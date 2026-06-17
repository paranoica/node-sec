"use client";
// Shared UI primitives for both dashboards (analyst + simulation): one Icon set + one custom
// Dropdown, so the two surfaces never drift apart.
import { useEffect, useRef, useState } from "react";

export function Icon({ name }) {
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
  if (name === "play") return <svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z" /></svg>;
  if (name === "pause") return <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4.2" height="14" rx="1.2" /><rect x="13.8" y="5" width="4.2" height="14" rx="1.2" /></svg>;
  if (name === "activity") return <svg viewBox="0 0 24 24" {...p}><path d="M3 12h4l3 8 4-16 3 8h4" /></svg>;
  if (name === "zap") return <svg viewBox="0 0 24 24" {...p}><path d="M13 2L4 14h7l-1 8 9-12h-7z" /></svg>;
  if (name === "reset") return <svg viewBox="0 0 24 24" {...p}><path d="M4 12a8 8 0 1 0 2.5-5.8M4 4v4h4" /></svg>;
  return <svg viewBox="0 0 24 24" {...p} strokeWidth="1.4"><path d="M3 5h18M3 12h18M3 19h10" /></svg>;
}

// Fully custom dropdown — a native <select> can't style its open option list (the OS draws it),
// so we render our own listbox. Closes on outside-click / Escape; arrow-key navigable.
export function Dropdown({ value, options, onChange, label }) {
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
