/* cardano-observer frontend - zero dependencies, one WebSocket. */
"use strict";

/* ── Category & icon registry ─────────────────────────────────────────── */

/** DEX venues emitted as `data.dex` - keep in sync with `src/dex.rs`. */
const DEX_VENUES = [
  "Minswap",
  "SundaeSwap",
  "WingRiders",
  "MuesliSwap",
  "Splash",
  "VyFinance",
  "CSWAP",
  "GeniusYield",
  "ChadSwap",
  "Dano Finance",
];

/** dApps emitted as `data.dapp` - keep in sync with `src/dapp/`. */
const DAPP_APPS = ["Iagon"];

/**
 * Governance subtype filters - CIP-1694 action types (proposals) plus other
 * governance event kinds. Ids match `data.actionType` / `kind` from the server.
 */
const GOV_TYPES = [
  { id: "treasuryWithdrawals", label: "Treasury Withdrawal" },
  { id: "protocolParametersUpdate", label: "Parameter Update" },
  { id: "hardForkInitiation", label: "Hard Fork" },
  { id: "constitutionalCommittee", label: "Committee Update" },
  { id: "constitution", label: "New Constitution" },
  { id: "noConfidence", label: "No Confidence" },
  { id: "information", label: "Info Action" },
  { id: "gov_vote", label: "Vote" },
  { id: "vote_delegation", label: "DRep Delegation" },
  { id: "drep_registration", label: "DRep Registration" },
  { id: "drep_update", label: "DRep Update" },
  { id: "drep_retirement", label: "DRep Retirement" },
  { id: "committee_auth", label: "Committee Auth" },
  { id: "committee_resign", label: "Committee Resign" },
];

function govTypeKey(ev) {
  if (!ev || ev.category !== "governance") return "";
  if (ev.kind === "gov_proposal") return String(ev.data?.actionType || "unknown");
  return String(ev.kind || "");
}

const CATS = [
  { id: "block",       label: "Blocks" },
  { id: "token",       label: "Tokens" },
  { id: "transaction", label: "Transactions" },
  { id: "dex",         label: "DEX" },
  { id: "dapp",        label: "dApp" },
  { id: "mint",        label: "Mint / Burn" },
  { id: "governance",  label: "Governance" },
  { id: "staking",     label: "Staking" },
  { id: "pool",        label: "Pools" },
  { id: "metadata",    label: "Metadata" },
  { id: "alert",       label: "Forks & Battles" },
];

const svg = (inner) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;

const ICONS = {
  block: svg('<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z"/><path d="M12 12l8-4.5M12 12L4 7.5M12 12v9"/>'),
  transaction: svg('<path d="M4 9h13"/><path d="M14 6l3 3-3 3"/><path d="M20 15H7"/><path d="M10 12l-3 3 3 3"/>'),
  token_transfer: svg('<circle cx="12" cy="12" r="8"/><path d="M12 7.5v9"/>'),
  mint: svg('<path d="M2 9.5L5.5 6.5h15V10H15v4h3.5v3.5H5.5V14H9V10H2z"/><path d="M6 17.5h12V20H6z"/>'),
  burn: svg('<path d="M12 3c3 3.5 6 6 6 11a6 6 0 0 1-12 0c0-2.5 1.2-4.5 3-6.2C8.2 10.5 9.8 12 12 11.2 11.5 8.5 11.8 5.5 12 3z"/>'),
  delegation: svg('<path d="M12 3l8 4-8 4-8-4 8-4z"/><path d="M4 13l8 4 8-4"/><path d="M4 17l8 4 8-4"/>'),
  vote_delegation: svg('<g transform="translate(12 12) scale(1.06) translate(-12 -12)"><path fill="currentColor" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round" stroke-linecap="round" paint-order="stroke fill" d="M17.62 6.73C17.52 6.76 17.38 6.81 17.31 6.86C17.24 6.90 16.88 7.23 16.51 7.59C15.66 8.42 15.15 8.82 14.39 9.25L14.08 9.43L12.95 9.43C11.63 9.43 11.64 9.43 10.68 9.09C9.57 8.69 9.02 8.58 8.28 8.62C7.35 8.66 6.55 9.00 5.84 9.66C5.70 9.78 5.36 10.16 5.09 10.48C4.47 11.22 4.10 11.58 3.40 12.12C3.10 12.35 2.81 12.58 2.75 12.63C2.53 12.84 2.45 13.16 2.54 13.47C2.58 13.62 2.66 13.75 2.88 14.01C3.03 14.20 3.35 14.58 3.58 14.86C3.81 15.15 4.27 15.71 4.61 16.12C4.95 16.52 5.30 16.96 5.39 17.08C5.58 17.31 5.66 17.37 5.74 17.34C5.77 17.33 6.05 17.04 6.36 16.71C7.37 15.62 7.63 15.41 8.16 15.19C8.68 14.98 8.98 14.95 10.66 14.94C12.15 14.94 12.69 14.91 13.23 14.81C15.09 14.45 17.05 13.30 19.09 11.37C19.98 10.53 21.17 9.14 21.43 8.62C21.50 8.50 21.52 8.38 21.53 8.19L21.55 7.93L21.47 7.78C21.42 7.70 21.33 7.59 21.27 7.53C21.11 7.41 20.79 7.32 20.58 7.35L20.41 7.37L20.39 7.29C20.30 7.02 20.01 6.78 19.68 6.71C19.42 6.65 19.09 6.71 18.84 6.87L18.64 6.99L18.54 6.90C18.31 6.70 17.94 6.64 17.62 6.73ZM18.09 7.02C18.19 7.04 18.30 7.09 18.33 7.13L18.40 7.21L17.73 7.92C17.00 8.71 16.49 9.16 15.96 9.52L15.62 9.75L15.37 9.63C15.20 9.54 15.05 9.49 14.92 9.48C14.81 9.47 14.72 9.44 14.72 9.43C14.72 9.41 14.74 9.40 14.75 9.40C14.80 9.40 15.35 9.02 15.72 8.73C15.92 8.58 16.37 8.16 16.73 7.81C17.37 7.19 17.52 7.07 17.74 7.02C17.89 6.98 17.91 6.98 18.09 7.02ZM19.82 7.08C19.95 7.15 20.10 7.34 20.10 7.43C20.09 7.46 20.01 7.54 19.90 7.61C19.79 7.68 19.51 7.94 19.21 8.26C18.15 9.39 17.52 9.92 16.80 10.25C16.55 10.37 16.09 10.54 16.02 10.54C16.01 10.54 16.00 10.49 16.00 10.44C16.00 10.39 15.96 10.26 15.91 10.17L15.83 9.98L16.04 9.84C16.71 9.39 17.17 8.98 18.02 8.06C18.65 7.39 18.86 7.20 19.03 7.10C19.28 6.98 19.59 6.97 19.82 7.08ZM21.01 7.74C21.24 7.88 21.30 8.12 21.18 8.43C21.05 8.77 20.15 9.86 19.27 10.75C18.02 12.03 16.56 13.13 15.26 13.78C14.83 14.00 14.06 14.28 13.63 14.39C12.89 14.58 12.64 14.60 10.90 14.62C10.02 14.63 9.16 14.65 9.00 14.67C8.34 14.75 7.74 14.98 7.27 15.35C7.13 15.46 6.72 15.87 6.37 16.25C6.01 16.63 5.70 16.94 5.69 16.94C5.68 16.93 5.51 16.74 5.33 16.51C5.14 16.28 4.52 15.51 3.94 14.81C3.36 14.11 2.87 13.49 2.84 13.44C2.78 13.30 2.79 13.16 2.87 13.00C2.92 12.89 3.07 12.76 3.55 12.40C4.33 11.79 4.62 11.52 5.24 10.78C5.53 10.45 5.88 10.06 6.02 9.91C6.48 9.47 6.99 9.18 7.62 9.01L7.88 8.94L8.45 8.94L9.02 8.94L9.38 9.02C9.77 9.10 10.09 9.20 10.79 9.46C11.05 9.56 11.38 9.66 11.53 9.69L11.81 9.76L13.41 9.77L15.02 9.79L15.15 9.85C15.62 10.07 15.80 10.49 15.60 10.90C15.49 11.12 15.28 11.28 15.00 11.36C14.87 11.40 13.90 11.54 12.70 11.69C11.55 11.83 10.59 11.96 10.57 11.99C10.50 12.04 10.51 12.18 10.58 12.22C10.66 12.26 14.91 11.73 15.14 11.66C15.48 11.53 15.75 11.31 15.90 11.02L15.98 10.87L16.24 10.79C16.80 10.63 17.32 10.36 17.86 9.96C18.11 9.78 18.49 9.43 19.13 8.78C19.73 8.18 20.10 7.83 20.19 7.78C20.49 7.63 20.80 7.62 21.01 7.74Z"/></g>'),
  stake_registration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 8h5M19.5 5.5v5" stroke-width="1.6"/>'),
  stake_deregistration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 8h5" stroke-width="1.6"/>'),
  withdrawal: svg('<path d="M5 11v8h14v-8"/><path d="M12 16V4"/><path d="M8.5 7.5L12 4l3.5 3.5"/>'),
  pool: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/>'),
  pool_registration: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/>'),
  pool_retirement: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/><path d="M4 4l16 16"/>'),
  gov_proposal: svg('<path d="M12 4v16"/><path d="M5 7h14"/><path d="M5 7l-2.6 5.2a3 3 0 0 0 5.9 0L5.7 7"/><path d="M18.3 7l-2.6 5.2a3 3 0 0 0 5.9 0L19 7"/><path d="M8 20h8"/>'),
  gov_vote: svg('<rect x="5" y="4" width="14" height="16" rx="2"/><path d="M9 12l2.2 2.2L15.5 9.5"/>'),
  gov_vote_no: svg('<rect x="5" y="4" width="14" height="16" rx="2"/><path d="M9 9l6 6M15 9l-6 6"/>'),
  gov_vote_abstain: svg('<rect x="5" y="4" width="14" height="16" rx="2"/><path d="M8 13c1.6-2.8 3-2.8 4 0s2.4 2.8 4 0"/>'),
  drep_registration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M16.5 3.5l2 2 3.5-4" stroke-width="1.6"/>'),
  drep_update: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/>'),
  drep_retirement: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 5h5" stroke-width="1.6"/>'),
  committee_auth: svg('<circle cx="8" cy="8" r="3.4"/><circle cx="16" cy="8" r="3.4"/><path d="M2.5 20c1.2-3.2 3.4-4.6 5.5-4.6M21.5 20c-1.2-3.2-3.4-4.6-5.5-4.6"/>'),
  committee_resign: svg('<circle cx="8" cy="8" r="3.4"/><circle cx="16" cy="8" r="3.4"/><path d="M2.5 20c1.2-3.2 3.4-4.6 5.5-4.6M21.5 20c-1.2-3.2-3.4-4.6-5.5-4.6"/><path d="M10 12l4-4"/>'),
  tx_metadata: svg('<path d="M4 5h16v11H9.5L4 20V5z"/><path d="M8 9h8M8 12h5"/>'),
  certificate: svg('<path d="M7 3h7l5 5v13H7V3z"/><path d="M14 3v5h5"/><path d="M10 13h5M10 16h5"/>'),
  rollback: svg('<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>'),
  orphaned_block: svg('<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z"/><path d="M4.5 6L19.5 18" stroke-dasharray="2.5 2.5"/>'),
  slot_battle: svg('<path d="M14.5 17.5L3 6V3h3l11.5 11.5"/><path d="M13 19l6-6M16 16l4 4M19 21l2-2"/><path d="M9.5 17.5L21 6V3h-3L6.5 14.5"/><path d="M11 19l-6-6M8 16l-4 4M5 21l-2-2"/>'),
  dex: svg('<circle cx="12" cy="12" r="9"/><path d="M8 10h7M12.5 7.5L15 10l-2.5 2.5"/><path d="M16 14.5H9M11.5 12l-2.5 2.5L11.5 17"/>'),
  dex_order: svg('<circle cx="12" cy="12" r="9"/><path d="M12 8v8M8 12h8"/>'),
  dex_fill: svg('<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.5 2.5L16.5 9"/>'),
  dex_lp: svg('<path d="M12 3v18"/><path d="M5 8h14"/><path d="M7 8c0 4 2.5 8 5 10"/><path d="M17 8c0 4-2.5 8-5 10"/><path d="M8 14h8"/>'),
  dex_lp_redeem: svg('<path d="M5 11v8h14v-8"/><path d="M12 16V4"/><path d="M8.5 7.5L12 4l3.5 3.5"/>'),
  dex_cancel: svg('<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>'),
  dapp: svg('<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>'),
  dapp_activity: svg('<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>'),
};
const iconFor = (kind, category, side, vote) => {
  if (kind === "dex_lp" && side === "redeem") return ICONS.dex_lp_redeem;
  if (kind === "delegation") return ICONS.vote_delegation;
  if (kind === "gov_vote") {
    const v = String(vote || "").toLowerCase();
    if (v === "no") return ICONS.gov_vote_no;
    if (v === "abstain") return ICONS.gov_vote_abstain;
    return ICONS.gov_vote;
  }
  return ICONS[kind] || ICONS[{ token: "token_transfer", staking: "delegation", governance: "gov_proposal", metadata: "tx_metadata", alert: "slot_battle", dex: "dex", dapp: "dapp", pool: "pool", mint: "mint" }[category]] || ICONS.transaction;
};

/* ── Tiny helpers ─────────────────────────────────────────────────────── */

const $ = (id) => document.getElementById(id);
const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
/** Capitalize the first letter of each word; leave already-capital letters alone (LP, Minswap, …). */
const titleCaseWords = (s) =>
  String(s ?? "").replace(/(^|[^A-Za-z0-9])([a-z])/g, (_, sep, ch) => sep + ch.toUpperCase());
const short = (h, a = 8, b = 6) => {
  h = String(h ?? "");
  return h.length <= a + b + 1 ? h : h.slice(0, a) + "…" + h.slice(-b);
};
const nf = new Intl.NumberFormat("en-US");
const fmtInt = (n) => nf.format(Math.round(Number(n) || 0));

/** Wrap fractional digits in a lighter `<span class="frac">` (keeps B/M / ₳ prefixes). */
function decorateFrac(formatted) {
  const s = String(formatted ?? "");
  const m = s.match(/^([^0-9.-]*)(-?[\d,]+)(\.\d+)?(.*)$/);
  if (!m || !m[3]) return `<span class="num">${esc(s)}</span>`;
  return `<span class="num">${esc(m[1])}${esc(m[2])}<span class="frac">${esc(m[3])}</span>${esc(m[4])}</span>`;
}

function fmtAda(lovelace) {
  const ada = Number(lovelace || 0) / 1e6;
  let plain;
  if (ada >= 1e9) plain = "₳ " + (ada / 1e9).toFixed(2) + "B";
  else if (ada >= 1e6) plain = "₳ " + (ada / 1e6).toFixed(2) + "M";
  else if (ada >= 1e4) plain = "₳ " + nf.format(Math.round(ada));
  else if (ada >= 1) plain = "₳ " + nf.format(+ada.toFixed(2));
  else plain = "₳ " + (+ada.toFixed(6));
  return decorateFrac(plain);
}

function fmtQty(qtyStr, decimals) {
  let s = String(qtyStr ?? "0");
  const neg = s.startsWith("-");
  if (neg) s = s.slice(1);
  let v;
  if (decimals > 0) {
    s = s.padStart(decimals + 1, "0");
    v = Number(s.slice(0, -decimals) + "." + s.slice(-decimals));
  } else {
    v = Number(s);
  }
  let out;
  if (v >= 1e9) out = (v / 1e9).toFixed(2) + "B";
  else if (v >= 1e6) out = (v / 1e6).toFixed(2) + "M";
  else if (v >= 1e4) out = nf.format(Math.round(v));
  else out = nf.format(+v.toFixed(Math.min(decimals || 0, 4)));
  return decorateFrac((neg ? "-" : "") + out);
}

/** Format on-chain qty with known decimals. Unknown → placeholder until registry
 *  meta lands (never show undivided raw units - those look like billions). */
function fmtTokenQty(qtyStr, decimals) {
  if (decimals == null || decimals === "" || Number.isNaN(Number(decimals))) return "…";
  return fmtQty(qtyStr, Number(decimals));
}

