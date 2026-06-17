// Mock GET /queue — the live CaseView shape (crates/api/src/analyst). Swapping to the real endpoint
// is a one-line fetch: `await (await fetch('/queue')).json()`.

// card:… is PCI-masked (BIN + last4, middle hidden) — required, never unmasked in a list view.
// wallet:… uses the standard head…tail address truncation (NOT privacy masking) — last4 shown for
// reconciliation, full address belongs in the audited detail view. acct-… are internal case refs.
const SUBJ = ["acct-9241","acct-3387","acct-7710","card:4012•••5521","acct-5560","acct-1108","acct-8834","wallet:0x71c3•••e08b","acct-2245","acct-6093","acct-4471","card:5391•••0042","acct-9980","acct-3019","acct-7741"];
const ALERTS = ["aml:structuring","sanctions:near-match","rules:velocity","rules:amount-anomaly","aml:funnel","p2p:app-fraud","crypto:taint","mcc:high-risk","aml:round-tripping"];
const CODES = ["R02_VELOCITY","R07_GEO_RISK","R11_AMOUNT","R19_DEVICE","MODEL_HIGH_VELOCITY_5M","CRYPTO_SANCTIONS","P2P_APP_NEW_PAYEE"];
const RELS = ["funds-to","shared-device","funds-from","co-signer"];
const STATES = ["alert","triage","investigate"];
const KINDS = ["velocity","device","sanctions","geo","amount"];
const DETAILS = ["7 sub-CTR deposits in 24h","new device, first seen 4m ago","name 0.91 to an OFAC SDN","impossible travel NZ → ES","8× the card's mean ticket","3 distinct PANs on one BIN"];

// deterministic PRNG so the prototype renders identically each load (stable QA screenshots)
let seed = 7;
const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
const pick = (a) => a[Math.floor(rnd() * a.length)];
const uniq = (a) => [...new Set(a)];

let n = 0;
export function makeCase(risk) {
  risk = risk ?? +(0.05 + rnd() * 0.93).toFixed(2);
  const id = 1000 + n++;
  return {
    case_id: "case-" + id,
    subject: pick(SUBJ),
    risk,
    state: pick(STATES),
    alerts: uniq(Array.from({ length: Math.floor(rnd() * 4) }, () => pick(ALERTS))),
    evidence: Array.from({ length: Math.floor(rnd() * 3) }, () => ({ kind: pick(KINDS), detail: pick(DETAILS) })),
    reason_codes: uniq(Array.from({ length: Math.floor(rnd() * 3) }, () => pick(CODES))),
    graph_links: Array.from({ length: Math.floor(rnd() * 3) }, () => ({ counterparty: pick(SUBJ), relation: pick(RELS), weight: +(0.3 + rnd() * 0.7).toFixed(2) })),
  };
}
export const byRisk = (a, b) => b.risk - a.risk;
export function initialQueue(count = 22) {
  return Array.from({ length: count }, () => makeCase()).sort(byRisk);
}
export const TIERS = [
  { id: "crit", label: "VERY HIGH", min: 0.85 },
  { id: "high", label: "HIGH", min: 0.6 },
  { id: "med", label: "MEDIUM", min: 0.3 },
  { id: "low", label: "LOW", min: 0 },
];
export const tierOf = (r) => TIERS.find((t) => r >= t.min);