/** Prefer event-stamped decimals, then the in-memory registry /api/registry map. */
function registryMetaFor(unit) {
  if (!unit) return null;
  if (assetMeta.has(unit)) return assetMeta.get(unit);
  if (unit.length <= 56) return null;
  // Match server lookup_keys for CIP-68 label variants.
  const policy = unit.slice(0, 56);
  const name = unit.slice(56);
  for (const prefix of ["000de140", "0014df10"]) {
    if (name.startsWith(prefix)) {
      const bare = policy + name.slice(prefix.length);
      if (assetMeta.has(bare)) return assetMeta.get(bare);
    } else {
      const withP = policy + prefix + name;
      if (assetMeta.has(withP)) return assetMeta.get(withP);
    }
  }
  return null;
}

function tokenDecimals(a) {
  if (a && a.decimals != null && a.decimals !== "") {
    const n = Number(a.decimals);
    if (Number.isFinite(n)) return n;
  }
  const meta = registryMetaFor(a?.unit || "");
  if (meta?.decimals != null) {
    const n = Number(meta.decimals);
    if (Number.isFinite(n)) return n;
  }
  // After registry hydrate: unregistered tokens (SONGMARKETCAP etc.) use CIP-26 default 0.
  if (registryReady && a?.qty != null && a?.unit) return 0;
  return null;
}

function tokenLabel(a) {
  if (!a) return "";
  const meta = registryMetaFor(a.unit || "");
  return a.ticker || meta?.ticker || a.name || meta?.name
    || (a.fingerprint ? short(a.fingerprint, 8, 4) : short(a.unit, 8, 4));
}

function timeAgo(ts) {
  const d = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (d < 3) return "now";
  if (d < 60) return d + "s ago";
  if (d < 3600) return Math.floor(d / 60) + "m ago";
  if (d < 86400) return Math.floor(d / 3600) + "h ago";
  return Math.floor(d / 86400) + "d ago";
}
const clock = (ts) => new Date(ts * 1000).toLocaleTimeString();

/** Copy plain text. Clipboard API needs a secure context (https/localhost);
 *  fall back to a temporary textarea so LAN http://192.168… still works. */
async function copyText(t) {
  const text = String(t ?? "");
  if (!text) return false;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* fall through */
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.cssText = "position:fixed;left:-9999px;top:0";
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  } catch {
    return false;
  }
}


/* ── Persistent settings ──────────────────────────────────────────────── */

const store = {
  get(k, dflt) {
    try { const v = localStorage.getItem(k); return v == null ? dflt : JSON.parse(v); }
    catch { return dflt; }
  },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch {} },
};

const settings = {
  // Merge over defaults so categories added in later versions start visible
  filters: { ...Object.fromEntries(CATS.map((c) => [c.id, true])), ...store.get("co_filters_v1", {}) },
  // Per-venue DEX toggles (true = include). Unknown venues stay visible.
  dexVenues: {
    ...Object.fromEntries(DEX_VENUES.map((d) => [d, true])),
    ...store.get("co_dex_venues_v1", {}),
  },
  // Per-dApp toggles (true = include). Unknown dApps stay visible.
  dappApps: {
    ...Object.fromEntries(DAPP_APPS.map((d) => [d, true])),
    ...store.get("co_dapp_apps_v1", {}),
  },
  // Per-type governance toggles (true = include). Unknown types stay visible.
  govTypes: {
    ...Object.fromEntries(GOV_TYPES.map((g) => [g.id, true])),
    ...store.get("co_gov_types_v1", {}),
  },
  layout: store.get("co_layout_v1", "vertical"),
  // Mobile defaults to compact; desktop stays roomy unless the user toggled it.
  compact: store.get(
    "co_compact_v1",
    typeof window.matchMedia === "function"
      ? window.matchMedia("(max-width: 720px)").matches
      : window.innerWidth <= 720
  ),
  minAda: store.get("co_minada_v1", 0),
};

function dexVenueEnabled(venue) {
  if (!venue) return true;
  return settings.dexVenues[venue] !== false;
}

function dappAppEnabled(app) {
  if (!app) return true;
  return settings.dappApps[app] !== false;
}

function govTypeEnabled(type) {
  if (!type) return true;
  return settings.govTypes[type] !== false;
}

/* ── Global state ─────────────────────────────────────────────────────── */

let NETWORK = "mainnet";
const feed = $("feed");

/* ── Light-cone hover: spend-graph highlighting ───────────────────────────
 * txGraph maps a tx hash to its immediate spend neighbours:
 *   ins  = txs whose outputs this tx spends (its direct past)
 *   outs = txs that spend this tx's outputs (its direct future)
 * Built incrementally from every ingested transaction event's data.inputTxs.
 * On hover we BFS both directions to light the whole light cone and dim the
 * rest of the feed. `seen` marks a tx we actually ingested (vs. a placeholder
 * created only because a newer tx referenced it as an input).
 */
const txGraph = new Map(); // txHash -> { ins:Set, outs:Set, seen:bool }

function txNode(h) {
  let n = txGraph.get(h);
  if (!n) { n = { ins: new Set(), outs: new Set(), seen: false }; txGraph.set(h, n); }
  return n;
}

function indexTxGraph(ev) {
  if (!ev || ev.kind !== "transaction" || !ev.tx_hash) return;
  const node = txNode(ev.tx_hash);
  node.seen = true;
  const inputs = ev.data && ev.data.inputTxs;
  if (Array.isArray(inputs)) {
    for (const src of inputs) {
      if (src === ev.tx_hash) continue;
      node.ins.add(src);
      txNode(src).outs.add(ev.tx_hash);
    }
  }
}

function gcTxNode(h) {
  const n = txGraph.get(h);
  if (n && !n.seen && n.ins.size === 0 && n.outs.size === 0) txGraph.delete(h);
}

// Drop a trimmed tx's edges so the graph stays bounded to the retention window.
function unindexTxGraph(ev) {
  if (!ev || ev.kind !== "transaction" || !ev.tx_hash) return;
  const h = ev.tx_hash;
  const node = txGraph.get(h);
  if (!node) return;
  for (const src of node.ins) {
    const sn = txGraph.get(src);
    if (sn) { sn.outs.delete(h); gcTxNode(src); }
  }
  node.ins.clear();
  node.seen = false;
  gcTxNode(h);
}

// BFS the spend graph in one direction ("ins" = past cone, "outs" = future).
function coneReach(start, dir) {
  const out = new Set();
  const stack = [start];
  const seen = new Set([start]);
  let guard = 0;
  while (stack.length && guard++ < 8000) {
    const node = txGraph.get(stack.pop());
    if (!node) continue;
    for (const nx of node[dir]) {
      if (seen.has(nx)) continue;
      seen.add(nx);
      out.add(nx);
      stack.push(nx);
    }
  }
  return out;
}

let lcTx = null;   // tx hash currently focused by hover
let lcLit = [];    // cards currently carrying a light-cone class

function clearLightCone() {
  if (lcTx === null && lcLit.length === 0) return;
  feed.classList.remove("lc-active");
  for (const c of lcLit) c.classList.remove("lc-self", "lc-past", "lc-future");
  lcLit = [];
  lcTx = null;
}

function litCards(hash, cls) {
  const key = (window.CSS && CSS.escape) ? CSS.escape(hash) : hash;
  for (const c of feed.querySelectorAll(`.card[data-tx="${key}"]`)) {
    if (c.classList.contains("f-hide") || c.closest(".block-group.f-hide")) continue;
    // One role per card - avoid stacked past+future shadows if a hash
    // somehow appears in both cones.
    c.classList.remove("lc-self", "lc-past", "lc-future");
    c.classList.add(cls);
    lcLit.push(c);
  }
}

function showLightCone(hash) {
  clearLightCone();
  lcTx = hash;
  if (!hash) return;
  const past = coneReach(hash, "ins");
  const future = coneReach(hash, "outs");
  feed.classList.add("lc-active");
  litCards(hash, "lc-self");
  for (const h of past) if (h !== hash && !future.has(h)) litCards(h, "lc-past");
  for (const h of future) if (h !== hash) litCards(h, "lc-future");
}

feed.addEventListener("mouseover", (e) => {
  const card = e.target.closest && e.target.closest(".card");
  if (!card || !feed.contains(card)) return;
  const tx = card.dataset.tx || null;
  if (tx === lcTx) return;      // still inside the same tx's cards
  if (!tx) { clearLightCone(); return; } // blocks / rollbacks have no cone
  showLightCone(tx);
});
feed.addEventListener("mouseleave", clearLightCone);

const groups = new Map();      // block hash -> .block-group element
const groupOrder = [];         // newest first
const seenEventIds = new Set(); // dedupe when merging search hits into the feed
const MAX_GROUPS = 50_000; // soft safety cap while scrolling back
const pending = [];            // buffered events while user is reading
let oldestEventId = null;      // smallest id currently in the feed
let historyExhausted = false;
let historyLoading = false;
/** Coalesce scroll/wheel storms so sparse filters don't flash-load every frame. */
let historyLoadTimer = 0;
/** After several disk pages add nothing visible, pause until filters change. */
let historySparsePause = false;
/** One load per approach to the history end; re-arms after leaving the end zone. */
let historyLoadArmed = true;
/** Non-empty when the page was opened with `?term` / `?q=` - drives search priming. */
let urlSearchPreset = "";
/** True while preloading history to fill the first search page. */
let searchPriming = false;
/** True while the user-triggered "Load more history" chunk is in flight. */
let searchExtending = false;
/** Bumped to cancel an in-flight search prime loop. */
let searchPrimeGen = 0;
/** AbortController for the active history fetch (prime or scroll). */
let historyAbort = null;
/** How many tip-side events the current search has already considered. */
let searchScanned = 0;
/** How many events the server currently holds in the retention window. */
let bufferedEventCount = 0;
/** Retention window reported by the server (hours). */
let retentionHours = 24;
/** Total matches for the active query (from local filter). */
let searchMatchesTotal = 0;
/** All matches for the active query (newest-first), held in JS - not all rendered. */
let searchHitBuffer = [];
/** How many buffer entries have been rendered into the DOM. */
let searchHitOffset = 0;
/** True when every buffered match has been rendered. */
let searchHitsExhausted = false;

/**
 * Client-side copy of the server's 24h retention window.
 * Kept fully in memory; the feed only renders small pages from it on scroll.
 */
const retentionCache = new Map(); // id -> { ev, hay }
let retentionReady = false;
let retentionLoading = null; // Promise while /api/buffer is in flight
let retentionLoadGen = 0; // bumped on reconnect so stale preloads don't notify
let retentionWaiters = []; // resolvers waiting for ready
/** Newest-first events matching current filters (from retentionCache). */
let feedHitBuffer = [];
/** How far through feedHitBuffer we have rendered into the DOM. */
let feedHitOffset = 0;
/** Serialized filter state used to build feedHitBuffer. */
let feedHitKey = "";
/** True once feedHitBuffer has been fully rendered (further scroll → disk). */
let retentionHistoryDone = false;

const SEARCH_PRIME_LOOKBACK = 300;
/** Events rendered into the DOM per scroll page (from the in-memory 24h buffer). */
const FEED_PAGE_SIZE = 40;
/** Matches rendered per search page - keeps the DOM/scrollbar bounded. */
const SEARCH_PAGE_SIZE = 40;
/** Older-than-24h disk pages (only after retention is exhausted). */
const HISTORY_PAGE_SIZE = 40;
const catCounts = Object.fromEntries(CATS.map((c) => [c.id, 0]));
/** Ids counted in catCounts / the session total (retention window + scrolled history). */
const loadedEventIds = new Set();
/** Older-than-retention events the user explicitly scrolled in (kept after trim). */
const extraHistoryIds = new Set();
/** id → unix seconds - drives activity sparkline over loaded history. */
const loadedTimestamps = new Map();
let sessionEvents = 0;

/* ── Filter chips & toolbar ───────────────────────────────────────────── */

const filterStyle = document.createElement("style");
document.head.appendChild(filterStyle);

function applyFilters() {
  const css = CATS.filter((c) => !settings.filters[c.id])
    .map((c) => `#feed .card[data-category="${c.id}"]{display:none}`)
    .join("\n");
  const minL = settings.minAda * 1e6;
  filterStyle.textContent = css;
  // min-ADA + search + subtype filters need per-card logic
  const q = $("search").value.trim().toLowerCase();
  for (const g of groupOrder) {
    let visible = 0;
    g.querySelectorAll(".card").forEach((card) => {
      let hide = false;
      if (minL > 0 && card.dataset.category === "transaction" && Number(card.dataset.ada || 0) < minL) hide = true;
      if (q && !(card.dataset.search || "").includes(q)) hide = true;
      if (card.dataset.category === "dex" && !dexVenueEnabled(card.dataset.dex)) hide = true;
      if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) hide = true;
      if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) hide = true;
      card.classList.toggle("f-hide", hide);
      if (!hide && settings.filters[card.dataset.category]) {
        visible++;
      }
    });
    // Collapse groups with nothing visible (category / venue / search filters),
    // so rare filtered events aren't separated by long empty spine stretches.
    // Orphaned blocks (and their detail events) are part of Forks & Battles —
    // hide the whole group when that filter is off.
    const hideOrphan = g.classList.contains("orphaned") && !settings.filters.alert;
    g.classList.toggle("f-hide", visible === 0 || hideOrphan);
  }
  store.set("co_filters_v1", settings.filters);
  store.set("co_minada_v1", settings.minAda);
  store.set("co_dex_venues_v1", settings.dexVenues);
  store.set("co_dapp_apps_v1", settings.dappApps);
  store.set("co_gov_types_v1", settings.govTypes);
  updateLoadedEventCount();
  if (pending.length) updateNewPill();
  if (!searchPriming && $("search").value.trim()) {
    updateSearchEmptyPrompt();
  } else if (!$("search").value.trim()) {
    hideSearchPrompts();
    // Only rebuild the scroll buffer when filter settings actually change.
    if (retentionReady && feedFilterKey() !== feedHitKey) {
      queueMicrotask(() => onFeedFiltersChanged());
    }
  }
}

/** Visible (filter-matching) events currently shown in the feed. */
function countVisibleEvents() {
  let n = 0;
  let oldest = Infinity;
  let newest = -Infinity;
  document.querySelectorAll("#feed .block-group:not(.f-hide) .card:not(.f-hide)").forEach((card) => {
    if (!settings.filters[card.dataset.category]) return;
    if (card.dataset.category === "dex" && !dexVenueEnabled(card.dataset.dex)) return;
    if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return;
    if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return;
    n++;
    const ts = Number(card.querySelector(".ev-time")?.dataset.ts || 0);
    if (ts > 0) {
      if (ts < oldest) oldest = ts;
      if (ts > newest) newest = ts;
    }
  });
  const hours = Number.isFinite(oldest) && newest >= oldest ? (newest - oldest) / 3600 : 0;
  return { n, hours };
}

/** Footer count: filtered visible events only (not the full loaded total). */
function updateLoadedEventCount() {
  const el = $("ft-session");
  if (!el) return;
  const { n, hours } = countVisibleEvents();
  const span = formatHistorySpan(hours);
  const unit = `event${n === 1 ? "" : "s"}`;
  el.textContent = span
    ? `${fmtInt(n)} ${unit} · ${span}`
    : `${fmtInt(n)} ${unit}`;
}

function resetLoadedCounts() {
  loadedEventIds.clear();
  extraHistoryIds.clear();
  loadedTimestamps.clear();
  for (const c of CATS) catCounts[c.id] = 0;
  updateLoadedEventCount();
  paintCatCounts();
  refreshActivityMonitor();
}

function noteLoadedEvent(ev) {
  if (!ev || ev.id == null || loadedEventIds.has(ev.id)) return;
  loadedEventIds.add(ev.id);
  const cat = ev.category;
  if (cat) catCounts[cat] = (catCounts[cat] || 0) + 1;
  if (ev.timestamp != null) loadedTimestamps.set(ev.id, Number(ev.timestamp) || 0);
}

function forgetLoadedEvent(ev) {
  if (!ev || ev.id == null || !loadedEventIds.has(ev.id)) return;
  // Scrolled-in history older than the retention window stays in the total.
  if (extraHistoryIds.has(ev.id)) return;
  loadedEventIds.delete(ev.id);
  loadedTimestamps.delete(ev.id);
  const cat = ev.category;
  if (cat && catCounts[cat] > 0) catCounts[cat]--;
}

function loadedHistoryHours() {
  let oldest = Infinity;
  let newest = -Infinity;
  for (const ts of loadedTimestamps.values()) {
    if (!(ts > 0)) continue;
    if (ts < oldest) oldest = ts;
    if (ts > newest) newest = ts;
  }
  if (!Number.isFinite(oldest) || newest < oldest) return 0;
  return (newest - oldest) / 3600;
}

/** Compact span label: `12m`, `3.5h`, `24h`. */
function formatHistorySpan(hours) {
  if (!(hours > 0)) return "";
  if (hours < 1) return `${Math.max(1, Math.round(hours * 60))}m`;
  if (hours < 10) {
    const t = Math.round(hours * 10) / 10;
    return `${t}h`;
  }
  return `${Math.round(hours)}h`;
}

function paintCatCounts() {
  for (const c of CATS) {
    const el = document.querySelector(`[data-cat-n="${c.id}"]`);
    if (el) el.textContent = catCounts[c.id] ? fmtInt(catCounts[c.id]) : "";
  }
}

/** Pre-fill search from the URL: `?minswap`, `?q=minswap`, or `?search=BROCK`. */
function searchFromUrl() {
  const params = new URLSearchParams(location.search);
  for (const key of ["q", "search", "filter"]) {
    if (!params.has(key)) continue;
    const v = params.get(key);
    if (v != null && String(v).trim() !== "") return String(v).trim();
  }
  // Bare flag style: http://host:9070/?minswap
  for (const [k, v] of params.entries()) {
    if (!k || ["q", "search", "filter"].includes(k)) continue;
    if (v === "" || v == null) return k;
  }
  return "";
}

/**
 * Category chip with a right-side multi-select submenu.
 * `opts` items are `{ id, label }`; toggles live in `settings[settingsKey]`.
 */
function buildSplitFilterChip(chips, {
  catId,
  label,
  iconKind,
  settingsKey,
  options,
  isEnabled,
  menuAria,
}) {
  const wrap = document.createElement("div");
  wrap.className = "chip-wrap";
  wrap.style.setProperty("--c", `var(--c-${catId})`);

  const b = document.createElement("button");
  b.type = "button";
  b.className = "chip chip-main" + (settings.filters[catId] ? " on" : "");
  b.innerHTML =
    `${iconFor(iconKind, catId)}<span>${esc(label)}</span><span class="n" data-cat-n="${catId}"></span>`;
  b.title = `show/hide ${label.toLowerCase()}`;
  b.onclick = () => {
    settings.filters[catId] = !settings.filters[catId];
    b.classList.toggle("on", settings.filters[catId]);
    wrap.classList.toggle("on", settings.filters[catId]);
    applyFilters();
  };

  const caret = document.createElement("button");
  caret.type = "button";
  caret.className = "chip-caret";
  caret.setAttribute("aria-label", menuAria);
  caret.setAttribute("aria-haspopup", "menu");
  caret.setAttribute("aria-expanded", "false");
  caret.innerHTML =
    `<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">` +
    `<path fill="currentColor" d="M4.2 6.2a.75.75 0 0 1 1.06 0L8 8.94l2.74-2.74a.75.75 0 1 1 1.06 1.06l-3.27 3.27a.75.75 0 0 1-1.06 0L4.2 7.26a.75.75 0 0 1 0-1.06z"/></svg>`;

  const menu = document.createElement("div");
  menu.className = "chip-sub-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;

  const rebuildMenu = () => {
    menu.innerHTML = "";
    for (const opt of options) {
      const id = typeof opt === "string" ? opt : opt.id;
      const text = typeof opt === "string" ? opt : opt.label;
      const on = isEnabled(id);
      const row = document.createElement("button");
      row.type = "button";
      row.className = "chip-sub-opt" + (on ? " on" : "");
      row.setAttribute("role", "menuitemcheckbox");
      row.setAttribute("aria-checked", on ? "true" : "false");
      row.innerHTML =
        `<span class="chip-sub-check" aria-hidden="true">${on ? "✓" : ""}</span>` +
        `<span class="chip-sub-label">${esc(text)}</span>`;
      row.onclick = (e) => {
        e.stopPropagation();
        settings[settingsKey][id] = !isEnabled(id);
        rebuildMenu();
        applyFilters();
      };
      menu.appendChild(row);
    }
  };
  rebuildMenu();

  const closeMenu = () => {
    menu.hidden = true;
    wrap.classList.remove("open");
    caret.setAttribute("aria-expanded", "false");
  };
  const openMenu = () => {
    rebuildMenu();
    menu.hidden = false;
    wrap.classList.add("open");
    caret.setAttribute("aria-expanded", "true");
  };

  caret.onclick = (e) => {
    e.stopPropagation();
    if (menu.hidden) openMenu();
    else closeMenu();
  };

  document.addEventListener("click", (e) => {
    if (!wrap.contains(e.target)) closeMenu();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeMenu();
  });

  wrap.classList.toggle("on", settings.filters[catId]);
  wrap.append(b, caret, menu);
  chips.appendChild(wrap);
}

function buildToolbar() {
  const chips = $("chips");
  for (const c of CATS) {
    if (c.id === "dex") {
      buildSplitFilterChip(chips, {
        catId: "dex",
        label: "DEX",
        iconKind: "dex",
        settingsKey: "dexVenues",
        options: DEX_VENUES,
        isEnabled: dexVenueEnabled,
        menuAria: "Filter by DEX venue",
      });
      continue;
    }
    if (c.id === "dapp") {
      buildSplitFilterChip(chips, {
        catId: "dapp",
        label: "dApp",
        iconKind: "dapp",
        settingsKey: "dappApps",
        options: DAPP_APPS,
        isEnabled: dappAppEnabled,
        menuAria: "Filter by dApp",
      });
      continue;
    }
    if (c.id === "governance") {
      buildSplitFilterChip(chips, {
        catId: "governance",
        label: "Governance",
        iconKind: "gov_proposal",
        settingsKey: "govTypes",
        options: GOV_TYPES,
        isEnabled: govTypeEnabled,
        menuAria: "Filter by governance action type",
      });
      continue;
    }
    const b = document.createElement("button");
    b.className = "chip" + (settings.filters[c.id] ? " on" : "");
    b.style.setProperty("--c", `var(--c-${c.id})`);
    b.innerHTML = `${iconFor(c.id === "alert" ? "slot_battle" : c.id, c.id)}<span>${c.label}</span><span class="n" data-cat-n="${c.id}"></span>`;
    b.title = `show/hide ${c.label.toLowerCase()}`;
    b.onclick = () => {
      settings.filters[c.id] = !settings.filters[c.id];
      b.classList.toggle("on", settings.filters[c.id]);
      applyFilters();
    };
    chips.appendChild(b);
  }
  const search = $("search");
  let deb;
  const onSearchInput = () => {
    if (!search.value.trim()) {
      searchScanned = 0;
      hideSearchPrompts();
      setSearchPrime(false);
      clearTimeout(emptyRecheckTimer);
      // Native clear (x) or delete - drop priming immediately.
      if (searchPriming || ($("search-prime") && !$("search-prime").hidden)) {
        cancelSearchPrime();
        return;
      }
      applyFilters();
      return;
    }
    clearTimeout(deb);
    deb = setTimeout(() => {
      applyFilters();
      // Auto-prime through the in-memory retention window (trending clicks + typed search).
      if (!urlSearchPreset) {
        runSearchPrime(document.querySelectorAll("#feed .card").length);
      }
    }, 180);
  };
  search.oninput = onSearchInput;
  search.addEventListener("search", onSearchInput); // fires on clear in WebKit
  urlSearchPreset = searchFromUrl();
  if (urlSearchPreset) {
    search.value = urlSearchPreset;
    setSearchPrime(true);
  }
  $("search-empty-more")?.addEventListener("click", () => {
    extendSearchHistory();
  });
  $("search-more-btn")?.addEventListener("click", () => {
    extendSearchHistory();
  });

  const minAda = $("min-ada");
  minAda.value = String(settings.minAda);
  minAda.onchange = () => { settings.minAda = Number(minAda.value); applyFilters(); };

  const layoutBtn = $("layout-btn");
  const setLayoutBtn = () => {
    layoutBtn.innerHTML = settings.layout === "vertical"
      ? svg('<path d="M12 4v16M8 8l4-4 4 4M8 16l4 4 4-4"/>') + "vertical"
      : svg('<path d="M4 12h16M8 8l-4 4 4 4M16 8l4 4-4 4"/>') + "horizontal";
  };
  setLayoutBtn();
  feed.className = settings.layout;
  layoutBtn.onclick = () => {
    clearLightCone();
    settings.layout = settings.layout === "vertical" ? "horizontal" : "vertical";
    feed.className = settings.layout;
    store.set("co_layout_v1", settings.layout);
    setLayoutBtn();
  };

  const compactBtn = $("compact-btn");
  document.body.classList.toggle("compact", settings.compact);
  compactBtn.classList.toggle("on", settings.compact);
  compactBtn.onclick = () => {
    settings.compact = !settings.compact;
    document.body.classList.toggle("compact", settings.compact);
    compactBtn.classList.toggle("on", settings.compact);
    store.set("co_compact_v1", settings.compact);
  };
}

setInterval(paintCatCounts, 1200);

/* ── Card builders ────────────────────────────────────────────────────── */

/* Each part is one flex item - otherwise a nested .frac span becomes its own
   item and picks up the 10px flex gap (e.g. "0" + gap + ".508841"). */
const sub = (parts) =>
  parts
    .filter(Boolean)
    .map((p) => `<span class="sub-i">${p}</span>`)
    .join('<span class="sep">·</span>');

function assetChipsHtml(assets) {
  if (!assets || !assets.items || !assets.items.length) return "";
  const chips = assets.items
    .map((a) => {
      const unit = a.unit || "";
      const decimals = tokenDecimals(a);
      const label = tokenLabel(a);
      return `<span class="asset" data-unit="${esc(unit)}" title="${esc(a.policy)}.${esc(a.nameHex)}">
        <span class="ph">◆</span><span class="t">${esc(label)}</span><span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span></span>`;
    })
    .join("");
  const more = assets.more ? `<span class="asset"><span class="t">+${assets.more} more</span></span>` : "";
  return `<div class="assets">${chips}${more}</div>`;
}

/** Plain `₳ 123` / `16,490 COCK` - no chip chrome on DEX cards. */
function dexAmtAda(lovelace, min) {
  if (lovelace == null || lovelace === "") return "";
  return `<b>${min ? "≥ " : ""}${fmtAda(lovelace)}</b>`;
}
function dexAmtToken(assets, min) {
  const a = assets && assets.items && assets.items[0];
  if (!a) return "";
  const unit = a.unit || "";
  const decimals = tokenDecimals(a);
  const label = tokenLabel(a);
  const qty = `<span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span>`;
  const name = `<span class="t">${esc(label)}</span>`;
  return `<b class="dex-amt" data-unit="${esc(unit)}">${min ? "≥" : ""}${qty} ${name}</b>`;
}
function dexAmtTokens(assets) {
  const items = (assets && assets.items) || [];
  return items.map((a) => {
    const unit = a.unit || "";
    const decimals = tokenDecimals(a);
    const label = tokenLabel(a);
    return `<b class="dex-amt" data-unit="${esc(unit)}"><span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span> <span class="t">${esc(label)}</span></b>`;
  }).join(" + ");
}

/** CIP-26 known: server-stamped ticker, or client registry hydrate. */
function cip26Known(a) {
  if (!a) return false;
  if (a.ticker && String(a.ticker).trim()) return true;
  return !!registryMetaFor(a.unit || "");
}

/** Hide swap/order DEX cards when any token is outside CIP-26 (or ask is missing). */
function keepDexEvent(ev) {
  const kind = ev.kind || "";
  if (kind !== "dex_order" && kind !== "dex_fill" && kind !== "dex_cancel") return true;
  const d = ev.data || {};
  const side = d.side || "";
  const wantItems = (d.want && d.want.items) || [];
  const assetItems = (d.assets && d.assets.items) || [];
  const hasWant = d.wantAda != null || wantItems.length > 0;
  if ((side === "buy" || side === "sell") && !hasWant) return false;
  if (side === "swap" && (!assetItems.length || !hasWant)) return false;
  for (const a of assetItems) {
    if (!cip26Known(a)) return false;
  }
  for (const a of wantItems) {
    if (!cip26Known(a)) return false;
  }
  return true;
}
/** Paid → want flow for buy / sell / swap / fill; LP shows deposited amounts. */
function dexFlowHtml(d) {
  if (!d) return "";
  const paidAda = d.ada ? dexAmtAda(d.ada) : "";
  const paidToks = dexAmtTokens(d.assets);
  if (d.side === "deposit" || d.side === "redeem") {
    return [paidAda, paidToks].filter(Boolean).join(" + ");
  }
  const wantAda = d.wantAda != null ? dexAmtAda(d.wantAda, d.wantMin) : "";
  const wantTok = dexAmtToken(d.want, d.wantMin);
  // Unresolved / unregistered wants: omit (never show a fake "token" label).
  const want = wantAda || wantTok;
  const paidTok = dexAmtToken(d.assets);
  let paid = "";
  if (d.side === "buy") paid = paidAda;
  else if (d.side === "sell") paid = paidTok || paidAda;
  else paid = paidTok && paidAda ? `${paidAda} + ${paidTok}` : (paidTok || paidAda);
  if (paid && want) return `${paid} <span class="sep">→</span> ${want}`;
  return paid || want;
}

function dexStatusPill(ev, d) {
  if (d.filled || ev.kind === "dex_fill") return "";
  if (ev.kind === "dex_cancel") return `<span class="badge cancelled">Cancelled</span>`;
  if (ev.kind === "dex_lp") {
    return `<span class="badge pending pulse-pending">Pending</span>`;
  }
  if (ev.kind === "dex_order" && (d.side === "buy" || d.side === "sell")) {
    return `<span class="badge pending pulse-pending">Pending</span>`;
  }
  return "";
}

function cardBody(ev) {
  const d = ev.data || {};
  switch (ev.kind) {
    case "block": {
      // Pool id is the visible fallback; enrichPools replaces with ticker when known.
      const pool = d.issuerPool
        ? `<span class="pool-id" data-pool="${esc(d.issuerPool)}" title="${esc(d.issuerPool)}">${esc(short(d.issuerPool, 10, 4))}</span>`
        : "";
      return sub([
        `<b>${fmtInt(d.txCount)}</b> tx`,
        d.totalOutput ? `<b>${fmtAda(d.totalOutput)}</b> moved` : "",
        d.size ? `${fmtInt(d.size / 1024)} kB` : "",
        pool,
      ]);
    }
    case "transaction":
      return sub([
        `<b>${fmtAda(d.ada)}</b>`,
        `${fmtInt(d.inputs)} in → ${fmtInt(d.outputs)} out`,
        `fee ${fmtAda(d.fee)}`,
        d.script ? `<span class="badge contract">contract</span>` : "",
        d.assets ? `${fmtInt(d.assets)} asset${d.assets > 1 ? "s" : ""}` : "",
      ]);
    case "token_transfer":
      return assetChipsHtml(d.assets);
    case "mint":
      return `<span class="badge plus">mint</span>` + assetChipsHtml(d.assets);
    case "burn":
      return `<span class="badge minus">burn</span>` + assetChipsHtml(d.assets);
    case "delegation": {
      const from = d.fromPool
        ? `<span class="pool-id" data-pool="${esc(d.fromPool)}" title="${esc(d.fromPool)}">${esc(short(d.fromPool, 10, 4))}</span>`
        : (d.stake ? `<span class="hash">${esc(short(d.stake, 12, 5))}</span>` : "");
      const to = d.pool
        ? `<span class="pool-id" data-pool="${esc(d.pool)}" title="${esc(d.pool)}">${esc(short(d.pool, 10, 4))}</span>`
        : "";
      return sub([from && to ? `${from} <span class="sep">→</span> ${to}` : (to || from)]);
    }
    case "vote_delegation": {
      const from = d.fromDrep
        ? drepSpan(d.fromDrep, d.fromDrepName)
        : (d.stake ? `<span class="hash">${esc(short(d.stake, 12, 5))}</span>` : "");
      const to = drepSpan(d.drep, d.drepName);
      return sub([from && to ? `${from} <span class="sep">→</span> ${to}` : (to || from)]);
    }
    case "stake_registration":
    case "stake_deregistration":
      return d.stake ? `<span class="hash">${esc(short(d.stake, 14, 6))}</span>` : "";
    case "withdrawal":
      return sub([
        `<b>${fmtAda(d.lovelace)}</b>`,
        d.account ? `<span class="hash">${esc(short(d.account, 12, 5))}</span>` : "",
      ]);
    case "pool_registration":
      return sub([
        d.pool ? `<span class="pool-id" data-pool="${esc(d.pool)}" title="${esc(d.pool)}">${esc(short(d.pool, 12, 5))}</span>` : "",
        d.pledge ? `pledge <b>${fmtAda(d.pledge)}</b>` : "",
        d.margin ? `margin ${esc(marginPct(d.margin))}` : "",
        d.cost ? `cost ${fmtAda(d.cost)}` : "",
      ]);
    case "pool_retirement":
      return sub([
        d.pool ? `<span class="pool-id" data-pool="${esc(d.pool)}" title="${esc(d.pool)}">${esc(short(d.pool, 12, 5))}</span>` : "",
        d.retirementEpoch != null ? `epoch <b>${esc(String(d.retirementEpoch))}</b>` : "",
      ]);
    case "gov_proposal": {
      const govKey = ev.tx_hash
        ? `${String(ev.tx_hash).toLowerCase()}#${d.index ?? 0}`
        : "";
      return sub([
        d.proposalTitle
          ? `<span class="gov-title" data-gov="${esc(govKey)}" title="${esc(ev.tx_hash || "")}#${esc(String(d.index ?? 0))}">${esc(d.proposalTitle)}</span>`
          : govKey
            ? `<span class="hash" data-gov="${esc(govKey)}" title="${esc(ev.tx_hash || "")}#${esc(String(d.index ?? 0))}">${esc(short(ev.tx_hash, 8, 4))}#${esc(String(d.index ?? 0))}</span>`
            : "",
        d.deposit ? `deposit <b>${fmtAda(d.deposit)}</b>` : "",
        d.anchorUrl ? `<span class="hash">${esc(short(d.anchorUrl, 22, 0))}</span>` : "",
      ]);
    }
    case "gov_vote": {
      const v = String(d.vote || "").toLowerCase();
      const cls = v === "yes" ? "yes" : v === "no" ? "no" : "abstain";
      const govKey = d.proposalTx != null
        ? `${String(d.proposalTx).toLowerCase()}#${d.proposalIndex ?? 0}`
        : "";
      const onProp = d.proposalTitle
        ? `on <span class="gov-title" data-gov="${esc(govKey)}" title="${esc(d.proposalTx || "")}#${esc(String(d.proposalIndex ?? 0))}">${esc(d.proposalTitle)}</span>`
        : d.proposalTx
          ? `on <span class="hash" data-gov="${esc(govKey)}" title="${esc(d.proposalTx)}#${esc(String(d.proposalIndex ?? 0))}">${esc(short(d.proposalTx, 8, 4))}#${esc(String(d.proposalIndex ?? 0))}</span>`
          : "";
      return sub([
        `<span class="badge ${cls}">${esc(v.toUpperCase())}</span>`,
        d.role ? esc(roleLabel(d.role)) : "",
        d.voter ? drepSpan(d.voter, d.voterName) : "",
        onProp,
      ]);
    }
    case "drep_registration":
    case "drep_update":
    case "drep_retirement":
      return d.drep ? drepSpan(d.drep, d.drepName) : "";
    case "tx_metadata":
      return d.msg
        ? `<span style="font-style:italic">“${esc(String(d.msg).slice(0, 160))}”</span>`
        : sub([(d.labels || []).slice(0, 6).map((l) => `<span class="hash">label ${esc(l)}</span>`).join(" ")]);
    case "rollback":
      return sub([
        `<b>${fmtInt(d.depth)}</b> block${d.depth > 1 ? "s" : ""} orphaned`,
        `rolled back to slot <b>${fmtInt(d.toSlot)}</b>`,
      ]);
    case "orphaned_block":
      return sub([
        `<span class="ribbon">orphaned</span>`,
        `<span class="hash">${esc(short(d.hash, 12, 6))}</span>`,
        `slot ${fmtInt(d.slot)}`,
      ]);
    case "slot_battle":
      return sub([
        `<span class="hash">${esc(short(d.winner, 10, 5))}</span> <b>won</b>`,
        `<span class="hash" style="text-decoration:line-through">${esc(short(d.loser, 10, 5))}</span> lost ${esc(d.battle || "slot")} ${fmtInt(d.slot)}`,
      ]);
    case "dex_order":
    case "dex_fill":
    case "dex_lp":
    case "dex_cancel": {
      const flow = dexFlowHtml(d);
      const status = dexStatusPill(ev, d);
      return sub([flow, status]);
    }
    case "dapp_activity": {
      const iag = d.iag != null
        ? `<b>${fmtTokenQty(d.iag, 6)}</b> IAG`
        : "";
      const nodeId = d.nodeId
        ? `node id <span class="hash" title="Node ID">${esc(d.nodeId)}</span>`
        : "";
      const ada = d.ada ? `<b>${fmtAda(d.ada)}</b>` : "";
      return sub([
        `<span class="badge contract">${esc(d.dapp || "dApp")}</span>`,
        nodeId,
        iag,
        ada,
      ]);
    }
    default:
      return esc(ev.summary || "");
  }
}

/** Card kind badge = filter category (title already names the specific event). */
const CAT_LABEL = Object.fromEntries(CATS.map((c) => [c.id, c.label]));

function roleLabel(r) {
  return { delegateRepresentative: "DRep", constitutionalCommittee: "CC", stakePoolOperator: "SPO" }[r] || r;
}
function marginPct(m) {
  if (typeof m === "string" && m.includes("/")) {
    const [a, b] = m.split("/").map(Number);
    if (b) return (100 * a / b).toFixed(1) + "%";
  }
  return String(m);
}

function poolIdsFromData(d) {
  if (!d || typeof d !== "object") return [];
  return [d.issuerPool, d.pool, d.fromPool].filter((id) => typeof id === "string" && id);
}

function isLookupDrepId(id) {
  return typeof id === "string"
    && (id.startsWith("drep1") || id.startsWith("drep_script1"))
    && id.length >= 50
    && id.length <= 120;
}

/** Prefer stamped/cached givenName; enrichDreps fills misses via /api/drep. */
function drepSpan(id, stampedName) {
  if (!id) return "";
  const s = String(id);
  if (s === "Always Abstain" || s === "Always No Confidence") {
    return `<b title="${esc(s)}">${esc(s)}</b>`;
  }
  if (!isLookupDrepId(s)) {
    const label = s.length > 24 ? short(s, 10, 5) : s;
    return `<span class="hash" title="${esc(s)}">${esc(label)}</span>`;
  }
  const known = (typeof stampedName === "string" && stampedName)
    || drepMeta.get(s)?.name
    || "";
  if (known) {
    if (!drepMeta.get(s)?.name) drepMeta.set(s, { name: known });
    return `<span class="drep-name" data-drep="${esc(s)}" title="${esc(s)}">${esc(known)}</span>`;
  }
  return `<span class="drep-id" data-drep="${esc(s)}" title="${esc(s)}">${esc(short(s, 10, 5))}</span>`;
}

function drepIdsFromData(d) {
  if (!d || typeof d !== "object") return [];
  return [d.drep, d.fromDrep, d.voter].filter(isLookupDrepId);
}

/** Build the lowercase search haystack for a card (includes pool ids + known tickers). */
function cardSearchText(ev) {
  const d = ev.data || {};
  const bits = [ev.title, ev.tx_hash, ev.block_hash, ev.kind, JSON.stringify(d)];
  for (const id of poolIdsFromData(d)) {
    bits.push(id);
    const meta = poolMeta.get(id);
    if (meta?.ticker) bits.push(meta.ticker);
    if (meta?.name) bits.push(meta.name);
  }
  for (const id of drepIdsFromData(d)) {
    bits.push(id);
    const meta = drepMeta.get(id);
    if (meta?.name) bits.push(meta.name);
  }
  // Asset registry tickers already fetched into assetMeta.
  const assetLists = [d.assets?.items, d.want?.items].filter(Array.isArray);
  for (const items of assetLists) {
    for (const a of items) {
      if (a?.name) bits.push(a.name);
      const unit = a?.unit;
      if (unit && assetMeta.has(unit)) {
        const m = assetMeta.get(unit);
        if (m?.ticker) bits.push(m.ticker);
        if (m?.name) bits.push(m.name);
      }
    }
  }
  return bits.filter(Boolean).join(" ").toLowerCase().slice(0, 4000);
}

/** Block hashes marked orphaned — hidden with Forks & Battles filter off. */
const orphanedBlocks = new Set();

/** Index one event into the client-side 24h retention cache. */
function retentionIndex(ev) {
  if (!ev || ev.id == null) return;
  if (!keepDexEvent(ev)) return;
  if (ev.kind === "orphaned_block" && ev.block_hash) {
    orphanedBlocks.add(ev.block_hash);
  }
  retentionCache.set(ev.id, { ev, hay: cardSearchText(ev) });
  indexTxGraph(ev);
  noteLoadedEvent(ev);
}

function retentionTrim() {
  if (!retentionHours || retentionCache.size === 0) return;
  const cutoff = Math.floor(Date.now() / 1000) - retentionHours * 3600;
  for (const [id, row] of retentionCache) {
    if ((row.ev.timestamp || 0) < cutoff) {
      retentionCache.delete(id);
      unindexTxGraph(row.ev);
      forgetLoadedEvent(row.ev);
    }
  }
}

function notifyRetentionReady() {
  retentionReady = true;
  const waiters = retentionWaiters.splice(0);
  for (const w of waiters) w();
}

function whenRetentionReady() {
  if (retentionReady) return Promise.resolve();
  return new Promise((resolve) => retentionWaiters.push(resolve));
}

/**
 * Background-load the full 24h window into retentionCache.
 * The feed stays small - pages are served from this cache on scroll.
 */
function startRetentionPreload(force = false) {
  if (!force && retentionLoading) return retentionLoading;
  const gen = ++retentionLoadGen;
  retentionReady = false;
  feedHitBuffer = [];
  feedHitOffset = 0;
  feedHitKey = "";
  retentionHistoryDone = false;
  retentionLoading = (async () => {
    try {
      const r = await fetch("/api/buffer");
      if (!r.ok) throw new Error("buffer fetch failed");
      if (gen !== retentionLoadGen) return;
      const m = await r.json();
      if (gen !== retentionLoadGen) return;
      if (m.buffered != null) bufferedEventCount = Number(m.buffered) || bufferedEventCount;
      if (m.retention_hours != null) retentionHours = Number(m.retention_hours) || retentionHours;
      const events = m.events || [];
      for (const ev of events) retentionIndex(ev);
      retentionTrim();
      bufferedEventCount = Math.max(bufferedEventCount, retentionCache.size);
      updateLoadedEventCount();
      paintCatCounts();
      refreshActivityMonitor();
    } catch (e) {
      if (gen === retentionLoadGen) console.warn("retention preload failed", e);
    } finally {
      if (gen === retentionLoadGen) {
        notifyRetentionReady();
        retentionLoading = null;
        queueMicrotask(() => onFeedFiltersChanged());
      }
    }
  })();
  return retentionLoading;
}

/** Category / gov-type / DEX venue / min-ADA (search text is separate). */
function eventPassesFeedFilters(ev) {
  if (!ev || !keepDexEvent(ev)) return false;
  if (!settings.filters[ev.category]) return false;
  // Orphaned-block content is gated by Forks & Battles (not only alert cards).
  if (
    !settings.filters.alert
    && ev.block_hash
    && orphanedBlocks.has(ev.block_hash)
  ) {
    return false;
  }
  if (ev.category === "dex" && !dexVenueEnabled(ev.data?.dex)) return false;
  if (ev.category === "dapp" && !dappAppEnabled(ev.data?.dapp)) return false;
  if (ev.category === "governance") {
    const gt = govTypeKey(ev);
    if (gt && !govTypeEnabled(gt)) return false;
  }
  if (settings.minAda > 0 && ev.category === "transaction") {
    if (Number(ev.data?.ada || 0) < settings.minAda * 1e6) return false;
  }
  return true;
}

function feedFilterKey() {
  return JSON.stringify({
    f: settings.filters,
    g: settings.govTypes,
    d: settings.dexVenues,
    a: settings.dappApps,
    m: settings.minAda,
  });
}

/** Newest-first by slot (chain order), then id within the same slot. */
function sortEventsNewestFirst(events) {
  events.sort((a, b) => (b.slot || 0) - (a.slot || 0) || (b.id || 0) - (a.id || 0));
  return events;
}

function cmpSlotIdDesc(slotA, idA, slotB, idB) {
  if (slotB !== slotA) return slotB - slotA;
  return idB - idA;
}

/** Keep group sort keys in sync with the newest card they contain. */
function syncGroupSortKey(g) {
  let bestSlot = 0;
  let bestId = 0;
  g.querySelectorAll(".card").forEach((card) => {
    const slot = Number(card.dataset.slot || 0);
    const id = Number(card.dataset.eid || 0);
    if (cmpSlotIdDesc(bestSlot, bestId, slot, id) > 0) {
      bestSlot = slot;
      bestId = id;
    }
  });
  // Fall back to whatever was stamped at creation if cards aren't mounted yet.
  if (!bestSlot) {
    bestSlot = Number(g.dataset.slot || 0);
    bestId = Number(g.dataset.eid || 0);
  }
  g.dataset.slot = String(bestSlot);
  g.dataset.eid = String(bestId);
}

/**
 * Re-order every block-group (and cards within) by slot.
 * This is the source of truth - insertion path must never define feed order.
 */
/**
 * Order cards within a block group as a containment hierarchy: the block first,
 * then each transaction immediately followed by its own child events (mint,
 * transfer, swap, …), transactions in chain (id) order. A child is keyed by its
 * parent transaction's id so it always clusters directly under that parent, even
 * for DEX/dApp events that arrive at the end of the block.
 */
function hierKey(card) {
  const eid = Number(card.dataset.eid || 0);
  const kind = card.dataset.kind;
  if (kind === "block") return [-Infinity, 0, eid];
  if (kind === "transaction") return [eid, 0, eid];
  const parent = card.dataset.parent;
  if (parent) return [Number(parent), 1, eid];
  return [eid, 0, eid];
}

function cmpHierarchy(a, b) {
  const ka = hierKey(a), kb = hierKey(b);
  return (ka[0] - kb[0]) || (ka[1] - kb[1]) || (ka[2] - kb[2]);
}

function resortFeedBySlot() {
  // Re-appending every group resets the browser's scroll anchoring and reads as
  // a flash when history pages land under sparse filters. Pin the viewport.
  const y = window.scrollY;
  const x = feed.scrollLeft;
  const items = [...feed.querySelectorAll(":scope > .block-group")];
  for (const g of items) {
    const host = g.querySelector(".group-events");
    if (host) {
      const cards = [...host.querySelectorAll(":scope > .card")];
      cards.sort(cmpHierarchy);
      for (const c of cards) host.appendChild(c);
    }
    syncGroupSortKey(g);
  }
  items.sort((a, b) => cmpSlotIdDesc(
    Number(a.dataset.slot || 0),
    Number(a.dataset.eid || 0),
    Number(b.dataset.slot || 0),
    Number(b.dataset.eid || 0),
  ));
  for (const g of items) feed.appendChild(g);
  groupOrder.length = 0;
  groupOrder.push(...items);
  pinHistoryLoader();
  if (settings.layout === "vertical") {
    if (window.scrollY !== y) window.scrollTo(0, y);
  } else if (feed.scrollLeft !== x) {
    feed.scrollLeft = x;
  }
}

let feedResortQueued = false;
/** When true, routeEvent skips scheduling a resort (caller will resort once). */
let suppressFeedResort = false;
function scheduleFeedResort() {
  if (suppressFeedResort) return;
  if (feedResortQueued) return;
  feedResortQueued = true;
  queueMicrotask(() => {
    feedResortQueued = false;
    if (!suppressFeedResort) resortFeedBySlot();
  });
}

/** Newest-first events from the 24h cache that pass the current feed filters. */
function collectFeedHits() {
  const hits = [];
  for (const { ev } of retentionCache.values()) {
    if (eventPassesFeedFilters(ev)) hits.push(ev);
  }
  return sortEventsNewestFirst(hits);
}

function syncFeedHitOffset() {
  feedHitOffset = 0;
  while (
    feedHitOffset < feedHitBuffer.length
    && seenEventIds.has(feedHitBuffer[feedHitOffset].id)
  ) {
    feedHitOffset++;
  }
  retentionHistoryDone = feedHitOffset >= feedHitBuffer.length;
}

function ensureFeedHitBuffer(force = false) {
  const key = feedFilterKey();
  if (!force && key === feedHitKey && feedHitBuffer.length) {
    syncFeedHitOffset();
    return;
  }
  feedHitKey = key;
  feedHitBuffer = collectFeedHits();
  syncFeedHitOffset();
}

/**
 * Render the next FEED_PAGE_SIZE *new* matches from feedHitBuffer into the DOM.
 * Returns how many cards were added.
 */
function renderFeedPage() {
  const batch = [];
  while (batch.length < FEED_PAGE_SIZE && feedHitOffset < feedHitBuffer.length) {
    const ev = feedHitBuffer[feedHitOffset++];
    if (ev?.id == null || seenEventIds.has(ev.id)) continue;
    batch.push(ev);
  }
  retentionHistoryDone = feedHitOffset >= feedHitBuffer.length;
  if (!batch.length) return 0;
  // One resort after the batch — per-event scheduleFeedResort was a major flash.
  suppressFeedResort = true;
  try {
    for (const ev of batch) routeEvent(ev);
  } finally {
    suppressFeedResort = false;
  }
  resortFeedBySlot();
  prefetchUnitsFromEvents(batch);
  applyFilters();
  return batch.length;
}

/** Drop painted cards but keep the retention cache (filter / order rebuild). */
function clearFeedDom() {
  clearLightCone();
  feed.querySelectorAll(".block-group").forEach((g) => g.remove());
  groups.clear();
  groupOrder.length = 0;
  seenEventIds.clear();
  oldestEventId = null;
  feedHitOffset = 0;
  retentionHistoryDone = false;
  pinHistoryLoader();
}

/**
 * After retention preload or a filter change: rebuild the visible feed from the
 * slot-sorted hit buffer so cards never sit in the wrong chain order.
 */
function onFeedFiltersChanged() {
  if ($("search").value.trim()) return;
  if (!retentionReady) return;
  historySparsePause = false;
  historyLoadArmed = true;
  ensureFeedHitBuffer(true);
  clearFeedDom();
  while (!visibleFeedFillsPage() && feedHitOffset < feedHitBuffer.length) {
    if (!renderFeedPage()) break;
  }
  resortFeedBySlot();
  applyFilters();
}

/** Local text search over the preloaded 24h cache. Returns newest-first matches. */
function searchRetentionLocal(query) {
  const q = String(query || "").trim().toLowerCase();
  if (!q) return [];
  const hits = [];
  for (const { ev, hay } of retentionCache.values()) {
    if (!eventPassesFeedFilters(ev)) continue;
    if (hay.includes(q)) hits.push(ev);
  }
  return sortEventsNewestFirst(hits);
}

function appendCardSearch(card, ...parts) {
  if (!card) return;
  const extra = parts.filter(Boolean).join(" ").toLowerCase().trim();
  if (!extra) return;
  const cur = card.dataset.search || "";
  // Avoid growing forever on repeated paint
  for (const token of extra.split(/\s+/)) {
    if (token && !cur.includes(token)) {
      card.dataset.search = (card.dataset.search || "") + " " + token;
    }
  }
  card.dataset.search = (card.dataset.search || "").slice(0, 4000);
}

let filterRefreshTimer = 0;
function scheduleFilterRefresh() {
  clearTimeout(filterRefreshTimer);
  filterRefreshTimer = setTimeout(applyFilters, 80);
}

function buildCard(ev) {
  const card = document.createElement("article");
  card.className = "card" + (ev.kind === "block" ? " card-block" : "");
  card.dataset.category = ev.category;
  card.dataset.kind = ev.kind;
  if (ev.id != null) card.dataset.eid = String(ev.id);
  if (ev.parent_id != null) card.dataset.parent = String(ev.parent_id);
  // A tx-scoped event (mint, transfer, swap, …) is "part of" its transaction:
  // indent it under its parent. Transactions and blocks stay at the base level.
  if (ev.parent_id != null && ev.kind !== "block" && ev.kind !== "transaction") {
    card.classList.add("ev-child");
  }
  if (ev.slot != null) card.dataset.slot = String(ev.slot);
  if (ev.tx_hash) card.dataset.tx = ev.tx_hash;
  if (ev.data && ev.data.ada != null) card.dataset.ada = ev.data.ada;
  if (ev.category === "dex" && ev.data?.dex) card.dataset.dex = String(ev.data.dex);
  if (ev.category === "dapp" && ev.data?.dapp) card.dataset.dapp = String(ev.data.dapp);
  if (ev.category === "governance") {
    const gt = govTypeKey(ev);
    if (gt) card.dataset.govType = gt;
  }

  const title = ev.kind === "block"
    ? `Block <span class="height">${fmtInt(ev.height)}</span>`
    : esc(titleCaseWords(ev.kind === "vote_delegation" ? "DRep Delegation" : ev.title));

  card.innerHTML = `
    <div class="ev-icon">${iconFor(ev.kind, ev.category, ev.data?.side, ev.data?.vote)}</div>
    <div class="ev-body">
      <div class="ev-head">
        <span class="ev-title">${title}</span>
        <span class="ev-kind">${esc(CAT_LABEL[ev.category] || ev.category)}</span>
        <span class="ev-time" data-ts="${ev.timestamp}" title="${esc(clock(ev.timestamp))}">${timeAgo(ev.timestamp)}</span>
      </div>
      <div class="ev-sub">${cardBody(ev)}</div>
    </div>`;

  card.dataset.search = cardSearchText(ev);

  card.addEventListener("click", () => openModal(ev));
  enrichAssets(card);
  enrichPools(card);
  enrichDreps(card);
  enrichGovActions(card);
  return card;
}

/* ── Feed assembly: block groups on the chain spine ───────────────────── */

function newGroup(blockHash, ev) {
  const g = document.createElement("div");
  g.className = "block-group";
  if (blockHash) g.dataset.block = blockHash;
  g.dataset.slot = String(ev?.slot || 0);
  g.dataset.eid = String(ev?.id || 0);
  const evs = document.createElement("div");
  evs.className = "group-events";
  g.appendChild(evs);
  // Temporary placement; scheduleFeedResort() establishes final slot order.
  feed.prepend(g);
  groupOrder.unshift(g);
  if (blockHash) groups.set(blockHash, g);
  while (groupOrder.length > MAX_GROUPS) {
    const old = groupOrder.pop();
    if (old?.dataset.block) groups.delete(old.dataset.block);
    old?.remove();
  }
  pinHistoryLoader();
  return g;
}

function noteEventId(ev) {
  if (ev?.id == null) return;
  seenEventIds.add(ev.id);
  if (oldestEventId == null || ev.id < oldestEventId) oldestEventId = ev.id;
  // History scrolled in from before the retention window stays in the totals
  // even after retentionTrim drops it from the search index.
  if (retentionHours && ev.timestamp != null) {
    const cutoff = Math.floor(Date.now() / 1000) - retentionHours * 3600;
    if (ev.timestamp < cutoff) extraHistoryIds.add(ev.id);
  }
  retentionIndex(ev);
}

function routeEvent(ev) {
  if (!keepDexEvent(ev)) return;
  if (ev?.id != null && seenEventIds.has(ev.id)) return;
  sessionEvents++;
  noteEventId(ev);

  if (ev.kind === "block") {
    let g = ev.block_hash ? groups.get(ev.block_hash) : null;
    if (!g) g = newGroup(ev.block_hash, ev);
    g.prepend(buildCard(ev));
    scheduleFeedResort();
    return;
  }

  if (ev.kind === "orphaned_block") {
    const g = ev.block_hash && groups.get(ev.block_hash);
    if (g) {
      g.classList.add("orphaned");
      const bc = g.querySelector(".card-block .ev-head");
      if (bc && !bc.querySelector(".ribbon")) {
        const r = document.createElement("span");
        r.className = "ribbon";
        r.textContent = "orphaned";
        bc.insertBefore(r, bc.querySelector(".ev-time"));
      }
    }
    standaloneCard(ev);
    return;
  }

  if (ev.kind === "rollback" || ev.kind === "slot_battle") {
    standaloneCard(ev);
    return;
  }

  let g = ev.block_hash ? groups.get(ev.block_hash) : null;
  if (!g) g = newGroup(ev.block_hash, ev);
  g.querySelector(".group-events").appendChild(buildCard(ev));
  scheduleFeedResort();
}

function standaloneCard(ev) {
  const g = newGroup(null, ev);
  g.querySelector(".group-events").appendChild(buildCard(ev));
  scheduleFeedResort();
}

/**
 * Insert a page of events. Final order always comes from resortFeedBySlot().
 */
function routeHistoricalBatch(events) {
  const anchors = new Map();
  suppressFeedResort = true;
  try {
    for (const ev of events) {
      if (!keepDexEvent(ev)) continue;
      if (ev?.id != null && seenEventIds.has(ev.id)) continue;
      sessionEvents++;
      noteEventId(ev);

      if (ev.kind === "rollback" || ev.kind === "slot_battle" || ev.kind === "orphaned_block") {
        if (ev.kind === "orphaned_block") {
          const g = ev.block_hash && groups.get(ev.block_hash);
          if (g) g.classList.add("orphaned");
        }
        standaloneCard(ev);
        continue;
      }

      const key = ev.block_hash || `__id_${ev.id}`;
      let g = ev.block_hash ? groups.get(ev.block_hash) : null;
      const created = !g;
      if (!g) g = newGroup(ev.block_hash, ev);

      if (ev.kind === "block") {
        if (!g.querySelector(".card-block")) {
          g.insertBefore(buildCard(ev), g.querySelector(".group-events"));
        }
        continue;
      }

      const host = g.querySelector(".group-events");
      const card = buildCard(ev);
      if (created) {
        host.appendChild(card);
      } else {
        if (!anchors.has(key)) anchors.set(key, host.firstChild);
        host.insertBefore(card, anchors.get(key) || null);
      }
    }
  } finally {
    suppressFeedResort = false;
  }
  resortFeedBySlot();
}

/* min-ADA / search / category filters must also apply to fresh cards */
const applySoon = (() => {
  let t;
  return () => { clearTimeout(t); t = setTimeout(applyFilters, 120); };
})();

/* ── Pause-on-read buffering ──────────────────────────────────────────── */

function isPaused() {
  return settings.layout === "vertical"
    ? window.scrollY > 90
    : feed.scrollLeft > 60;
}

/** Pending tip events that match the current filters (drives the "n new" pill). */
function pendingVisibleCount() {
  const q = $("search").value.trim().toLowerCase();
  let n = 0;
  for (const ev of pending) {
    if (!eventPassesFeedFilters(ev)) continue;
    if (q && !cardSearchText(ev).includes(q)) continue;
    n++;
  }
  return n;
}

function updateNewPill() {
  const el = $("newpill");
  const nEl = $("newpill-n");
  if (!el || !nEl) return;
  const n = pendingVisibleCount();
  if (n > 0) {
    nEl.textContent = fmtInt(n);
    el.classList.add("show");
  } else {
    el.classList.remove("show");
  }
}

function onEvent(ev) {
  if (isPaused()) {
    pending.push(ev);
    if (pending.length > 800) pending.shift();
    updateNewPill();
  } else {
    routeEvent(ev);
    applySoon();
  }
}

function flushPending() {
  if (!pending.length) return;
  suppressFeedResort = true;
  try {
    while (pending.length) routeEvent(pending.shift());
  } finally {
    suppressFeedResort = false;
  }
  resortFeedBySlot();
  updateNewPill();
  applySoon();
}

/** Debounced tip-unpause flush — avoids a flash of N inserts when rubber-banding to top. */
let flushPendingTimer = 0;
function scheduleFlushPending() {
  if (isPaused() || !pending.length) return;
  clearTimeout(flushPendingTimer);
  flushPendingTimer = setTimeout(() => {
    if (!isPaused() && pending.length) flushPending();
  }, 280);
}

$("newpill").onclick = () => {
  if (settings.layout === "vertical") window.scrollTo({ top: 0, behavior: "smooth" });
  else feed.scrollTo({ left: 0, behavior: "smooth" });
  setTimeout(flushPending, 350);
};

/** Only load older pages while scrolling toward history (not back to tip). */
let lastScrollPos = 0;
/**
 * Approaching the history end consumes historyLoadArmed for one load. It
 * re-arms only after the user scrolls back out of the end zone — so appending
 * a page cannot immediately chain-trigger the next page (the flash/load loop).
 */
function feedScrollPos() {
  return settings.layout === "vertical" ? window.scrollY : feed.scrollLeft;
}
/** @returns {boolean} true once when the user newly arrives in the end zone */
function consumeHistoryLoadArm() {
  if (!nearHistoryEnd()) {
    historyLoadArmed = true;
    return false;
  }
  if (!historyLoadArmed) return false;
  historyLoadArmed = false;
  return true;
}
function onScrollDirection() {
  const pos = feedScrollPos();
  // Require clear movement toward history — ignore jitter / overscroll bounce.
  const towardHistory = pos > lastScrollPos + 2;
  const towardTip = pos < lastScrollPos - 2;
  lastScrollPos = pos;
  if (towardTip) scheduleFlushPending();
  if (!towardHistory) {
    if (!nearHistoryEnd()) historyLoadArmed = true;
    return;
  }
  const q = $("search").value.trim();
  if (q) {
    if (searchPriming || searchExtending) return;
    if (!visibleFeedFillsPage() || consumeHistoryLoadArm()) extendSearchHistory();
    return;
  }
  if (consumeHistoryLoadArm()) scheduleLoadHistory();
}
addEventListener("scroll", onScrollDirection, { passive: true });
feed.addEventListener("scroll", onScrollDirection, { passive: true });

// Feeds that don't fill the viewport can't scroll - treat wheel-down as load-more.
addEventListener("wheel", (e) => {
  if (e.deltaY <= 0) return;
  if (searchPriming || searchExtending) return;
  if (visibleFeedFillsPage()) return;
  // Short view: one wheel burst → one fill attempt (latch re-arms when short).
  if (!historyLoadArmed) return;
  historyLoadArmed = false;
  if ($("search").value.trim()) extendSearchHistory();
  else scheduleLoadHistory();
}, { passive: true });

/** True only at the last ~64px of the feed (not hundreds of px early). */
function nearHistoryEnd() {
  if (settings.layout === "vertical") {
    const scrollable = document.documentElement.scrollHeight - window.innerHeight;
    // Not scrollable yet — leave fill-on-wheel to the wheel handler, otherwise
    // rubber-band / layout thrash would spam history loads in both directions.
    if (scrollable < 48) return false;
    return scrollable - window.scrollY < 64;
  }
  const scrollable = feed.scrollWidth - feed.clientWidth;
  if (scrollable < 48) return false;
  return scrollable - feed.scrollLeft < 64;
}

/** True when visible (filter-matching) cards fill at least one viewport. */
function visibleFeedFillsPage() {
  let n = 0;
  document.querySelectorAll("#feed .card").forEach((card) => {
    if (card.classList.contains("f-hide")) return;
    if (!settings.filters[card.dataset.category]) return;
    if (card.dataset.category === "dex" && !dexVenueEnabled(card.dataset.dex)) return;
    if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return;
    if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return;
    n++;
  });
  if (!n) return false;
  if (settings.layout === "vertical") {
    return document.documentElement.scrollHeight >= window.innerHeight + 48;
  }
  return feed.scrollWidth >= feed.clientWidth + 48;
}

function setSearchPrime(on) {
  const el = $("search-prime");
  if (!el) return;
  el.hidden = !on;
  document.body.classList.toggle("search-priming", !!on);
  if (on) hideSearchPrompts();
}

function visibleMatchCount() {
  let n = 0;
  document.querySelectorAll("#feed .card:not(.f-hide)").forEach((card) => {
    if (!settings.filters[card.dataset.category]) return;
    if (card.dataset.category === "dex" && !dexVenueEnabled(card.dataset.dex)) return;
    if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return;
    if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return;
    n++;
  });
  return n;
}

function setSearchEmpty(on, scanned = searchScanned) {
  const el = $("search-empty");
  if (!el) return;
  const btn = $("search-empty-more");
  const text = $("search-empty-t");
  if (!on || !$("search").value.trim()) {
    el.classList.remove("show");
    el.hidden = true;
    return;
  }
  el.hidden = false;
  el.classList.add("show");
  const n = Math.max(1, scanned || SEARCH_PRIME_LOOKBACK);
  const windowLabel = searchWindowLabel(n);
  if (text) {
    text.textContent = `No results found in ${windowLabel}.`;
  }
  // Search is local over the preloaded 24h index - no server history crawl.
  if (btn) btn.hidden = true;
}

function setSearchMore(on, scanned = searchScanned, matches = 0) {
  const el = $("search-more");
  if (!el) return;
  const btn = $("search-more-btn");
  const text = $("search-more-t");
  if (!on || !$("search").value.trim()) {
    el.classList.remove("show");
    el.hidden = true;
    return;
  }
  el.hidden = false;
  el.classList.add("show");
  const n = Math.max(1, scanned || SEARCH_PRIME_LOOKBACK);
  const windowLabel = searchWindowLabel(n);
  const total = searchMatchesTotal > 0 ? searchMatchesTotal : matches;
  const showing = `${fmtInt(matches)} of ${fmtInt(total)} match${total === 1 ? "" : "es"}`;
  if (text) {
    text.textContent = `${showing} in ${windowLabel}.`;
  }
  if (btn) {
    if (searchHitOffset < searchHitBuffer.length) {
      btn.hidden = false;
      btn.disabled = false;
      btn.textContent = "Load more matches";
    } else {
      btn.hidden = true;
    }
  }
}

function coveredRetentionWindow(scanned) {
  return bufferedEventCount > 0 && scanned >= bufferedEventCount;
}

/** Human label for how far a search has looked: prefer “past 24 hours” once the buffer is covered. */
function searchWindowLabel(scanned) {
  if (coveredRetentionWindow(scanned) && retentionHours > 0) {
    const h = retentionHours === 1 ? "1 hour" : `${retentionHours} hours`;
    return `the past ${h} (${fmtInt(scanned)} events)`;
  }
  return `the past ${fmtInt(scanned)} events`;
}

function searchLookbackTarget() {
  // Cover the full in-memory retention window, not just the tip snapshot (~25).
  return Math.max(SEARCH_PRIME_LOOKBACK, bufferedEventCount || 0);
}

function hideSearchPrompts() {
  setSearchEmpty(false);
  setSearchMore(false);
}

/** Pool/DRep/asset metadata still resolving - search haystacks may gain labels shortly. */
function enrichmentPending() {
  return poolWaiters.size > 0 || drepWaiters.size > 0;
}

/** Don't declare "no results" until this time (ms) while pool tickers may still land. */
let searchEmptyGraceUntil = 0;
const SEARCH_EMPTY_GRACE_MS = 2000;

function armSearchEmptyGrace() {
  searchEmptyGraceUntil = Date.now() + SEARCH_EMPTY_GRACE_MS;
}

let emptyRecheckTimer = 0;
function scheduleEmptyRecheck() {
  clearTimeout(emptyRecheckTimer);
  const delay = Math.max(50, Math.min(150, searchEmptyGraceUntil - Date.now()));
  emptyRecheckTimer = setTimeout(() => {
    applyFilters();
    updateSearchEmptyPrompt();
  }, delay);
}

/**
 * Wait out in-flight pool metadata so ticker searches aren't judged too early.
 * Stops early once any match is visible; caps so slow /api/pool can't strand us.
 */
async function waitForEnrichment(gen, timeoutMs = SEARCH_EMPTY_GRACE_MS) {
  const start = Date.now();
  while (
    gen === searchPrimeGen
    && enrichmentPending()
    && Date.now() - start < timeoutMs
  ) {
    applyFilters();
    if (visibleMatchCount() > 0) break;
    await new Promise((r) => setTimeout(r, 50));
  }
}

function updateSearchEmptyPrompt() {
  // Don't show the summary while the initial match list is still loading.
  if (searchPriming) {
    hideSearchPrompts();
    return;
  }
  const q = $("search").value.trim();
  if (!q) {
    hideSearchPrompts();
    return;
  }
  // Raw history crawls must not run during search; if a spinner is up, clear it.
  if (historyLoading) {
    abortHistoryLoad();
  }
  const matches = visibleMatchCount();
  if (matches > 0) {
    setSearchEmpty(false);
    if (!searchPriming) setSearchPrime(false);
    if (searchScanned > 0 || searchMatchesTotal > 0) {
      setSearchMore(true, searchScanned, matches);
      if (searchExtending) {
        const btn = $("search-more-btn");
        if (btn) {
          btn.disabled = true;
          btn.textContent = "Loading…";
        }
      }
    } else {
      setSearchMore(false);
    }
    return;
  }
  setSearchMore(false);
  if (searchScanned <= 0 && searchMatchesTotal <= 0) {
    setSearchEmpty(false);
    return;
  }
  // Hold off while tickers/names are still resolving - but only for a short grace.
  if (enrichmentPending() && Date.now() < searchEmptyGraceUntil) {
    setSearchEmpty(false);
    setSearchPrime(true);
    scheduleEmptyRecheck();
    return;
  }
  setSearchPrime(false);
  setSearchEmpty(true, searchScanned);
  if (searchExtending) {
    const btn = $("search-empty-more");
    if (btn) {
      btn.disabled = true;
      btn.textContent = "Loading…";
    }
  }
}

function abortHistoryLoad() {
  if (historyAbort) {
    try { historyAbort.abort(); } catch { /* ignore */ }
    historyAbort = null;
  }
  historyLoading = false;
  setHistoryLoading(false);
}

function cancelSearchPrime() {
  searchPrimeGen++;
  if (historyAbort) {
    try { historyAbort.abort(); } catch { /* ignore */ }
    historyAbort = null;
  }
  searchPriming = false;
  historyLoading = false;
  searchScanned = 0;
  searchMatchesTotal = 0;
  searchHitBuffer = [];
  searchHitOffset = 0;
  searchHitsExhausted = false;
  searchEmptyGraceUntil = 0;
  clearTimeout(emptyRecheckTimer);
  setSearchPrime(false);
  hideSearchPrompts();
  applyFilters();
  queueMicrotask(() => maybeLoadHistory());
}

/**
 * Load the next page of search matches from the in-memory hit buffer.
 */
async function extendSearchHistory() {
  if (searchPriming || searchExtending) return;
  if (!$("search").value.trim()) return;
  if (historyLoading) abortHistoryLoad();

  if (searchHitOffset < searchHitBuffer.length) {
    renderSearchPage();
    applyFilters();
    updateSearchEmptyPrompt();
    return;
  }
  searchHitsExhausted = true;
  updateSearchEmptyPrompt();
}

function scheduleLoadHistory() {
  clearTimeout(historyLoadTimer);
  historyLoadTimer = setTimeout(maybeLoadHistory, 120);
}

function maybeLoadHistory() {
  // Active search pages matches from searchHitBuffer only.
  if ($("search").value.trim()) return;
  if (searchPriming || searchExtending || historyLoading || oldestEventId == null) return;
  if (historySparsePause) return;
  // Nothing left in the 24h page buffer and disk history is exhausted.
  if (retentionHistoryDone && historyExhausted) return;
  // Load when the filtered view is short, or the user scrolled near the end.
  if (visibleFeedFillsPage() && !nearHistoryEnd()) return;
  loadHistory();
}

/** Pin viewport across a DOM mutation that would otherwise jump to the new end. */
async function withPinnedFeedScroll(fn) {
  const y = window.scrollY;
  const x = feed.scrollLeft;
  const result = await fn();
  const restore = () => {
    if (settings.layout === "vertical") {
      if (window.scrollY !== y) window.scrollTo(0, y);
    } else if (feed.scrollLeft !== x) {
      feed.scrollLeft = x;
    }
  };
  restore();
  requestAnimationFrame(restore);
  return result;
}

/**
 * Load older events. Two modes:
 *  - Short filtered view: drain pages until the viewport fills (one session).
 *  - Normal scroll-near-end: append exactly one more page (buffer or disk).
 * The scroll latch ensures a load cannot chain into the next page until the
 * user scrolls back through the newly loaded content.
 */
async function loadHistory() {
  if (searchPriming) return;
  if ($("search").value.trim()) return;
  if (historyLoading) return;

  if (!retentionReady) {
    startRetentionPreload();
    setHistoryLoading(true);
    await whenRetentionReady();
    if ($("search").value.trim()) {
      setHistoryLoading(false, historyExhausted);
      return;
    }
  }

  historyLoading = true;
  ensureFeedHitBuffer();
  const beforeVisible = visibleMatchCount();

  // Short filtered view: drain in-memory pages until the viewport fills (no spinner).
  await withPinnedFeedScroll(() => {
    while (feedHitOffset < feedHitBuffer.length && !visibleFeedFillsPage()) {
      if (!renderFeedPage()) break;
    }
  });

  // Viewport already full: append exactly one more buffer page.
  if (feedHitOffset < feedHitBuffer.length && visibleFeedFillsPage()) {
    await withPinnedFeedScroll(() => { renderFeedPage(); });
    historyLoading = false;
    setHistoryLoading(false, false);
    // Stay disarmed while still in the end zone; re-arm when user scrolls up.
    if (!nearHistoryEnd()) historyLoadArmed = true;
    return;
  }

  // More buffer left but nothing to do this tick (not near end / still filling).
  if (feedHitOffset < feedHitBuffer.length) {
    historyLoading = false;
    setHistoryLoading(false, false);
    if (!visibleFeedFillsPage()) historyLoadArmed = true;
    return;
  }

  // Past the 24h filtered buffer — disk history.
  retentionHistoryDone = true;
  if (historyExhausted) {
    historyLoading = false;
    setHistoryLoading(false, true);
    return;
  }

  setHistoryLoading(true);
  const ac = new AbortController();
  historyAbort = ac;
  try {
    if (!visibleFeedFillsPage()) {
      // Sparse filters: crawl until the view fills or we stop making progress.
      let emptyStreak = 0;
      while (!visibleFeedFillsPage() && !historyExhausted && emptyStreak < 10) {
        const before = visibleMatchCount();
        const events = await withPinnedFeedScroll(() => fetchHistoryPage(ac.signal));
        if (events == null) break;
        if (visibleMatchCount() <= before) emptyStreak++;
        else emptyStreak = 0;
      }
      if (!visibleFeedFillsPage() && !historyExhausted && emptyStreak >= 10) {
        historySparsePause = true;
      }
      historyLoadArmed = true; // short view may need another wheel burst
    } else {
      // Normal infinite scroll — one disk page, pinned so we don't jump to its end.
      const events = await withPinnedFeedScroll(() => fetchHistoryPage(ac.signal));
      if (events == null || visibleMatchCount() <= beforeVisible) {
        // Nothing new visible (filtered out or exhausted) — allow a retry scroll,
        // but stop completely when the server says we're done.
        if (!historyExhausted) historyLoadArmed = true;
      } else if (!nearHistoryEnd()) {
        historyLoadArmed = true;
      }
      // else: still in end zone after pin — stay disarmed until user scrolls up
    }
  } catch (e) {
    if (e?.name !== "AbortError") { /* best-effort */ }
    historyLoadArmed = true;
  }
  if (historyAbort === ac) historyAbort = null;
  historyLoading = false;
  setHistoryLoading(false, historyExhausted);
}

/** Fetch one older disk page into the feed. Returns events, or null on abort/empty. */
async function fetchHistoryPage(signal) {
  if (oldestEventId == null || historyExhausted) return null;
  const r = await fetch(`/api/events?before=${oldestEventId}&limit=${HISTORY_PAGE_SIZE}`, {
    signal,
  });
  const m = await r.json();
  const events = m.events || [];
  if (m.exhausted || !events.length) historyExhausted = true;
  if (events.length) {
    routeHistoricalBatch(events);
    prefetchUnitsFromEvents(events);
    applyFilters();
  }
  return events.length ? events : null;
}

function routeSearchHits(events) {
  const fresh = (events || []).filter((ev) => ev?.id != null && !seenEventIds.has(ev.id));
  if (!fresh.length) return 0;
  // Historical insert expects oldest→newest within a batch.
  routeHistoricalBatch(fresh);
  prefetchUnitsFromEvents(fresh);
  applyFilters();
  return fresh.length;
}

/**
 * Render the next SEARCH_PAGE_SIZE *new* matches from searchHitBuffer into the DOM.
 * Tip snapshot events are already in the feed (and in seenEventIds); skip those.
 */
function renderSearchPage() {
  let added = 0;
  while (added < SEARCH_PAGE_SIZE && searchHitOffset < searchHitBuffer.length) {
    const page = searchHitBuffer.slice(searchHitOffset, searchHitOffset + SEARCH_PAGE_SIZE);
    searchHitOffset += page.length;
    // Buffer is newest-first; reverse so older cards stack below newer ones.
    added += routeSearchHits(page.slice().reverse());
  }
  if (searchHitOffset >= searchHitBuffer.length) searchHitsExhausted = true;
  return added;
}

/**
 * Wait for the background 24h cache if needed, then filter locally and render
 * the first DOM page. No per-search server round-trip.
 */
async function runSearchPrime(snapshotCount) {
  if (!$("search").value.trim()) return;
  const gen = ++searchPrimeGen;
  abortHistoryLoad();
  searchPriming = true;
  searchScanned = snapshotCount || 0;
  searchMatchesTotal = 0;
  searchHitBuffer = [];
  searchHitOffset = 0;
  searchHitsExhausted = false;
  armSearchEmptyGrace();
  setSearchPrime(true);
  hideSearchPrompts();
  applyFilters();

  // Tip paints immediately; search waits on the background retention preload.
  if (!retentionReady) {
    startRetentionPreload();
    await whenRetentionReady();
    if (gen !== searchPrimeGen) return;
  }

  await waitForEnrichment(gen);
  if (gen !== searchPrimeGen) return;

  const q = $("search").value.trim();
  const hits = searchRetentionLocal(q);
  searchHitBuffer = hits;
  searchHitOffset = 0;
  searchMatchesTotal = hits.length;
  searchHitsExhausted = false;
  searchScanned = Math.max(searchScanned, retentionCache.size, bufferedEventCount || 0);
  renderSearchPage();
  await waitForEnrichment(gen);
  if (gen !== searchPrimeGen) return;

  searchPriming = false;
  setSearchPrime(false);
  applyFilters();
  updateSearchEmptyPrompt();
}

/** Keep the history loader after every block group (older pages append downward). */
function pinHistoryLoader() {
  const el = $("history-loader");
  if (el && el.parentElement === feed) feed.appendChild(el);
}

function setHistoryLoading(on, exhausted = false, label = null) {
  let el = $("history-loader");
  if (!el) {
    el = document.createElement("div");
    el.id = "history-loader";
    el.className = "history-loader";
    el.setAttribute("aria-live", "polite");
  }
  // Always pin to the bottom of the feed - history loads downward.
  feed.appendChild(el);
  if (on) {
    const text = label || "Loading older events…";
    el.innerHTML = `<span class="hist-spin" aria-hidden="true"></span><span class="hist-t">${esc(text)}</span>`;
    el.classList.add("show");
    el.dataset.state = "loading";
  } else if (exhausted) {
    el.innerHTML = `<span class="hist-t">Beginning of recorded history</span>`;
    el.classList.add("show");
    el.dataset.state = "end";
  } else {
    el.classList.remove("show");
    el.dataset.state = "";
  }
}

/* ── Header stats & sparkline ─────────────────────────────────────────── */

function setTip(tip) {
  if (!tip || !tip.height) return;
  $("st-height").textContent = fmtInt(tip.height);
  $("st-slot").textContent = fmtInt(tip.slot);
  $("st-epoch").innerHTML = `${fmtInt(tip.epoch)} <small>${(tip.epoch_progress * 100).toFixed(1)}%</small>`;
  $("st-epoch-bar").style.width = (tip.epoch_progress * 100).toFixed(2) + "%";
}

/** Activity / epm over the full loaded history (chain timestamps), not live arrivals. */
function refreshActivityMonitor() {
  const stamps = [];
  for (const ts of loadedTimestamps.values()) {
    if (ts > 0) stamps.push(ts);
  }
  const hist = $("st-hist");
  const actTile = document.querySelector(".tile.activity");
  const sparkLine = document.querySelector("#spark .line");
  const sparkArea = document.querySelector("#spark .area");

  if (!stamps.length) {
    $("st-epm").textContent = "-";
    $("st-act").textContent = "-";
    if (hist) {
      hist.hidden = true;
      hist.textContent = "";
    }
    if (actTile) actTile.title = "event density over loaded history";
    if (sparkLine) sparkLine.setAttribute("d", "");
    if (sparkArea) sparkArea.setAttribute("d", "");
    return;
  }

  stamps.sort((a, b) => a - b);
  const oldest = stamps[0];
  const newest = stamps[stamps.length - 1];
  const spanSec = Math.max(1, newest - oldest);
  const hours = spanSec / 3600;
  const spanMin = spanSec / 60;
  const epm = stamps.length / Math.max(spanMin, 1 / 60);
  $("st-epm").textContent = fmtInt(Math.round(epm));
  $("st-act").textContent = fmtInt(stamps.length);

  const label = formatHistorySpan(hours);
  if (hist) {
    hist.hidden = !label;
    hist.textContent = label;
  }
  if (actTile) {
    actTile.title = label
      ? `event density over ${label} of loaded history`
      : "event density over loaded history";
  }

  // Sparkline: density across the whole loaded span (36 buckets).
  const buckets = new Array(36).fill(0);
  for (const ts of stamps) {
    const i = Math.min(35, Math.floor(((ts - oldest) / spanSec) * 36));
    buckets[i]++;
  }
  const max = Math.max(4, ...buckets);
  const W = 110, H = 34, P = 3;
  const pts = buckets.map((v, i) => [
    P + (i * (W - 2 * P)) / 35,
    H - P - (v / max) * (H - 2 * P),
  ]);
  const line = "M" + pts.map((p) => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join(" L");
  if (sparkLine) sparkLine.setAttribute("d", line);
  if (sparkArea) {
    sparkArea.setAttribute(
      "d",
      `${line} L${(W - P).toFixed(1)} ${H - P} L${P} ${H - P} Z`
    );
  }
}

setInterval(() => {
  refreshActivityMonitor();
  updateLoadedEventCount();
}, 2000);

setInterval(() => {
  document.querySelectorAll(".ev-time[data-ts]").forEach((el) => {
    el.textContent = timeAgo(Number(el.dataset.ts));
  });
}, 20_000);

/* ── Asset & pool metadata enrichment ─────────────────────────────────── */

const assetMeta = new Map(Object.entries(store.get("co_assets_v2", {})));
/** True after /api/registry hydrate finishes (misses can safely default to 0). */
let registryReady = false;
const assetInflight = new Set();
const assetQueue = [];
/** unit → Promise<meta> so many cards share one in-flight fetch. */
const assetFetches = new Map();

function persistAssetCache() {
  const obj = {};
  let i = 0;
  for (const [k, v] of assetMeta) {
    if (i++ > 700) break;
    // Only persist resolved decimals so a prior "unknown → 0" bug cannot stick.
    if (v.decimals == null) continue;
    obj[k] = { name: v.name, ticker: v.ticker, decimals: v.decimals };
  }
  store.set("co_assets_v2", obj);
}

/** Hydrate assetMeta from the server's in-memory CIP-26 registry, then repaint. */
async function loadRegistryMeta() {
  try {
    const r = await fetch("/api/registry");
    if (!r.ok) return;
    const m = await r.json();
    const assets = m.assets || {};
    let n = 0;
    for (const [unit, meta] of Object.entries(assets)) {
      if (!meta || typeof meta !== "object") continue;
      const prev = assetMeta.get(unit) || {};
      const decimals = meta.decimals == null || meta.decimals === ""
        ? prev.decimals
        : Number(meta.decimals);
      assetMeta.set(unit, {
        name: meta.name || prev.name || null,
        ticker: meta.ticker || prev.ticker || null,
        decimals: Number.isFinite(decimals) ? decimals : prev.decimals ?? null,
        logo: prev.logo || null,
      });
      n++;
    }
    if (n) {
      persistAssetCache();
    }
    registryReady = true;
    document
      .querySelectorAll(".asset[data-unit], .dex-amt[data-unit]")
      .forEach((chip) => {
        const unit = chip.dataset.unit;
        const meta = registryMetaFor(unit) || assetMeta.get(unit);
        const decimals = meta?.decimals != null ? Number(meta.decimals) : (registryReady ? 0 : null);
        if (decimals == null) return;
        const painted = meta || { decimals: 0 };
        if (painted.decimals == null) painted.decimals = 0;
        paintAsset(chip, painted);
      });
  } catch (e) {
    console.warn("registry hydrate failed", e);
    registryReady = true; // still allow default-0 for unregistered
  }
}

function enrichAssets(root) {
  root.querySelectorAll(".asset[data-unit], .dex-amt[data-unit]").forEach((chip) => {
    const unit = chip.dataset.unit;
    if (!unit) return;
    const cached = assetMeta.get(unit);
    if (cached && cached.decimals != null) return void paintAsset(chip, cached);
    assetQueue.push(chip);
    pumpAssets();
  });
}

/** Warm the cache for every unit in a batch of events (snapshot / history). */
function prefetchUnitsFromEvents(events) {
  const units = new Set();
  const walk = (obj) => {
    if (!obj || typeof obj !== "object") return;
    if (Array.isArray(obj)) return obj.forEach(walk);
    if (obj.unit && typeof obj.unit === "string") units.add(obj.unit);
    if (obj.items) walk(obj.items);
    for (const v of Object.values(obj)) {
      if (v && typeof v === "object") walk(v);
    }
  };
  for (const ev of events || []) walk(ev.data);
  for (const unit of units) {
    if (assetMeta.get(unit)?.decimals != null || assetFetches.has(unit)) continue;
    fetchAssetMeta(unit).then((meta) => {
      if (!meta || meta.decimals == null) return;
      document
        .querySelectorAll(`.asset[data-unit="${CSS.escape(unit)}"], .dex-amt[data-unit="${CSS.escape(unit)}"]`)
        .forEach((c) => paintAsset(c, meta));
    });
  }
}

function fetchAssetMeta(unit) {
  if (assetMeta.get(unit)?.decimals != null) {
    return Promise.resolve(assetMeta.get(unit));
  }
  if (assetFetches.has(unit)) return assetFetches.get(unit);
  const p = (async () => {
    try {
      const r = await fetch(`/api/asset/${unit}`);
      const m = await r.json();
      if (m.error) return null;
      let decimals = m.decimals == null || m.decimals === "" ? null : Number(m.decimals);
      // Server defaults missing metadata to 0; keep that so we never leave "…".
      if (!Number.isFinite(decimals)) decimals = 0;
      const meta = {
        name: m.name || null,
        ticker: m.ticker || null,
        decimals,
        logo: m.logo || null,
      };
      assetMeta.set(unit, meta);
      persistAssetCache();
      return meta;
    } catch {
      return null;
    } finally {
      assetFetches.delete(unit);
    }
  })();
  assetFetches.set(unit, p);
  return p;
}

let assetWorkers = 0;
async function pumpAssets() {
  if (assetWorkers >= 8) return;
  const chip = assetQueue.shift();
  if (!chip) return;
  const unit = chip.dataset.unit;
  const hit = assetMeta.get(unit);
  if (hit && hit.decimals != null) { paintAsset(chip, hit); return pumpAssets(); }
  assetWorkers++;
  try {
    const meta = await fetchAssetMeta(unit);
    if (meta) {
      document
        .querySelectorAll(`.asset[data-unit="${CSS.escape(unit)}"], .dex-amt[data-unit="${CSS.escape(unit)}"]`)
        .forEach((c) => paintAsset(c, meta));
    }
  } catch { /* enrichment is best-effort */ }
  assetWorkers--;
  pumpAssets();
}

function paintAsset(chip, meta) {
  const t = chip.querySelector(".t");
  const q = chip.querySelector(".q");
  if ((meta.ticker || meta.name) && t) t.textContent = meta.ticker || meta.name;
  if (q && q.dataset.qty) {
    q.innerHTML = fmtTokenQty(q.dataset.qty, meta.decimals);
  }
  if (meta.logo && chip.classList.contains("asset")) {
    const img = document.createElement("img");
    img.src = meta.logo.startsWith("data:") ? meta.logo : `data:image/png;base64,${meta.logo}`;
    img.alt = "";
    img.onerror = () => img.remove();
    chip.querySelector(".ph")?.replaceWith(img);
  }
  // Keep registry tickers/names in the card search haystack.
  const card = chip.closest(".card");
  if (card) {
    appendCardSearch(card, meta.ticker, meta.name);
    const eid = Number(card.dataset.eid);
    if (Number.isFinite(eid) && retentionCache.has(eid)) {
      retentionIndex(retentionCache.get(eid).ev);
    }
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
}

const poolMeta = new Map(Object.entries(store.get("co_pools_v1", {})));
const poolWaiters = new Map(); // poolId → elements waiting on in-flight fetch
const drepMeta = new Map(Object.entries(store.get("co_dreps_v1", {})));
const drepWaiters = new Map(); // drepId → elements waiting on in-flight fetch

function persistPoolCache() {
  const obj = {};
  let i = 0;
  for (const [k, v] of poolMeta) {
    if (!v || (!v.ticker && !v.name)) continue;
    if (i++ > 700) break;
    obj[k] = { ticker: v.ticker || null, name: v.name || null };
  }
  store.set("co_pools_v1", obj);
}

function enrichPools(root) {
  root.querySelectorAll("[data-pool]").forEach((el) => {
    const id = el.dataset.pool;
    if (!id) return;
    const cached = poolMeta.get(id);
    if (cached && (cached.ticker || cached.name)) {
      paintPool(el, cached);
      return;
    }
    if (poolWaiters.has(id)) {
      poolWaiters.get(id).push(el);
      return;
    }
    poolWaiters.set(id, [el]);
    fetch(`/api/pool/${encodeURIComponent(id)}`)
      .then((r) => r.json())
      .then((meta) => {
        if (meta && (meta.ticker || meta.name)) {
          poolMeta.set(id, meta);
          persistPoolCache();
        } else {
          poolMeta.set(id, { pool: id }); // negative cache - keep id visible
        }
        const waiters = poolWaiters.get(id) || [];
        poolWaiters.delete(id);
        const all = new Set([
          ...waiters,
          ...document.querySelectorAll(`[data-pool="${CSS.escape(id)}"]`),
        ]);
        all.forEach((e) => paintPool(e, poolMeta.get(id)));
      })
      .catch(() => {
        poolWaiters.delete(id);
        if ($("search").value.trim()) scheduleFilterRefresh();
      });
  });
}

function paintPool(el, meta) {
  if (!meta) return;
  const ticker = typeof meta.ticker === "string" && meta.ticker ? meta.ticker : "";
  const name = typeof meta.name === "string" && meta.name ? meta.name : "";
  const poolId = el.dataset.pool || "";
  // Keep ticker/name searchable even before paint (and after async resolve).
  const card = el.closest(".card");
  if (card) {
    appendCardSearch(card, poolId, ticker, name);
    const eid = Number(card.dataset.eid);
    if (Number.isFinite(eid) && retentionCache.has(eid)) {
      retentionIndex(retentionCache.get(eid).ev);
    }
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
  if (!ticker && !name) return; // leave truncated pool id as fallback
  el.textContent = ticker || name;
  el.classList.remove("hash");
  el.classList.add("pool-ticker");
  el.title = [name && name !== ticker ? name : null, poolId].filter(Boolean).join(" · ");
}

function persistDrepCache() {
  const obj = {};
  let i = 0;
  for (const [k, v] of drepMeta) {
    if (!v || !v.name) continue;
    if (i++ > 700) break;
    obj[k] = { name: v.name || null };
  }
  store.set("co_dreps_v1", obj);
}

/** Hydrate drepMeta from the server's durable cache, then repaint. */
async function loadDrepMeta() {
  try {
    const r = await fetch("/api/dreps");
    if (!r.ok) return;
    const m = await r.json();
    let n = 0;
    for (const [id, meta] of Object.entries(m || {})) {
      if (!meta || typeof meta !== "object" || !meta.name) continue;
      drepMeta.set(id, { name: meta.name });
      n++;
    }
    if (n) persistDrepCache();
    document.querySelectorAll("[data-drep]").forEach((el) => {
      const id = el.dataset.drep;
      const cached = drepMeta.get(id);
      if (cached?.name) paintDrep(el, cached);
    });
  } catch {
    /* leave truncated ids */
  }
}

function enrichDreps(root) {
  root.querySelectorAll("[data-drep]").forEach((el) => {
    const id = el.dataset.drep;
    if (!id || !isLookupDrepId(id)) return;
    const cached = drepMeta.get(id);
    if (cached && cached.name) {
      paintDrep(el, cached);
      return;
    }
    if (drepWaiters.has(id)) {
      drepWaiters.get(id).push(el);
      return;
    }
    drepWaiters.set(id, [el]);
    fetch(`/api/drep/${encodeURIComponent(id)}`)
      .then((r) => r.json())
      .then((meta) => {
        if (meta && meta.name) {
          drepMeta.set(id, meta);
          persistDrepCache();
        } else {
          drepMeta.set(id, { drep: id }); // negative cache - keep id visible
        }
        const waiters = drepWaiters.get(id) || [];
        drepWaiters.delete(id);
        const all = new Set([
          ...waiters,
          ...document.querySelectorAll(`[data-drep="${CSS.escape(id)}"]`),
        ]);
        all.forEach((e) => paintDrep(e, drepMeta.get(id)));
      })
      .catch(() => {
        drepWaiters.delete(id);
        if ($("search").value.trim()) scheduleFilterRefresh();
      });
  });
}

function paintDrep(el, meta) {
  if (!meta) return;
  const name = typeof meta.name === "string" && meta.name ? meta.name : "";
  const drepId = el.dataset.drep || "";
  const card = el.closest(".card");
  if (card) {
    appendCardSearch(card, drepId, name);
    const eid = Number(card.dataset.eid);
    if (Number.isFinite(eid) && retentionCache.has(eid)) {
      retentionIndex(retentionCache.get(eid).ev);
    }
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
  if (!name) return; // leave truncated drep id as fallback
  el.textContent = name;
  el.classList.remove("hash");
  el.classList.add("drep-name");
  el.title = drepId;
}

const govMeta = new Map(Object.entries(store.get("co_gov_v1", {})));
const govWaiters = new Map();

function persistGovCache() {
  const obj = {};
  let i = 0;
  for (const [k, v] of govMeta) {
    if (!v || !v.title) continue;
    if (i++ > 700) break;
    obj[k] = { title: v.title || null };
  }
  store.set("co_gov_v1", obj);
}

/** Hydrate govMeta from the server's durable cache, then repaint. */
async function loadGovMeta() {
  try {
    const r = await fetch("/api/gov-actions");
    if (!r.ok) return;
    const m = await r.json();
    let n = 0;
    for (const [key, meta] of Object.entries(m || {})) {
      if (!meta || typeof meta !== "object" || !meta.title) continue;
      govMeta.set(key, { title: meta.title });
      n++;
    }
    if (n) persistGovCache();
    document.querySelectorAll("[data-gov]").forEach((el) => {
      const key = el.dataset.gov;
      const cached = govMeta.get(key);
      if (cached?.title) paintGov(el, cached);
    });
  } catch {
    /* leave tx#index hashes */
  }
}

function enrichGovActions(root) {
  root.querySelectorAll("[data-gov]").forEach((el) => {
    const key = el.dataset.gov;
    if (!key || !key.includes("#")) return;
    const cached = govMeta.get(key);
    if (cached && cached.title) {
      paintGov(el, cached);
      return;
    }
    if (el.classList.contains("gov-title")) return; // already stamped
    if (govWaiters.has(key)) {
      govWaiters.get(key).push(el);
      return;
    }
    const [tx, idxRaw] = key.split("#");
    const index = Number(idxRaw);
    if (!tx || !Number.isFinite(index)) return;
    govWaiters.set(key, [el]);
    fetch(`/api/gov-action/${encodeURIComponent(tx)}/${encodeURIComponent(index)}`)
      .then((r) => r.json())
      .then((meta) => {
        if (meta && meta.title) {
          govMeta.set(key, { title: meta.title });
          persistGovCache();
        } else {
          govMeta.set(key, { title: null }); // negative cache
        }
        const waiters = govWaiters.get(key) || [];
        govWaiters.delete(key);
        const all = new Set([
          ...waiters,
          ...document.querySelectorAll(`[data-gov="${CSS.escape(key)}"]`),
        ]);
        all.forEach((e) => paintGov(e, govMeta.get(key)));
      })
      .catch(() => {
        govWaiters.delete(key);
      });
  });
}

function paintGov(el, meta) {
  if (!meta) return;
  const title = typeof meta.title === "string" && meta.title ? meta.title : "";
  const key = el.dataset.gov || "";
  const card = el.closest(".card");
  if (card) {
    appendCardSearch(card, key, title);
    const eid = Number(card.dataset.eid);
    if (Number.isFinite(eid) && retentionCache.has(eid)) {
      retentionIndex(retentionCache.get(eid).ev);
    }
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
  if (!title) return;
  el.textContent = title;
  el.classList.remove("hash");
  el.classList.add("gov-title");
  if (key) el.title = key;
}

/* ── Modal ────────────────────────────────────────────────────────────── */

const overlay = $("overlay");
const mBody = $("m-body");
const mTitle = $("m-title");

function closeModal() { overlay.classList.remove("show"); mBody.innerHTML = ""; }
$("m-close").onclick = closeModal;
overlay.addEventListener("click", (e) => { if (e.target === overlay) closeModal(); });
addEventListener("keydown", (e) => { if (e.key === "Escape") closeModal(); });

mBody.addEventListener("click", async (e) => {
  const c = e.target.closest(".copyable");
  if (!c || c.dataset.copying) return;
  const text = c.dataset.copy || c.textContent;
  c.dataset.copying = "1";
  const label = c.dataset.label || c.textContent;
  const ok = await copyText(text);
  c.classList.toggle("copied", ok);
  c.classList.toggle("copy-fail", !ok);
  c.textContent = ok ? "Copied ✓" : "Copy failed";
  setTimeout(() => {
    c.textContent = label;
    c.classList.remove("copied", "copy-fail");
    delete c.dataset.copying;
  }, 1000);
});

function explorers(txHash) {
  const sub = NETWORK === "mainnet" ? "" : `${NETWORK}.`;
  const scan = `https://${sub}cardanoscan.io`;
  const cex = `https://${sub}cexplorer.io`;
  const ada = `https://${sub}adastat.net`;
  return `<div class="m-links">
    <a href="${scan}/transaction/${esc(txHash)}" target="_blank" rel="noopener">Cardanoscan ↗</a>
    <a href="${cex}/tx/${esc(txHash)}" target="_blank" rel="noopener">Cexplorer ↗</a>
    <a href="${ada}/transactions/${esc(txHash)}" target="_blank" rel="noopener">AdaStat ↗</a>
    <button type="button" class="copyable m-copy" data-copy="${esc(txHash)}" data-label="Copy Hash">Copy Hash</button>
  </div>`;
}

function kvRow(k, v) { return v == null || v === "" ? "" : `<dt>${k}</dt><dd>${v}</dd>`; }
const mono = (s, full) =>
  `<span class="hash copyable" data-copy="${esc(full || s)}" title="click to copy">${esc(short(s, 14, 8))}</span>`;

function openModal(ev) {
  overlay.classList.add("show");
  if (ev.tx_hash) return openTx(ev);
  mTitle.textContent = titleCaseWords(
    ev.kind === "vote_delegation" ? "DRep Delegation" : ev.title
  );
  mBody.innerHTML = renderEventDetail(ev);
}

function renderEventDetail(ev) {
  const d = ev.data || {};
  let extra = "";
  if (ev.kind === "block") {
    extra = `<div class="m-section"><h3>Block</h3><dl class="kv">
      ${kvRow("height", `<b>${fmtInt(d.height)}</b>`)}
      ${kvRow("hash", mono(d.hash, d.hash))}
      ${kvRow("slot", fmtInt(d.slot))}
      ${kvRow("time", esc(new Date(ev.timestamp * 1000).toLocaleString()))}
      ${kvRow("transactions", fmtInt(d.txCount))}
      ${kvRow("total output", d.totalOutput ? fmtAda(d.totalOutput) : null)}
      ${kvRow("total fees", d.totalFees ? fmtAda(d.totalFees) : null)}
      ${kvRow("size", d.size ? fmtInt(d.size / 1024) + " kB" : null)}
      ${kvRow("issuer pool", d.issuerPool ? mono(d.issuerPool, d.issuerPool) : null)}
      ${kvRow("era", d.era ? esc(d.era) : null)}
    </dl></div>`;
  }
  return `${extra}
    <div class="m-section"><h3>Event data</h3>
      <pre class="json">${esc(JSON.stringify(ev.data, null, 2))}</pre></div>`;
}

async function openTx(ev) {
  mTitle.textContent = "Transaction";
  mBody.innerHTML = `<div class="spin"></div>`;
  const hash = ev.tx_hash;
  try {
    const r = await fetch(`/api/tx/${hash}`);
    if (!r.ok) throw new Error("not found");
    const detail = await r.json();
    mBody.innerHTML = detail.tx
      ? renderOgmiosTx(hash, detail.tx, detail.block, ev)
      : renderBlockfrostTx(hash, detail.blockfrost, ev);
    enrichAssets(mBody);
  } catch {
    mBody.innerHTML = `<div class="m-empty">Transaction details are no longer in the live cache.<br>
      Configure <code>BLOCKFROST_URL</code> for history lookups.</div>${explorers(hash)}`;
  }
}

function valueAssetsChips(value) {
  const items = [];
  for (const [policy, assets] of Object.entries(value || {})) {
    if (policy === "ada" || typeof assets !== "object") continue;
    for (const [nameHex, qty] of Object.entries(assets)) {
      items.push({ unit: policy + nameHex, policy, nameHex, name: hexName(nameHex), qty: String(qty) });
    }
  }
  return items.length ? assetChipsHtml({ items, more: 0 }) : "";
}
function hexName(h) {
  try {
    const bytes = h.match(/.{2}/g)?.map((b) => parseInt(b, 16)) || [];
    const s = String.fromCharCode(...bytes);
    return /^[\x20-\x7e]+$/.test(s) ? s : null;
  } catch { return null; }
}

function renderOgmiosTx(hash, tx, block, ev) {
  const outputs = tx.outputs || [];
  const inputs = tx.inputs || [];
  const fee = tx.fee?.ada?.lovelace ?? 0;
  const total = outputs.reduce((s, o) => s + (o.value?.ada?.lovelace || 0), 0);

  const inHtml = inputs.map((i) =>
    `<div class="utxo">${mono(`${i.transaction?.id || "?"}`, i.transaction?.id)}<span class="hash">#${esc(String(i.index ?? "?"))}</span></div>`
  ).join("") || `<div class="m-empty">-</div>`;

  const outHtml = outputs.map((o) => `
    <div class="utxo">
      <div>${mono(o.address || "?", o.address)}</div>
      <div class="amt">${fmtAda(o.value?.ada?.lovelace || 0)}</div>
      ${valueAssetsChips(o.value)}
    </div>`).join("") || `<div class="m-empty">-</div>`;

  const section = (title, body) => body ? `<div class="m-section"><h3>${title}</h3>${body}</div>` : "";
  const jsonSection = (title, v) =>
    v && (Array.isArray(v) ? v.length : Object.keys(v).length)
      ? section(title, `<pre class="json">${esc(JSON.stringify(v, null, 2))}</pre>`)
      : "";

  return `
    <div class="m-section"><dl class="kv">
      ${kvRow("hash", mono(hash, hash))}
      ${kvRow("block", block ? `<b>${fmtInt(block.height)}</b> · slot ${fmtInt(block.slot)}` : null)}
      ${kvRow("time", block ? esc(new Date(block.timestamp * 1000).toLocaleString()) : null)}
      ${kvRow("total output", `<b>${fmtAda(total)}</b>`)}
      ${kvRow("fee", fmtAda(fee))}
      ${kvRow("inputs / outputs", `${inputs.length} / ${outputs.length}`)}
      ${kvRow("smart contract", tx.redeemers && Object.keys(tx.redeemers).length ? "yes" : null)}
    </dl></div>
    <div class="m-section io">
      <div class="io-col"><h4>Inputs <span>${inputs.length}</span></h4>${inHtml}</div>
      <div class="io-col"><h4>Outputs <span>${outputs.length}</span></h4>${outHtml}</div>
    </div>
    ${tx.mint ? section("Minted / burned", valueAssetsChips(tx.mint) || "") : ""}
    ${jsonSection("Certificates", tx.certificates)}
    ${jsonSection("Withdrawals", tx.withdrawals)}
    ${jsonSection("Governance proposals", tx.proposals)}
    ${jsonSection("Votes", tx.votes)}
    ${jsonSection("Metadata", tx.metadata?.labels)}
    ${jsonSection("Required signers", tx.requiredExtraSignatories)}
    <div class="m-section"><details class="raw"><summary>Raw transaction JSON</summary>
      <pre class="json">${esc(JSON.stringify(tx, null, 2))}</pre></details></div>
    ${explorers(hash)}`;
}

function renderBlockfrostTx(hash, bf, ev) {
  if (!bf || !bf.tx) return `<div class="m-empty">No details available.</div>${explorers(hash)}`;
  const tx = bf.tx;
  const utxos = bf.utxos || {};
  const io = (list, dir) => (list || []).map((u) => `
    <div class="utxo">
      <div>${mono(u.address || "?", u.address)}</div>
      <div class="amt">${fmtAda((u.amount || []).find((a) => a.unit === "lovelace")?.quantity || 0)}</div>
      ${assetChipsHtml({
        items: (u.amount || []).filter((a) => a.unit !== "lovelace").slice(0, 10).map((a) => ({
          unit: a.unit, policy: a.unit.slice(0, 56), nameHex: a.unit.slice(56),
          name: hexName(a.unit.slice(56)), qty: a.quantity,
        })),
        more: 0,
      })}
    </div>`).join("") || `<div class="m-empty">-</div>`;

  return `
    <div class="m-section"><dl class="kv">
      ${kvRow("hash", mono(hash, hash))}
      ${kvRow("block", tx.block_height ? `<b>${fmtInt(tx.block_height)}</b> · slot ${fmtInt(tx.slot)}` : null)}
      ${kvRow("fee", fmtAda(tx.fees))}
      ${kvRow("total output", fmtAda(tx.output_amount?.find((a) => a.unit === "lovelace")?.quantity))}
      ${kvRow("certificates", (tx.stake_cert_count || 0) + (tx.delegation_count || 0) + (tx.pool_update_count || 0) || null)}
      ${kvRow("source", "Blockfrost (historical)")}
    </dl></div>
    <div class="m-section io">
      <div class="io-col"><h4>Inputs</h4>${io(utxos.inputs, "in")}</div>
      <div class="io-col"><h4>Outputs</h4>${io(utxos.outputs, "out")}</div>
    </div>
    <div class="m-section"><details class="raw"><summary>Raw JSON</summary>
      <pre class="json">${esc(JSON.stringify(bf, null, 2))}</pre></details></div>
    ${explorers(hash)}`;
}

/* ── WebSocket ────────────────────────────────────────────────────────── */

let ws, wsRetry = 1;

function setConn(status) {
  const el = $("conn");
  el.className = "conn " + status;
  $("conn-t").textContent = status === "demo" ? "demo feed" : status;
}

function connect() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  ws = new WebSocket(`${proto}//${location.host}/ws`);

  ws.onopen = () => { wsRetry = 1; };
  ws.onmessage = (e) => {
    let m;
    try { m = JSON.parse(e.data); } catch { return; }
    switch (m.type) {
      case "snapshot":
        NETWORK = m.network || NETWORK;
        $("net").textContent = NETWORK;
        setConn(m.source || "connected");
        if (m.buffered != null) bufferedEventCount = Number(m.buffered) || 0;
        if (m.retention_hours != null) retentionHours = Number(m.retention_hours) || 24;
        feed.innerHTML = "";
        groups.clear();
        groupOrder.length = 0;
        seenEventIds.clear();
        oldestEventId = null;
        historyExhausted = false;
        historyLoading = false;
        retentionHistoryDone = false;
        feedHitBuffer = [];
        feedHitOffset = 0;
        feedHitKey = "";
        searchPrimeGen++;
        if (historyAbort) {
          try { historyAbort.abort(); } catch { /* ignore */ }
          historyAbort = null;
        }
        searchPriming = false;
        setSearchPrime(false);
        hideSearchPrompts();
        searchScanned = 0;
        searchMatchesTotal = 0;
        searchHitBuffer = [];
        searchHitOffset = 0;
        searchHitsExhausted = false;
        // Tip events re-index via noteEventId; full 24h window loads in the
        // background without blocking the initial paint.
        retentionCache.clear();
        orphanedBlocks.clear();
        historySparsePause = false;
        historyLoadArmed = true;
        retentionReady = false;
        resetLoadedCounts();
        for (const ev of m.events || []) routeEvent(ev);
        setTip(m.tip);
        if (m.trending) renderTrending(m.trending);
        applyFilters();
        prefetchUnitsFromEvents(m.events || []);
        startRetentionPreload(true);
        {
          const snapN = (m.events || []).length;
          const wantPrime = !!(urlSearchPreset && $("search").value.trim());
          // Don't pad the tip with history on first paint - wait for scroll.
          if (wantPrime) queueMicrotask(() => runSearchPrime(snapN));
        }
        break;
      case "event":
        onEvent(m.event);
        break;
      case "tip":
        setTip(m.tip);
        break;
      case "status":
        setConn(m.source);
        break;
      case "stats":
        if (m.buffered != null) bufferedEventCount = Number(m.buffered) || bufferedEventCount;
        if (m.retention_hours != null) retentionHours = Number(m.retention_hours) || retentionHours;
        retentionTrim();
        updateLoadedEventCount();
        paintCatCounts();
        refreshActivityMonitor();
        break;
      case "trending":
        renderTrending(m.terms);
        break;
    }
  };
  ws.onclose = () => {
    setConn("disconnected");
    setTimeout(connect, Math.min(15000, wsRetry * 1000));
    wsRetry = Math.min(wsRetry * 2, 15);
  };
  ws.onerror = () => ws.close();
}

/* ── Trending ticker ──────────────────────────────────────────────────── */

let trendingTerms = [];
let trendingSig = "";

function renderTrending(terms) {
  const bar = $("trending");
  const track = $("trending-track");
  if (!bar || !track) return;
  const list = Array.isArray(terms) ? terms.filter((t) => t && t.term) : [];
  const sig = list.map((t) => `${t.term}:${t.count}`).join("|");
  if (sig === trendingSig) return;
  trendingSig = sig;
  trendingTerms = list;

  if (!list.length) {
    bar.hidden = true;
    track.innerHTML = "";
    track.classList.remove("marquee");
    return;
  }

  const chips = list
    .map((t, i) => {
      const term = String(t.term);
      return `<button type="button" class="trend-chip" data-term="${esc(term)}" title="Filter by “${esc(term)}”">
        <span class="rank">${i + 1}</span><span>${esc(term)}</span><span class="n">${fmtInt(t.count)}</span>
      </button>`;
    })
    .join("");

  // Duplicate the row so the CSS marquee can loop seamlessly.
  track.innerHTML = chips + chips;
  track.classList.toggle("marquee", list.length >= 4);
  // Slower when more terms so each stays readable.
  const dur = Math.max(18, Math.min(48, 8 + list.length * 3.5));
  track.style.setProperty("--trending-dur", dur + "s");
  bar.hidden = false;
}

function applyTrendingTerm(term) {
  const search = $("search");
  if (!search || !term) return;
  search.value = term;
  search.focus();
  // Mirror typed-search behaviour (filters + empty/history prompts).
  search.dispatchEvent(new Event("input", { bubbles: true }));
}

$("trending-track")?.addEventListener("click", (e) => {
  const btn = e.target.closest(".trend-chip");
  if (!btn) return;
  applyTrendingTerm(btn.dataset.term || btn.textContent);
});

/* ── Boot ─────────────────────────────────────────────────────────────── */

buildToolbar();
applyFilters();
loadRegistryMeta();
loadDrepMeta();
loadGovMeta();
connect();
