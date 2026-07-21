/* cardano-observer frontend - zero dependencies, one WebSocket. */

/* ── DEX UI pack (`static/dex/mod.js`) ─────────────────────────────────── */
/** DEX venues emitted as `data.dex` — fallback if `/dex/mod.js` is missing. */
let DEX_VENUES = [
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
/** Brand icon for DEX cards; null when pack/absent. */
let dexIconHtml = null;
try {
  const dexMod = await import("/dex/mod.js");
  if (Array.isArray(dexMod.DEX_VENUES) && dexMod.DEX_VENUES.length) {
    DEX_VENUES = dexMod.DEX_VENUES;
  }
  if (typeof dexMod.dexIconHtml === "function") {
    dexIconHtml = dexMod.dexIconHtml;
  }
} catch {
  // Venues list above still works without the logo pack.
}

/** Server-side `model::FINANCE_APPS` — dApps filed under Finance, not dApp. */
let FINANCE_DAPP_NAMES = new Set([
  "Dano Finance",
  "FluidTokens",
  "Indigo Protocol",
  "Liqwid",
  "Optim Finance",
  "Strike",
  "Surf",
]);

/* ── Optional dApp UI pack (`static/dapp/mod.js`) ──────────────────────── */
/** Names for per-dApp filters; empty when the dApp pack is absent. */
let DAPP_APPS = [];
/** Card renderer from the pack; null when absent. */
let renderDappActivityHtml = null;
/** Brand icon HTML for dApp cards; null when pack/absent. */
let dappIconHtml = null;
try {
  const dappMod = await import("/dapp/mod.js");
  DAPP_APPS = Array.isArray(dappMod.DAPP_APPS) ? dappMod.DAPP_APPS : [];
  if (typeof dappMod.renderDappActivityHtml === "function") {
    renderDappActivityHtml = dappMod.renderDappActivityHtml;
  }
  if (typeof dappMod.dappIconHtml === "function") {
    dappIconHtml = dappMod.dappIconHtml;
  }
  if (Array.isArray(dappMod.FINANCE_APPS) && dappMod.FINANCE_APPS.length) {
    FINANCE_DAPP_NAMES = new Set(dappMod.FINANCE_APPS);
  }
} catch {
  // Core UI runs without `static/dapp/` (or when the server was built without it).
}

/* ── Finance filter list ───────────────────────────────────────────────── */
/**
 * DEX venues and finance dApps share one category and one filter list, so a
 * protocol that both trades and lends (Dano Finance) is a single chip rather
 * than one under DEX and another under dApp. `FINANCE_DAPPS` mirrors
 * `model::FINANCE_APPS` on the server.
 */
let FINANCE_DAPPS = DAPP_APPS.filter((a) => FINANCE_DAPP_NAMES.has(a));
/** Merged, de-duplicated: `data.dex` and `data.dapp` values in one list. */
let FINANCE_APPS = [...new Set([...DEX_VENUES, ...FINANCE_DAPPS])];
// Everything left over (Iagon, Wayup) keeps its own dApp chip.
DAPP_APPS = DAPP_APPS.filter((a) => !FINANCE_DAPP_NAMES.has(a));

/* ── Category & icon registry ─────────────────────────────────────────── */

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
  { id: "finance",     label: "Finance" },
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
  return ICONS[kind] || ICONS[{ token: "token_transfer", staking: "delegation", governance: "gov_proposal", metadata: "tx_metadata", alert: "slot_battle", finance: "dex", dapp: "dapp", pool: "pool", mint: "mint" }[category]] || ICONS.transaction;
};

/* ── Tiny helpers ─────────────────────────────────────────────────────── */

const $ = (id) => document.getElementById(id);
const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
/** Capitalize the first letter of each word; leave already-capital letters alone (LP, Minswap, …). */
const titleCaseWords = (s) =>
  String(s ?? "").replace(/(^|[^A-Za-z0-9])([a-z])/g, (_, sep, ch) => sep + ch.toUpperCase());

/** Card / modal title. DApp event titles are shown as authored by the detector. */
function formatEventTitle(ev) {
  if (!ev) return "";
  if (ev.kind === "vote_delegation") return "DRep Delegation";
  if (ev.data?.dapp) return String(ev.title || "");
  return titleCaseWords(ev.title);
}
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
  // Per-venue/app Finance toggles (true = include). Unknown stay visible.
  // Seeded from the pre-merge keys so existing preferences survive the rename.
  financeApps: {
    ...Object.fromEntries(FINANCE_APPS.map((d) => [d, true])),
    ...store.get("co_dex_venues_v1", {}),
    ...store.get("co_dapp_apps_v1", {}),
    ...store.get("co_finance_apps_v1", {}),
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

/** Keys that are never filter tokens (search / misc URL controls). */
/**
 * Split `location.search` into a search phrase and a list of filter names.
 *
 * Tokens are `&`-separated. `search=` and `filters=` each open a section that
 * runs until the next `key=value` token, so a bare token means "another search
 * word" or "another filter name" depending on which section it lands in. That
 * removes the old ambiguity where `?usdcx&filters=finance&dapp` couldn't tell
 * whether `dapp` was a word to search for or a chip to enable.
 *
 *   ?search=usdcx&pool                         → search "usdcx pool"
 *   ?filters=minswap&blocks                    → Minswap + Blocks
 *   ?search=usdcx&pool&filters=finance&dapp    → both
 *   ?filters=finance&search=usdcx              → order-independent
 *   ?usdcx                                     → shorthand, search only
 *
 * `&` inside a search section stands in for a space, so search words can't
 * contain `=`; anything with one closes the section.
 */
function parseUrlQuery(raw = location.search) {
  const decode = (t) => {
    try {
      return decodeURIComponent(t.replace(/\+/g, " "));
    } catch {
      return t;
    }
  };
  const words = [];
  const filters = [];
  const loose = [];
  let section = null;

  for (const part of raw.replace(/^\?/, "").split("&")) {
    if (!part) continue;
    const eq = part.indexOf("=");
    if (eq === -1) {
      const tok = decode(part);
      if (section === "search") words.push(tok);
      else if (section === "filters") filters.push(tok);
      else loose.push(tok);
      continue;
    }
    const key = decode(part.slice(0, eq)).toLowerCase();
    const val = decode(part.slice(eq + 1)).trim();
    if (key === "search") {
      section = "search";
      if (val) words.push(val);
    } else if (key === "filters") {
      section = "filters";
      if (val) filters.push(val);
    } else {
      // Any other `key=value` (min, layout, …) closes the open section.
      section = null;
    }
  }

  // Shorthand `?usdcx`: only when nothing else claimed the query, so a bare
  // token can never be mistaken for a filter name.
  if (!words.length && !filters.length && loose.length) words.push(loose[0]);

  return { search: words.filter(Boolean).join(" "), filters };
}

function normFilterKey(s) {
  return String(s || "").toLowerCase().replace(/[\s_\-./]+/g, "");
}

/**
 * Significant words from an on-screen category chip label.
 * Splits on spaces / & / / ; drops "and".
 * e.g. "Forks & Battles" → forks, battles; "Mint / Burn" → mint, burn.
 */
function filterLabelWords(label) {
  return String(label || "")
    .split(/[\s&/+,|.\-]+/)
    .map((w) => normFilterKey(w))
    .filter((w) => w && w !== "and");
}

/**
 * DEX/dApp URL tokens from the on-screen name.
 * One-word names (`VyFinance`, `SundaeSwap`) → only the full name.
 * Multi-word names (`Dano Finance`) → full name + first word brand (`dano`).
 */
function brandTokens(name) {
  const label = String(name || "").trim();
  if (!label) return [];
  const full = normFilterKey(label);
  const spaced = label.split(/\s+/).filter(Boolean);
  if (spaced.length > 1) return [full, normFilterKey(spaced[0])];
  return [full];
}

/**
 * Category chips: exact label/id, or any word of a multi-word label.
 * DEX/dApp: exact one-word name, or brand root of a multi-word name.
 */
function matchUiFilterNames(raw, candidates, labelOf, idOf, mode) {
  const n = normFilterKey(raw);
  if (!n) return [];
  const exact = candidates.filter((c) => {
    if (idOf && normFilterKey(idOf(c)) === n) return true;
    return normFilterKey(labelOf(c)) === n;
  });
  if (exact.length) return exact;

  if (mode === "brand") {
    return candidates.filter((c) => brandTokens(labelOf(c)).includes(n));
  }
  return candidates.filter((c) => {
    const words = filterLabelWords(labelOf(c));
    return words.length > 1 && words.includes(n);
  });
}

function matchFinanceApps(raw) {
  return matchUiFilterNames(raw, FINANCE_APPS, (v) => v, null, "brand");
}

function matchDappApps(raw) {
  return matchUiFilterNames(raw, DAPP_APPS, (a) => a, null, "brand");
}

function matchFilterCategories(raw) {
  return matchUiFilterNames(
    raw,
    CATS,
    (c) => c.label,
    (c) => c.id,
    "category",
  ).map((c) => c.id);
}

/**
 * Chip / submenu presets from the `filters=` section — see [`parseUrlQuery`]:
 *   ?filters=minswap&blocks&iagon
 *   ?filters=forks&dano          → Forks & Battles + Dano Finance
 *   ?filters=vyfinance           → VyFinance (full one-word name)
 *
 * Venue/app names: one-word names match in full; multi-word use the first word.
 */
function parseUrlFilterPreset() {
  const tokens = parseUrlQuery().filters;
  if (!tokens.length) return null;

  const categories = new Set();
  const financeApps = new Set();
  const dappApps = new Set();

  for (const tok of tokens) {
    const venues = matchFinanceApps(tok);
    if (venues.length) {
      for (const v of venues) financeApps.add(v);
      categories.add("finance");
      continue;
    }
    const dapps = matchDappApps(tok);
    if (dapps.length) {
      for (const a of dapps) dappApps.add(a);
      categories.add("dapp");
      continue;
    }
    for (const id of matchFilterCategories(tok)) categories.add(id);
  }

  if (!categories.size && !financeApps.size && !dappApps.size) return null;
  return { categories, financeApps, dappApps };
}

/** Apply `?filters=` over localStorage defaults (shareable deep-links win). */
function applyUrlFilterPreset() {
  const parsed = parseUrlFilterPreset();
  if (!parsed) return false;
  const { categories, financeApps, dappApps } = parsed;

  for (const c of CATS) {
    settings.filters[c.id] = categories.has(c.id);
  }

  if (financeApps.size) {
    for (const v of FINANCE_APPS) settings.financeApps[v] = financeApps.has(v);
  } else if (categories.has("finance")) {
    for (const v of FINANCE_APPS) settings.financeApps[v] = true;
  }

  if (dappApps.size) {
    for (const a of DAPP_APPS) settings.dappApps[a] = dappApps.has(a);
  } else if (categories.has("dapp")) {
    for (const a of DAPP_APPS) settings.dappApps[a] = true;
  }

  // Governance subtype menu stays all-on when the Governance chip is enabled.
  if (categories.has("governance")) {
    for (const g of GOV_TYPES) settings.govTypes[g.id] = true;
  }

  return true;
}

applyUrlFilterPreset();

function financeAppEnabled(name) {
  if (!name) return true;
  return settings.financeApps[name] !== false;
}

/** Finance cards carry `data.dex` or `data.dapp`; the chip list merges both. */
function financeNameOf(card) {
  return card.dataset.finance || "";
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
 * On hover we BFS both directions to light the whole light cone (inset glow;
 * unrelated cards stay as-is). `seen` marks a tx we actually ingested (vs. a
 * placeholder created only because a newer tx referenced it as an input).
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
  // Settlement txs often arrive as a transaction event before/with the DEX
  // fill card — once edges exist, resolve any pending order pills.
  reconcileDexSettlementsForTx(ev.tx_hash);
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

// Max directed hops for light-cone highlighting. Unbounded BFS walks through
// consolidators/batchers and lights unrelated dApp activity (e.g. Iagon↔Indigo).
const LC_MAX_HOPS = 2;

// BFS the spend graph in one direction ("ins" = past cone, "outs" = future).
function coneReach(start, dir, maxHops = LC_MAX_HOPS) {
  const out = new Set();
  const queue = [[start, 0]];
  const seen = new Set([start]);
  let guard = 0;
  while (queue.length && guard++ < 8000) {
    const [h, depth] = queue.shift();
    if (depth >= maxHops) continue;
    const node = txGraph.get(h);
    if (!node) continue;
    for (const nx of node[dir]) {
      if (seen.has(nx)) continue;
      seen.add(nx);
      out.add(nx);
      queue.push([nx, depth + 1]);
    }
  }
  return out;
}

/* ── DEX order settlement via spend-graph ────────────────────────────────
 * Open order / LP cards start as pulsing Pending. When a later dex_fill or
 * dex_cancel spends that order's tx (edge in txGraph / inputTxs), flip the
 * pill to Filled (green) or Cancelled (red) on the original card.
 *
 * Keyed by orderTx + venue: change UTxOs from an order tx can be spent by an
 * unrelated DEX cancel/fill. A Splash cancel must not lock a Minswap order.
 */
const dexOrderSettlement = new Map(); // `${orderTx}\0${dex}` -> { status, dex }
const pendingDexSettlements = [];     // fill/cancel events waiting on graph edges

function parentTxsOf(txHash) {
  if (!txHash) return [];
  const node = txGraph.get(txHash);
  if (node && node.ins.size) return [...node.ins];
  return [];
}

function dexSettlementKey(orderTx, dex) {
  return `${orderTx}\0${dex || ""}`;
}

function dexSettlementStatus(orderTx, dex) {
  if (!orderTx) return null;
  const dexStr = dex ? String(dex) : "";
  if (dexStr) {
    const hit = dexOrderSettlement.get(dexSettlementKey(orderTx, dexStr));
    return hit ? hit.status : null;
  }
  for (const [k, hit] of dexOrderSettlement) {
    if (k === orderTx || k.startsWith(`${orderTx}\0`)) return hit.status;
  }
  return null;
}

function dexStatusPillHtml(status) {
  if (status === "filled") return `<span class="badge filled">Filled</span>`;
  if (status === "cancelled") return `<span class="badge cancelled">Cancelled</span>`;
  if (status === "pending") {
    return `<span class="badge pending pulse-pending">Pending</span>`;
  }
  return "";
}

function updateDexOrderCardPill(card, status) {
  if (!card || !status) return;
  const sub = card.querySelector(".ev-sub");
  if (!sub) return;
  const pill = sub.querySelector(".badge.pending, .badge.filled, .badge.cancelled");
  const html = dexStatusPillHtml(status);
  if (!html) return;
  if (pill) pill.outerHTML = html;
  else sub.insertAdjacentHTML("beforeend", " " + html);
  card.dataset.settlement = status;
}

function parentHasDexOrder(orderTx, dex) {
  for (const ev of eventsForTx(orderTx)) {
    if (ev.kind !== "dex_order" && ev.kind !== "dex_lp") continue;
    if (dex && ev.data?.dex && ev.data.dex !== dex) continue;
    return true;
  }
  return false;
}

function markDexOrderSettled(orderTx, status, settleEv) {
  if (!orderTx || (status !== "filled" && status !== "cancelled")) return;
  const dex = settleEv?.data?.dex ? String(settleEv.data.dex) : "";
  const mapKey = dexSettlementKey(orderTx, dex);
  const prev = dexOrderSettlement.get(mapKey);
  if (prev && prev.status !== status) return; // first terminal status per venue
  dexOrderSettlement.set(mapKey, { status, dex });

  for (const ev of eventsForTx(orderTx)) {
    if (ev.kind !== "dex_order" && ev.kind !== "dex_lp") continue;
    if (dex && ev.data?.dex && ev.data.dex !== dex) continue;
    if (!ev.data || typeof ev.data !== "object") ev.data = {};
    ev.data.settlement = status;
  }

  const key = (window.CSS && CSS.escape) ? CSS.escape(orderTx) : orderTx;
  for (const card of feed.querySelectorAll(`.card[data-tx="${key}"]`)) {
    if (card.dataset.kind !== "dex_order" && card.dataset.kind !== "dex_lp") continue;
    if (dex && card.dataset.dex && card.dataset.dex !== dex) continue;
    updateDexOrderCardPill(card, status);
  }
}

function applyDexSettlement(settleEv) {
  if (!settleEv?.tx_hash) return;
  if (settleEv.kind !== "dex_fill" && settleEv.kind !== "dex_cancel") return;
  // The detector names the order this settlement closes, so the pairing is a
  // direct lookup with no tx-graph traversal.
  const stamped = settleEv.data?.orderTx;
  if (stamped) {
    const status = settleEv.kind === "dex_cancel" ? "cancelled" : "filled";
    markDexOrderSettled(String(stamped), status, settleEv);
    return;
  }
  const parents = parentTxsOf(settleEv.tx_hash);
  if (!parents.length) {
    if (!pendingDexSettlements.includes(settleEv)) pendingDexSettlements.push(settleEv);
    return;
  }
  const status = settleEv.kind === "dex_cancel" ? "cancelled" : "filled";
  const dex = settleEv?.data?.dex ? String(settleEv.data.dex) : "";
  // When a parent already has an order on this venue, only settle those —
  // other inputs are usually change / batcher UTxOs from unrelated txs.
  const venueParents = dex
    ? parents.filter((p) => parentHasDexOrder(p, dex))
    : [];
  const targets = venueParents.length ? venueParents : parents;
  for (const orderTx of targets) {
    markDexOrderSettled(orderTx, status, settleEv);
  }
}

function reconcileDexSettlementsForTx(txHash) {
  if (!txHash) return;
  for (let i = pendingDexSettlements.length - 1; i >= 0; i--) {
    const ev = pendingDexSettlements[i];
    if (ev.tx_hash !== txHash && !parentTxsOf(ev.tx_hash).length) continue;
    pendingDexSettlements.splice(i, 1);
    applyDexSettlement(ev);
  }
  for (const ev of [...eventsForTx(txHash)]) {
    if (ev.kind === "dex_fill" || ev.kind === "dex_cancel") applyDexSettlement(ev);
  }
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
  // Dead space between cards is still inside #feed — clear the cone so
  // related cards don't stay lit after the cursor leaves a card.
  if (!card || !feed.contains(card)) {
    clearLightCone();
    return;
  }
  const tx = card.dataset.tx || null;
  if (tx === lcTx) return;      // still inside the same tx's cards
  if (!tx) { clearLightCone(); return; } // blocks / rollbacks have no cone
  showLightCone(tx);
});
feed.addEventListener("mouseleave", clearLightCone);

const groups = new Map();      // block hash -> .block-group element
const groupOrder = [];         // newest first
const seenEventIds = new Set(); // dedupe when merging search hits into the feed
/** Soft safety cap while scrolling deep history. */
const MAX_GROUPS = 50_000;
/**
 * Hard ceiling on mounted block-groups (newest first). Safety net only —
 * view-based pruning normally keeps far fewer.
 */
const TIP_DOM_GROUPS = 250;
/** Always keep at least this many newest groups so the tip never goes empty. */
const TIP_DOM_MIN_GROUPS = 4;
/**
 * Mounted-card budget for the feed window.
 *
 * Per-append work — filtering, hierarchy sorting, pipe layout, pruning — is
 * proportional to what is mounted. Holding the mounted count near this number
 * keeps each additional page the same cost whether the reader is 50 events deep
 * or 5,000. Unmounted events remain in retentionCache and remount on scroll.
 */
const DOM_WINDOW_CARDS = 350;
/** Groups within this distance of the viewport are always kept mounted. */
const DOM_WINDOW_MARGIN_PX = 1500;
/**
 * Automatic tip paint / retention-absorb hard stop (block-groups). Prevents
 * mobile + flaky scrollHeight checks from dumping thousands of cards before
 * the user scrolls.
 */
const TIP_PAINT_MAX_GROUPS = 20;
/**
 * Drop a mounted block-group once it has been off-screen (unviewed) this long.
 * Intentionally loaded history is kept only while recently viewed; scroll again
 * to remount from retentionCache / disk.
 */
const VIEW_STALE_MS = 5 * 60 * 1000;
const pending = [];            // buffered events while user is reading
let oldestEventId = null;      // smallest id currently in the feed
let historyExhausted = false;
let historyLoading = false;
/** Coalesce scroll/wheel storms so sparse filters don't flash-load every frame. */
let historyLoadTimer = 0;
/** One load per approach to the history end; re-arms after leaving the end zone. */
let historyLoadArmed = true;
/**
 * Spinner is only shown when the viewer reaches the history end (scroll/wheel).
 * Background hydrate/network must not toggle it — that made loads feel laggy.
 */
let historyLoaderUserPinned = false;
/** Safety cap so a pathological loop can't spin forever on disk seeks. */
const HISTORY_SEEK_MAX_PAGES = 10_000;
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
/** tx hash -> Set of cached events carrying it. */
const eventsByTx = new Map();

function indexEventByTx(ev) {
  if (!ev?.tx_hash) return;
  let set = eventsByTx.get(ev.tx_hash);
  if (!set) eventsByTx.set(ev.tx_hash, (set = new Set()));
  set.add(ev);
}

function unindexEventByTx(ev) {
  if (!ev?.tx_hash) return;
  const set = eventsByTx.get(ev.tx_hash);
  if (!set) return;
  set.delete(ev);
  if (!set.size) eventsByTx.delete(ev.tx_hash);
}

/** Cached events for a tx hash, newest-first insertion order. */
function eventsForTx(txHash) {
  return txHash ? (eventsByTx.get(txHash) || EMPTY_EVENT_SET) : EMPTY_EVENT_SET;
}
const EMPTY_EVENT_SET = new Set();
let retentionReady = false;
let retentionLoading = null; // Promise while /api/buffer is in flight
let retentionLoadGen = 0; // bumped on reconnect so stale preloads don't notify
let retentionWaiters = []; // resolvers waiting for ready
/** Newest-first events matching current filters (from retentionCache). */
let feedHitBuffer = [];
/** How far through feedHitBuffer we have rendered into the DOM. */
let feedHitOffset = 0;
/** Index of the first mounted hit; the window runs [feedHitStart, feedHitOffset). */
let feedHitStart = 0;
/** True when retentionCache has gained events the hit list has not folded in. */
let feedHitsDirty = false;
/** Ids present in feedHitBuffer — lets hydration fold in only the delta. */
let feedHitIds = new Set();
/** Event id -> its index in feedHitBuffer. */
let feedHitIndexById = new Map();

function reindexFeedHits() {
  feedHitIndexById = new Map();
  for (let i = 0; i < feedHitBuffer.length; i++) {
    const id = feedHitBuffer[i]?.id;
    if (id != null) feedHitIndexById.set(id, i);
  }
}
/** Serialized filter state used to build feedHitBuffer. */
let feedHitKey = "";
/** True once feedHitBuffer has been fully rendered (further scroll → disk). */
let retentionHistoryDone = false;

const SEARCH_PRIME_LOOKBACK = 300;
/** Events rendered into the DOM per scroll page (from the in-memory 24h buffer). */
const FEED_PAGE_SIZE = 50;
/** Matches rendered per search page - keeps the DOM/scrollbar bounded. */
const SEARCH_PAGE_SIZE = 40;
/** Older-than-24h disk pages (only after retention is exhausted). */
const HISTORY_PAGE_SIZE = 40;
/** Larger pages while seeking sparse filter matches through disk. */
const HISTORY_SEEK_PAGE_SIZE = 200;
/**
 * Retention-window hydrate page size. Larger pages = fewer round trips.
 */
const BUFFER_PAGE_SIZE = 5000;
/**
 * When the filtered hit list is at most this size, paint every match on filter
 * change (sparse dApp/DEX views). Larger sets still tip-paint and scroll-load.
 */
/** Yield to the browser while indexing a hydrate chunk (keep rare — speed first). */
const RETENTION_INDEX_CHUNK = 2500;
/** Yield every N feed pages while painting so large match sets stay responsive. */
const PAINT_YIELD_EVERY = 3;

function yieldToBrowser() {
  return new Promise((resolve) => {
    // Prefer a macrotask over rAF so we don't stall a full frame between batches.
    setTimeout(resolve, 0);
  });
}

/** Bumped to cancel in-flight async feed rebuilds (filter change / retention). */
let feedRebuildGen = 0;
/** True while onFeedFiltersChanged is clearing/painting — skip tip DOM prune. */
let feedRebuilding = false;
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
    const cards = g.querySelectorAll(".card");
    // Pass 1: non-min-ADA hide reasons (needed to know which tx children stay).
    // min-ADA needs two passes to keep parents of visible children; every other
    // filter decides and applies in a single walk.
    const twoPass = minL > 0;
    const baseHide = twoPass ? new Map() : null;
    cards.forEach((card) => {
      let hide = false;
      if (q && !(card.dataset.search || "").includes(q)) hide = true;
      if (card.dataset.category === "finance" && !financeAppEnabled(financeNameOf(card))) hide = true;
      if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) hide = true;
      if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) hide = true;
      if (twoPass) {
        baseHide.set(card, hide);
        return;
      }
      card.classList.toggle("f-hide", hide);
      if (!hide && settings.filters[card.dataset.category]) visible++;
    });
    if (twoPass) {
      // Tx ids that still have a visible child — keep those parents despite min-ADA.
      const parentsWithKids = new Set();
      cards.forEach((card) => {
        if (baseHide.get(card)) return;
        if (!settings.filters[card.dataset.category]) return;
        const parent = card.dataset.parent;
        if (parent && card.dataset.kind !== "transaction") parentsWithKids.add(parent);
      });
      cards.forEach((card) => {
        let hide = baseHide.get(card) || false;
        if (
          card.dataset.category === "transaction"
          && Number(card.dataset.ada || 0) < minL
          && !parentsWithKids.has(card.dataset.eid)
        ) {
          hide = true;
        }
        card.classList.toggle("f-hide", hide);
        if (!hide && settings.filters[card.dataset.category]) {
          visible++;
        }
      });
    }
    // Collapse groups with nothing visible (category / venue / search filters),
    // so rare filtered events aren't separated by long empty spine stretches.
    // Orphaned blocks (and their detail events) are part of Forks & Battles —
    // hide the whole group when that filter is off.
    const hideOrphan = g.classList.contains("orphaned") && !settings.filters.alert;
    g.classList.toggle("f-hide", visible === 0 || hideOrphan);
    // Spine diamond cutout only when the Block header is actually laid out —
    // otherwise the cutout reads as a gap under the inter-block dotted segment.
    const blockCard = g.querySelector(":scope > .card-block");
    const blockSpine = !!(
      blockCard
      && settings.filters.block
      && !blockCard.classList.contains("f-hide")
      && !hideOrphan
      && visible > 0
    );
    g.classList.toggle("has-block-spine", blockSpine);
    // Drop empty event stacks (all children filtered) so their margins don't
    // open a blank stretch above the dotted inter-block spacer.
    const host = g.querySelector(":scope > .group-events");
    if (host) {
      const anyEvent = [...host.querySelectorAll(":scope > .card")].some((c) => {
        if (c.classList.contains("f-hide")) return false;
        if (!settings.filters[c.dataset.category]) return false;
        if (c.dataset.category === "finance" && !financeAppEnabled(financeNameOf(c))) return false;
        if (c.dataset.category === "dapp" && !dappAppEnabled(c.dataset.dapp)) return false;
        if (c.dataset.category === "governance" && !govTypeEnabled(c.dataset.govType)) {
          return false;
        }
        return true;
      });
      host.classList.toggle("is-empty", !anyEvent);
    }
  }
  // Dotted spine is inter-block only (never between cards in the same group).
  // • Blocks filter off: always dot between different blocks — otherwise abutting
  //   solid spines read as "same block" across minutes of chain time.
  // • Blocks filter on: diamonds already mark boundaries; only dot when filters
  //   hide content (or collapsed groups) between the two visible blocks.
  const filteredMarks = collectFilteredSpineMarks();
  const blocksVisible = !!settings.filters.block;
  for (let i = 0; i < groupOrder.length; i++) {
    const g = groupOrder[i];
    if (g.classList.contains("f-hide")) {
      g.classList.remove("spine-ellipsis");
      continue;
    }
    let nextVis = -1;
    for (let j = i + 1; j < groupOrder.length; j++) {
      if (!groupOrder[j].classList.contains("f-hide")) {
        nextVis = j;
        break;
      }
    }
    if (nextVis < 0 || !spineDifferentBlocks(g, groupOrder[nextVis])) {
      g.classList.remove("spine-ellipsis");
      continue;
    }
    const next = groupOrder[nextVis];
    const skippedGroups = nextVis > i + 1;
    const skippedFiltered = spineHasFilteredBetween(g, next, filteredMarks);
    const mark = blocksVisible
      ? (skippedGroups || skippedFiltered)
      : true;
    g.classList.toggle("spine-ellipsis", mark);
  }
  store.set("co_filters_v1", settings.filters);
  store.set("co_minada_v1", settings.minAda);
  store.set("co_finance_apps_v1", settings.financeApps);
  store.set("co_dapp_apps_v1", settings.dappApps);
  store.set("co_gov_types_v1", settings.govTypes);
  updateLoadedEventCount();
  if (pending.length) updateNewPill();
  if (!searchPriming && $("search").value.trim()) {
    updateSearchEmptyPrompt();
  } else if (!$("search").value.trim()) {
    hideSearchPrompts();
    // Rebuild from whatever is already cached (including mid-hydrate).
    if (feedFilterKey() !== feedHitKey) {
      queueMicrotask(() => onFeedFiltersChanged());
    }
  }
  updateFilterEasterEgg();
  // Drop / restore `.ev-child` indent when Transactions (or other filters) toggle,
  // so tip cards don't sit flush while older siblings stay indented.
  refreshHierarchyIndent();
}

/** True when every category chip is off — show the bathroom-robot easter egg. */
function allCategoryFiltersOff() {
  return CATS.every((c) => !settings.filters[c.id]);
}

function updateFilterEasterEgg() {
  const egg = $("filter-easter");
  if (!egg) return;
  const show = allCategoryFiltersOff();
  egg.classList.toggle("show", show);
  egg.hidden = !show;
  feed.classList.toggle("filter-easter-hide", show);
}

/** Visible (filter-matching) events currently shown in the feed. */
/**
 * Footer total: events from the chain tip down to the deepest one loaded, under
 * the active filters. Counting the mounted window instead would shrink as older
 * events are pruned, hiding the fact that history keeps growing.
 */
function countLoadedSpan() {
  const n = Math.max(feedHitOffset, 0);
  let oldest = Infinity;
  for (let i = 0; i < n && i < feedHitBuffer.length; i++) {
    const ts = Number(feedHitBuffer[i]?.timestamp || 0);
    if (ts > 0 && ts < oldest) oldest = ts;
  }
  const now = Math.floor(Date.now() / 1000);
  const hours = Number.isFinite(oldest) && oldest < now ? (now - oldest) / 3600 : 0;
  return { n: Math.min(n, feedHitBuffer.length), hours };
}

function countVisibleEvents() {
  let n = 0;
  let oldest = Infinity;
  document.querySelectorAll("#feed .block-group:not(.f-hide) .card:not(.f-hide)").forEach((card) => {
    if (!settings.filters[card.dataset.category]) return;
    if (card.dataset.category === "finance" && !financeAppEnabled(financeNameOf(card))) return;
    if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return;
    if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return;
    n++;
    const ts = Number(card.querySelector(".ev-time")?.dataset.ts || 0);
    if (ts > 0 && ts < oldest) oldest = ts;
  });
  // Lookback from now → oldest visible card (not newest−oldest). A feed whose
  // tip is a few hours old and whose tail is 23h old should read "23h", matching
  // the "23h ago" labels on cards.
  const now = Math.floor(Date.now() / 1000);
  const hours = Number.isFinite(oldest) && oldest < now ? (now - oldest) / 3600 : 0;
  return { n, hours };
}

/** Footer count: filtered visible events only (not the full loaded total). */
function updateLoadedEventCount() {
  const el = $("ft-session");
  if (!el) return;
  const { n, hours } = countLoadedSpan();
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

/** Compact span label: `12m`, `3.5h`, `24h`. Hour labels use floor so they
 *  match card "Nh ago" text (also floored). */
function formatHistorySpan(hours) {
  if (!(hours > 0)) return "";
  if (hours < 1) return `${Math.max(1, Math.round(hours * 60))}m`;
  if (hours < 10) {
    const t = Math.round(hours * 10) / 10;
    return `${t}h`;
  }
  // Round to the nearest hour: a full 24h window ends one block short of the
  // boundary (~23.98h), and flooring reported that as 23h.
  return `${Math.round(hours)}h`;
}

function paintCatCounts() {
  for (const c of CATS) {
    const el = document.querySelector(`[data-cat-n="${c.id}"]`);
    if (el) el.textContent = catCounts[c.id] ? fmtInt(catCounts[c.id]) : "";
  }
}

/** Pre-fill the search box from the URL — see [`parseUrlQuery`]. */
function searchFromUrl() {
  return parseUrlQuery().search;
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
    const ids = options.map((opt) => (typeof opt === "string" ? opt : opt.id));
    const allOn = ids.length > 0 && ids.every((id) => isEnabled(id));

    const allRow = document.createElement("button");
    allRow.type = "button";
    allRow.className = "chip-sub-opt chip-sub-all" + (allOn ? " on" : "");
    allRow.setAttribute("role", "menuitemcheckbox");
    allRow.setAttribute("aria-checked", allOn ? "true" : "false");
    allRow.innerHTML =
      `<span class="chip-sub-check" aria-hidden="true">${allOn ? "✓" : ""}</span>` +
      `<span class="chip-sub-label">All</span>`;
    allRow.onclick = (e) => {
      e.stopPropagation();
      // All on → turn all off; otherwise turn all on (sync mixed → on).
      const next = !allOn;
      for (const id of ids) settings[settingsKey][id] = next;
      rebuildMenu();
      applyFilters();
    };
    menu.appendChild(allRow);

    const sep = document.createElement("div");
    sep.className = "chip-sub-sep";
    sep.setAttribute("role", "separator");
    menu.appendChild(sep);

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

/** Turn every category / venue / subtype filter back on; clear min-ADA and search. */
function resetFilters() {
  for (const c of CATS) settings.filters[c.id] = true;
  for (const v of FINANCE_APPS) settings.financeApps[v] = true;
  for (const a of DAPP_APPS) settings.dappApps[a] = true;
  for (const g of GOV_TYPES) settings.govTypes[g.id] = true;
  settings.minAda = 0;
  urlSearchPreset = "";

  const minAda = $("min-ada");
  if (minAda) minAda.value = "0";

  // Sync chip UI without rebuilding (split chips register document listeners).
  for (const wrap of document.querySelectorAll("#chips .chip-wrap")) {
    wrap.classList.add("on");
    wrap.querySelector(".chip-main")?.classList.add("on");
  }
  for (const chip of document.querySelectorAll("#chips > .chip")) {
    chip.classList.add("on");
  }

  const search = $("search");
  if (search?.value.trim() || searchPriming) {
    search.value = "";
    // cancelSearchPrime clears prime state and calls applyFilters.
    cancelSearchPrime();
    return;
  }

  applyFilters();
}

function buildToolbar() {
  const chips = $("chips");
  for (const c of CATS) {
    if (c.id === "finance") {
      buildSplitFilterChip(chips, {
        catId: "finance",
        label: "Finance",
        iconKind: "dex",
        settingsKey: "financeApps",
        options: FINANCE_APPS,
        isEnabled: financeAppEnabled,
        menuAria: "Filter by DEX or finance app",
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

  $("reset-filters-btn").onclick = () => resetFilters();

  const layoutBtn = $("layout-btn");
  const syncLayoutChrome = () => {
    layoutBtn.textContent = settings.layout === "vertical" ? "vertical" : "horizontal";
    feed.className = settings.layout;
    document.body.classList.toggle("layout-horizontal", settings.layout === "horizontal");
    const dir = $("newpill-dir");
    if (dir) dir.textContent = settings.layout === "horizontal" ? "◀" : "▲";
  };
  syncLayoutChrome();
  layoutBtn.onclick = () => {
    clearLightCone();
    settings.layout = settings.layout === "vertical" ? "horizontal" : "vertical";
    store.set("co_layout_v1", settings.layout);
    syncLayoutChrome();
    scheduleHierarchyPipes();
  };

  const compactBtn = $("compact-btn");
  document.body.classList.toggle("compact", settings.compact);
  compactBtn.classList.toggle("on", settings.compact);
  compactBtn.onclick = () => {
    settings.compact = !settings.compact;
    document.body.classList.toggle("compact", settings.compact);
    compactBtn.classList.toggle("on", settings.compact);
    store.set("co_compact_v1", settings.compact);
    requestAnimationFrame(() => layoutTxStakes());
    scheduleHierarchyPipes();
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

/** Stake/DRep delegation: `who → to` or `who → from → to` (arrows, not bullets). */
const delegationFlow = (who, from, to) => {
  const parts = [who, from, to].filter(Boolean);
  if (!parts.length) return "";
  return parts
    .map((p) => `<span class="sub-i">${p}</span>`)
    .join('<span class="sep"> → </span>');
};

/**
 * Asset chip row. Optional `badge` sits inside the same `.assets` flex row so
 * labels like "collateral" stay beside the chips (the whole card body lives in
 * a wrapping `.ev-sub`, so a bare badge sibling would land on the meta line).
 */
function assetChipsHtml(assets, badge) {
  if (!assets || !assets.items || !assets.items.length) return "";
  const chips = assets.items
    .map((a) => {
      const unit = a.unit || "";
      const decimals = tokenDecimals(a);
      const label = tokenLabel(a);
      const scamCls = a.scam ? " scam" : "";
      const title = a.scam
        ? `Known scam token · ${a.policy || ""}.${a.nameHex || ""}`
        : `${a.policy || ""}.${a.nameHex || ""}`;
      return `<span class="asset${scamCls}" data-unit="${esc(unit)}" title="${esc(title)}">
        <span class="ph">◆</span><span class="t">${esc(label)}</span><span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span></span>`;
    })
    .join("");
  const more = assets.more ? `<span class="asset"><span class="t">+${assets.more} more</span></span>` : "";
  const badgeHtml = badge
    ? `<span class="badge${badge.cls ? ` ${esc(badge.cls)}` : ""}">${esc(badge.text)}</span>`
    : "";
  return `<div class="assets">${badgeHtml}${chips}${more}</div>`;
}

/** Plain `₳ 123` / `16,490 USDCx` - no chip chrome on DEX cards. */
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

/** Blockfrost / Ogmios spellings → same labels the server emits. */
function normalizeDrepLabel(id) {
  if (!id) return "";
  const s = String(id).trim();
  if (
    s === "drep_always_abstain"
    || s === "always_abstain"
    || s === "alwaysAbstain"
    || s === "abstain"
    || s === "Always Abstain"
  ) return "Always Abstain";
  if (
    s === "drep_always_no_confidence"
    || s === "always_no_confidence"
    || s === "alwaysNoConfidence"
    || s === "noConfidence"
    || s === "Always No Confidence"
  ) return "Always No Confidence";
  return s;
}

/** Hide pool / DRep cards that only “redelegate” to the same target. */
function isNoopRedelegation(ev) {
  const d = ev?.data || {};
  if (ev.kind === "delegation") {
    const from = d.fromPool;
    const to = d.pool;
    return !!(from && to && from === to);
  }
  if (ev.kind === "vote_delegation") {
    const from = d.fromDrep;
    const to = d.drep;
    return !!(from && to && normalizeDrepLabel(from) === normalizeDrepLabel(to));
  }
  return false;
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
  const settled = d.settlement
    || dexSettlementStatus(ev.tx_hash, d.dex)
    || (d.filled ? "filled" : null);
  if (settled === "filled") return dexStatusPillHtml("filled");
  if (settled === "cancelled" || ev.kind === "dex_cancel") {
    return dexStatusPillHtml("cancelled");
  }
  // Fill cards are the settlement itself — no status pill.
  if (ev.kind === "dex_fill") return "";
  if (ev.kind === "dex_lp") return dexStatusPillHtml("pending");
  if (ev.kind === "dex_order" && (d.side === "buy" || d.side === "sell")) {
    return dexStatusPillHtml("pending");
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
      // Stakes sit outside `sub()` so they can flex-grow and fill remaining
      // card width (same idea as token-transfer asset chips).
      return sub([
        `<b>${fmtAda(d.ada)}</b>`,
        `${fmtInt(d.inputs)} in → ${fmtInt(d.outputs)} out`,
        `fee ${fmtAda(d.fee)}`,
        d.script ? `<span class="badge contract">contract</span>` : "",
        d.assets ? `${fmtInt(d.assets)} asset${d.assets > 1 ? "s" : ""}` : "",
      ]) + txStakesHtml(d.stakes);
    case "token_transfer":
      return (d.scam ? `<span class="badge scam">scam token</span>` : "") + assetChipsHtml(d.assets);
    case "mint":
      return `<span class="badge plus">mint</span>` + assetChipsHtml(d.assets);
    case "burn":
      return `<span class="badge minus">burn</span>` + assetChipsHtml(d.assets);
    case "delegation": {
      // stake (or $handle) → pool; redelegations: stake → from → to
      const who = stakeSpan(d.stake, 12, 5);
      const from = d.fromPool
        ? `<span class="pool-id" data-pool="${esc(d.fromPool)}" title="${esc(d.fromPool)}">${esc(short(d.fromPool, 10, 4))}</span>`
        : "";
      const to = d.pool
        ? `<span class="pool-id" data-pool="${esc(d.pool)}" title="${esc(d.pool)}">${esc(short(d.pool, 10, 4))}</span>`
        : "";
      return delegationFlow(who, from, to);
    }
    case "vote_delegation": {
      // stake (or $handle) → DRep; redelegations: stake → from → to
      const who = stakeSpan(d.stake, 12, 5);
      const from = d.fromDrep
        ? drepSpan(d.fromDrep, d.fromDrepName)
        : "";
      const to = drepSpan(d.drep, d.drepName);
      return delegationFlow(who, from, to);
    }
    case "stake_registration":
    case "stake_deregistration":
      return stakeSpan(d.stake, 14, 6);
    case "withdrawal":
      return sub([
        `<b>${fmtAda(d.lovelace)}</b>`,
        stakeSpan(d.account, 12, 5),
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
      const vote = `<span class="badge ${cls}">${esc(v.toUpperCase())}</span>`;
      const prop = d.proposalTitle
        ? `<span class="gov-title" data-gov="${esc(govKey)}" title="${esc(d.proposalTx || "")}#${esc(String(d.proposalIndex ?? 0))}">${esc(d.proposalTitle)}</span>`
        : d.proposalTx
          ? `<span class="hash" data-gov="${esc(govKey)}" title="${esc(d.proposalTx)}#${esc(String(d.proposalIndex ?? 0))}">${esc(short(d.proposalTx, 8, 4))}#${esc(String(d.proposalIndex ?? 0))}</span>`
          : "";
      return delegationFlow(govVoteVoterSpan(d), vote, prop);
    }
    case "drep_registration":
    case "drep_update":
    case "drep_retirement":
      return d.drep ? drepSpan(d.drep, d.drepName) : "";
    case "tx_metadata":
      return d.msg
        ? `<span style="font-style:italic">“${esc(String(d.msg).slice(0, 160))}”</span>`
        : // One flex item per label so chips wrap inside the card.
          sub((d.labels || []).slice(0, 6).map((l) => `<span class="hash">label ${esc(l)}</span>`));
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
      return sub([flow, status, actorSpan(d)]);
    }
    case "dapp_activity": {
      if (renderDappActivityHtml) {
        return renderDappActivityHtml(d, {
          esc,
          fmtAda,
          fmtTokenQty,
          assetChipsHtml,
          actorSpan,
          sub,
        });
      }
      // Pack absent: still show a minimal card for historical dApp events.
      return sub([
        `<span class="badge contract">${esc(d.dapp || "dApp")}</span>`,
        actorSpan(d),
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
  const voter = typeof d.voter === "string" && d.voter.startsWith("pool1") ? d.voter : null;
  return [d.issuerPool, d.pool, d.fromPool, voter].filter((id) => typeof id === "string" && id);
}

function isLookupPoolId(id) {
  return typeof id === "string" && id.startsWith("pool1") && id.length >= 50 && id.length <= 120;
}

function isLookupDrepId(id) {
  return typeof id === "string"
    && (id.startsWith("drep1") || id.startsWith("drep_script1"))
    && id.length >= 50
    && id.length <= 120;
}

function isLookupStakeAddr(addr) {
  return typeof addr === "string"
    && (addr.startsWith("stake1") || addr.startsWith("stake_test1"))
    && addr.length >= 50
    && addr.length <= 120;
}

function isLookupPaymentAddr(addr) {
  return typeof addr === "string"
    && (addr.startsWith("addr1") || addr.startsWith("addr_test1"))
    && addr.length >= 50
    && addr.length <= 120;
}

function isLookupHandleAddr(addr) {
  return isLookupStakeAddr(addr) || isLookupPaymentAddr(addr);
}

/** Actor on dex/dapp cards: `data.stake` preferred, else payment `data.address`. */
function actorSpan(d, head = 12, tail = 5) {
  if (!d || typeof d !== "object") return "";
  if (d.stake) return handleSpan(d.stake, head, tail);
  if (d.address) return handleSpan(d.address, head, tail);
  return "";
}

/** All stake pills; `layoutTxStakes` hides what won't fit and adds a +N pill. */
function txStakesHtml(stakes) {
  const list = Array.isArray(stakes)
    ? stakes.filter((s) => typeof s === "string" && s)
    : [];
  if (!list.length) return "";
  return `<span class="tx-stakes">${list.map((s) => handleSpan(s, 10, 4)).join("")}</span>`;
}

function stakeMoreTitle(addrs) {
  return addrs.map((s) => {
    const h = handleMeta.get(s)?.handle;
    return h ? `$${h}` : short(s, 12, 5);
  }).join(" · ");
}

/** Fit stake/handle pills into the available card width; overflow → +N. */
function layoutTxStakes(root = document) {
  const nodes = root.querySelectorAll
    ? root.querySelectorAll(".tx-stakes")
    : [];
  nodes.forEach(fitTxStakes);
}

function fitTxStakes(el) {
  if (!el || !el.isConnected) return;
  el.querySelector(".stake-more")?.remove();
  const pills = [...el.children].filter((c) => c.classList.contains("hash")
    || c.classList.contains("ada-handle"));
  if (!pills.length) return;

  // Reveal everything to measure true widths.
  for (const p of pills) {
    p.hidden = false;
    p.style.removeProperty("display");
  }

  const avail = el.clientWidth;
  if (avail <= 1) return;

  const styles = getComputedStyle(el);
  const gap = parseFloat(styles.columnGap || styles.gap) || 4;

  const widths = pills.map((p) => p.getBoundingClientRect().width);
  const allWidth = widths.reduce((s, w, i) => s + w + (i ? gap : 0), 0);
  if (allWidth <= avail + 0.5) return; // everything fits

  // Probe +N width (worst-case digit count for this list).
  const probe = document.createElement("span");
  probe.className = "stake-more";
  probe.textContent = `+${pills.length}`;
  probe.style.visibility = "hidden";
  el.appendChild(probe);
  const moreW = probe.getBoundingClientRect().width;
  probe.remove();

  let used = 0;
  let fit = 0;
  for (let i = 0; i < pills.length; i++) {
    const next = used + (fit ? gap : 0) + widths[i];
    if (next + gap + moreW > avail + 0.5) break;
    used = next;
    fit++;
  }
  // Prefer at least one pill when the row can hold pill + +N.
  if (fit === 0 && widths[0] + gap + moreW <= avail + 0.5) fit = 1;

  const hiddenAddrs = [];
  pills.forEach((p, i) => {
    if (i < fit) {
      p.hidden = false;
    } else {
      p.hidden = true;
      hiddenAddrs.push(p.dataset.handle || p.dataset.stake || p.title || "");
    }
  });

  if (!hiddenAddrs.length) return;
  const more = document.createElement("span");
  more.className = "stake-more";
  more.textContent = `+${hiddenAddrs.length}`;
  more.title = stakeMoreTitle(hiddenAddrs.filter(Boolean));
  el.appendChild(more);
}

let txStakesRo = null;
function ensureTxStakesObserver() {
  if (txStakesRo || typeof ResizeObserver === "undefined") return;
  txStakesRo = new ResizeObserver((entries) => {
    for (const e of entries) layoutTxStakes(e.target);
  });
}

/** Truncated address; enrichHandles swaps in `$handle` when known. */
function stakeSpan(addr, head = 12, tail = 5) {
  return handleSpan(addr, head, tail);
}

function handleSpan(addr, head = 12, tail = 5) {
  if (!addr) return "";
  const s = String(addr);
  if (!isLookupHandleAddr(s)) {
    return `<span class="hash" title="${esc(s)}">${esc(short(s, head, tail))}</span>`;
  }
  const known = handleMeta.get(s)?.handle || "";
  if (known) {
    return `<span class="ada-handle" data-handle="${esc(s)}" title="${esc(s)}"><span class="ada-handle-dollar">$</span>${esc(known)}</span>`;
  }
  return `<span class="hash" data-handle="${esc(s)}" title="${esc(s)}">${esc(short(s, head, tail))}</span>`;
}

/** Modal/detail address: truncated or `$handle`, click-to-copy full address. */
function addressSpan(addr, head = 14, tail = 8) {
  if (!addr || addr === "?") {
    return `<span class="hash">?</span>`;
  }
  const s = String(addr);
  if (!isLookupHandleAddr(s)) {
    return `<span class="hash copyable" data-copy="${esc(s)}" title="click to copy">${esc(short(s, head, tail))}</span>`;
  }
  const known = handleMeta.get(s)?.handle || "";
  if (known) {
    return `<span class="ada-handle copyable" data-handle="${esc(s)}" data-copy="${esc(s)}" title="click to copy"><span class="ada-handle-dollar">$</span>${esc(known)}</span>`;
  }
  return `<span class="hash copyable" data-handle="${esc(s)}" data-copy="${esc(s)}" title="click to copy">${esc(short(s, head, tail))}</span>`;
}

/** Prefer cached pool ticker/name; enrichPools fills misses via /api/pool. */
function poolSpan(id) {
  if (!id) return "";
  const meta = poolMeta.get(id);
  const ticker = meta?.ticker;
  const name = meta?.name;
  if (ticker || name) {
    const title = [name && name !== ticker ? name : null, id].filter(Boolean).join(" · ");
    return `<span class="pool-ticker pool-id" data-pool="${esc(id)}" title="${esc(title)}">${esc(ticker || name)}</span>`;
  }
  return `<span class="pool-id hash" data-pool="${esc(id)}" title="${esc(id)}">${esc(short(id, 10, 4))}</span>`;
}

/** DRep / SPO / CC voter on governance vote cards. */
function govVoteVoterSpan(d) {
  if (!d?.voter) return "";
  const voter = String(d.voter);
  if (d.role === "stakePoolOperator" || isLookupPoolId(voter)) return poolSpan(voter);
  if (d.role === "delegateRepresentative" || isLookupDrepId(voter) || d.voterName) {
    return drepSpan(voter, d.voterName);
  }
  return `<span class="hash" title="${esc(voter)}">${esc(short(voter, 10, 4))}</span>`;
}

/** Prefer stamped/cached givenName; enrichDreps fills misses via /api/drep. */
function drepSpan(id, stampedName) {
  if (!id) return "";
  const s = normalizeDrepLabel(id);
  if (s === "Always Abstain" || s === "Always No Confidence") {
    return `<b title="${esc(s)}">${esc(s)}</b>`;
  }
  if (!isLookupDrepId(s)) {
    const label = s.length > 24 ? short(s, 10, 5) : s;
    return `<span class="hash" title="${esc(s)}">${esc(label)}</span>`;
  }
  const known = (typeof stampedName === "string" && stampedName)
    || drepMeta.get(s)?.name
    || drepMeta.get(String(id))?.name
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
  const stakeList = [
    d.stake,
    d.account,
    d.address,
    ...(Array.isArray(d.stakes) ? d.stakes : []),
  ].filter(isLookupHandleAddr);
  for (const addr of stakeList) {
    bits.push(addr);
    const h = handleMeta.get(addr)?.handle;
    if (h) {
      bits.push(h);
      bits.push(`$${h}`);
    }
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
/** Fast lookup for ensureBlockCard / parent wiring. */
const blocksByHash = new Map(); // block_hash -> block event
const txByHash = new Map();     // tx_hash -> transaction event

/** Index one event into the client-side 24h retention cache. */
function retentionIndex(ev) {
  if (!ev || ev.id == null) return;
  if (!keepDexEvent(ev) || isNoopRedelegation(ev)) return;
  if (ev.kind === "orphaned_block" && ev.block_hash) {
    orphanedBlocks.add(ev.block_hash);
  }
  retentionCache.set(ev.id, { ev, hay: cardSearchText(ev) });
  indexEventByTx(ev);
  // Only a backward insert (hydrate / backfill) can add hits the paging cursor
  // has not passed; tip events are newer than the buffer and the tip path paints
  // them directly.
  if (!feedHitBuffer.length || ev.id < feedHitBuffer[0]?.id) feedHitsDirty = true;
  if (ev.kind === "block" && ev.block_hash) blocksByHash.set(ev.block_hash, ev);
  if (ev.kind === "transaction" && ev.tx_hash) txByHash.set(ev.tx_hash, ev);
  indexTxGraph(ev);
  if (ev.kind === "dex_fill" || ev.kind === "dex_cancel") applyDexSettlement(ev);
  if (ev.kind === "dex_order" || ev.kind === "dex_lp") {
    const st = dexSettlementStatus(ev.tx_hash, ev.data?.dex);
    if (st) {
      if (!ev.data || typeof ev.data !== "object") ev.data = {};
      ev.data.settlement = st;
    }
  }
  noteLoadedEvent(ev);
}

function retentionTrim() {
  if (!retentionHours || retentionCache.size === 0) return;
  const cutoff = Math.floor(Date.now() / 1000) - retentionHours * 3600;
  for (const [id, row] of retentionCache) {
    if ((row.ev.timestamp || 0) < cutoff) {
      retentionCache.delete(id);
      unindexEventByTx(row.ev);
      const ev = row.ev;
      if (ev.kind === "block" && ev.block_hash && blocksByHash.get(ev.block_hash) === ev) {
        blocksByHash.delete(ev.block_hash);
      }
      if (ev.kind === "transaction" && ev.tx_hash && txByHash.get(ev.tx_hash) === ev) {
        txByHash.delete(ev.tx_hash);
      }
      unindexTxGraph(ev);
      forgetLoadedEvent(ev);
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
 * Background-hydrate the full 24h retention window into retentionCache in
 * back-to-back pages (prefetch the next chunk while indexing the current one).
 * Tip snapshot paints first; absorb runs without blocking the fetch pipeline.
 * Disk history is only used after this hydrate completes.
 */
function startRetentionPreload(force = false) {
  if (!force && retentionLoading) return retentionLoading;
  const gen = ++retentionLoadGen;
  retentionReady = false;
  retentionHistoryDone = false;
  retentionLoading = (async () => {
    let absorbChain = Promise.resolve();
    const queueAbsorb = (final = false) => {
      absorbChain = absorbChain
        .then(() => {
          if (gen !== retentionLoadGen) return;
          return absorbRetentionHits({ gen, final });
        })
        .catch(() => { /* best-effort */ });
      return absorbChain;
    };

    try {
      // Newest page first (before=max), then walk older until the window is done.
      let before = Number.MAX_SAFE_INTEGER;
      /** @type {Promise<object> | null} */
      let pending = null;
      let guard = 0;
      while (guard++ < 10_000) {
        if (gen !== retentionLoadGen) return;

        const m = pending
          ? await pending
          : await (async () => {
              const r = await fetch(
                `/api/buffer?before=${before}&limit=${BUFFER_PAGE_SIZE}`,
              );
              if (!r.ok) throw new Error("buffer fetch failed");
              return r.json();
            })();
        pending = null;
        if (gen !== retentionLoadGen) return;

        if (m.buffered != null) bufferedEventCount = Number(m.buffered) || bufferedEventCount;
        if (m.retention_hours != null) retentionHours = Number(m.retention_hours) || retentionHours;

        const events = m.events || [];
        let minId = before;
        for (const ev of events) {
          if (ev?.id != null && ev.id < minId) minId = ev.id;
        }

        // Prefetch the next older page immediately — don't wait on index/absorb.
        const more = !m.exhausted && events.length && minId < before;
        if (more) {
          const nextBefore = minId;
          pending = fetch(
            `/api/buffer?before=${nextBefore}&limit=${BUFFER_PAGE_SIZE}`,
          ).then((r) => {
            if (!r.ok) throw new Error("buffer fetch failed");
            return r.json();
          });
        }

        for (let i = 0; i < events.length; i++) {
          if (gen !== retentionLoadGen) return;
          retentionIndex(events[i]);
          if (i > 0 && i % RETENTION_INDEX_CHUNK === 0) await yieldToBrowser();
        }
        if (gen !== retentionLoadGen) return;

        bufferedEventCount = Math.max(bufferedEventCount, retentionCache.size);
        updateLoadedEventCount();
        paintCatCounts();
        refreshActivityMonitor();

        // Absorb in parallel with the in-flight prefetch — never stall the pipeline.
        queueAbsorb(false);

        if (!more) break;
        before = minId;
      }
      if (gen !== retentionLoadGen) return;
      retentionTrim();
      bufferedEventCount = Math.max(bufferedEventCount, retentionCache.size);
      updateLoadedEventCount();
      paintCatCounts();
      refreshActivityMonitor();
      await queueAbsorb(true);
    } catch (e) {
      if (gen === retentionLoadGen) console.warn("retention preload failed", e);
    } finally {
      if (gen === retentionLoadGen) {
        notifyRetentionReady();
        retentionLoading = null;
        queueMicrotask(() => { absorbRetentionHits({ final: true }); });
      }
    }
  })();
  return retentionLoading;
}

/**
 * Merge newly cached retention hits into the visible feed.
 * Does not clear the DOM — tip stays up while 24h hydrates underneath.
 * Only fills toward one viewport; older matches wait for history scroll.
 */
async function absorbRetentionHits(opts = {}) {
  if ($("search").value.trim()) return;
  if (feedRebuilding) return;
  if (opts.gen != null && opts.gen !== retentionLoadGen) return;
  // Tip already seeded — keep hydrating retentionCache only; don't keep painting.
  // The hit list is left dirty and folded in lazily when something needs it.
  if (tipPaintBudgetExceeded() || (groupOrder.length >= TIP_DOM_MIN_GROUPS && visibleFeedFillsPage())) {
    return;
  }

  ensureFeedHitBuffer();
  // Seed the tip once; afterwards hydration only keeps the hit buffer fresh.
  if (groupOrder.length) return;
  await paintFeedHitPages({ pages: 1, gen: feedRebuildGen, sync: true });
  resortFeedBySlot();
  applyFilters();
  pruneTipDomGroups();
  // Never auto-arm history load from absorb — scroll/wheel owns that.
}

/**
 * Category / gov-type / DEX venue / min-ADA (search text is separate).
 * Pass `{ ignoreMinAda: true }` when pulling a parent Transaction for hierarchy
 * so a small tx isn't dropped while its DEX/token children still show.
 */
function eventPassesFeedFilters(ev, opts = {}) {
  if (!ev || !keepDexEvent(ev) || isNoopRedelegation(ev)) return false;
  if (!settings.filters[ev.category]) return false;
  // Orphaned-block content is gated by Forks & Battles (not only alert cards).
  if (
    !settings.filters.alert
    && ev.block_hash
    && orphanedBlocks.has(ev.block_hash)
  ) {
    return false;
  }
  if (ev.category === "finance"
    && !financeAppEnabled(String(ev.data?.dex || ev.data?.dapp || ""))) return false;
  if (ev.category === "dapp" && !dappAppEnabled(ev.data?.dapp)) return false;
  if (ev.category === "governance") {
    const gt = govTypeKey(ev);
    if (gt && !govTypeEnabled(gt)) return false;
  }
  if (!opts.ignoreMinAda && settings.minAda > 0 && ev.category === "transaction") {
    if (Number(ev.data?.ada || 0) < settings.minAda * 1e6) return false;
  }
  return true;
}

/** Parent Transaction event for a tx-scoped child (via parent_id or tx_hash). */
function parentTransactionEvent(ev) {
  if (!ev || ev.kind === "block" || ev.kind === "transaction") return null;
  if (ev.parent_id != null) {
    const row = retentionCache.get(ev.parent_id);
    if (row?.ev?.kind === "transaction") return row.ev;
  }
  if (ev.tx_hash) return txByHash.get(ev.tx_hash) || null;
  return null;
}

function feedFilterKey() {
  return JSON.stringify({
    f: settings.filters,
    g: settings.govTypes,
    d: settings.financeApps,
    a: settings.dappApps,
    m: settings.minAda,
  });
}

/** Newest-first by slot (chain order). Within a slot, the block header comes
 *  first so paged loads never paint orphan child groups without their Block. */
function sortEventsNewestFirst(events) {
  events.sort((a, b) => {
    const slotDiff = (b.slot || 0) - (a.slot || 0);
    if (slotDiff) return slotDiff;
    const aBlock = a.kind === "block" ? 0 : 1;
    const bBlock = b.kind === "block" ? 0 : 1;
    if (aBlock !== bBlock) return aBlock - bBlock;
    return (b.id || 0) - (a.id || 0);
  });
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

/**
 * When parent_id is missing (demo / older rows), attach children to the
 * Transaction card that shares their tx_hash so hierarchy sort still works.
 * Only indent when the Transactions filter is on *and* the parent card is in
 * the DOM — otherwise tip inserts (which skip mounting parents when the filter
 * is off) sit flush while older cards keep a stale `.ev-child` from a hidden
 * transaction still left in the group.
 */
function resolveCardParents(host) {
  const txIds = new Map();
  const byEid = new Map();
  for (const card of host.querySelectorAll(":scope > .card")) {
    if (card.dataset.eid) byEid.set(card.dataset.eid, card);
    if (card.dataset.kind === "transaction" && card.dataset.tx && card.dataset.eid) {
      txIds.set(card.dataset.tx, card.dataset.eid);
    }
  }
  const indentChildren = !!settings.filters.transaction;
  for (const card of host.querySelectorAll(":scope > .card")) {
    if (card.dataset.kind === "block" || card.dataset.kind === "transaction") continue;
    if (!card.dataset.parent && card.dataset.tx && txIds.has(card.dataset.tx)) {
      card.dataset.parent = txIds.get(card.dataset.tx);
    }
    const parentMounted =
      indentChildren
      && card.dataset.parent
      && byEid.has(card.dataset.parent);
    card.classList.toggle("ev-child", !!parentMounted);
  }
}

/** Recompute child indent + pipes after filter changes (no full feed rebuild). */
function refreshHierarchyIndent() {
  for (const g of groupOrder) {
    const host = g.querySelector(".group-events");
    if (host) resolveCardParents(host);
  }
  scheduleHierarchyPipes();
}

function resortFeedBySlot() {
  // Re-appending every group resets the browser's scroll anchoring and reads as
  // a flash when history pages land under sparse filters. Pin the viewport.
  const y = window.scrollY;
  const x = feed.scrollLeft;
  const items = [...feed.querySelectorAll(":scope > .block-group")];
  for (const g of items) {
    repairBlockGroup(g);
    const host = g.querySelector(".group-events");
    if (host) {
      resolveCardParents(host);
      const cards = [...host.querySelectorAll(":scope > .card")];
      const sorted = [...cards].sort(cmpHierarchy);
      // Re-append only when the order actually changed; each move costs a reflow.
      if (sorted.some((c, i) => c !== cards[i])) {
        for (const c of sorted) host.appendChild(c);
      }
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
  scheduleHierarchyPipes();
}

/* ── Hierarchy pipes (measured SVG, not CSS pixel guesses) ────────────── */

const SVG_NS = "http://www.w3.org/2000/svg";
let hierarchyPipeTimer = 0;
let hierarchyPipeMaxTimer = 0;
let hierarchyPipeDrawing = false;
let hierarchyPipeRo = null;
let hierarchyPipeMo = null;

function hierarchyGeom() {
  const cs = getComputedStyle(feed);
  return {
    dot: parseFloat(cs.getPropertyValue("--event-dot")) || 6,
    gap: parseFloat(cs.getPropertyValue("--event-dot-gap")) || 10,
  };
}

/** Visible in layout (not f-hide and not display:none via category CSS). */
function cardPipeVisible(card) {
  if (card.classList.contains("f-hide")) return false;
  return card.getClientRects().length > 0;
}

/** Parent event-dot center / child elbow, in host-local coordinates. */
function hierarchyAnchor(card, hostRect, geom, role) {
  const r = card.getBoundingClientRect();
  const bl = parseFloat(getComputedStyle(card).borderLeftWidth) || 0;
  const y = r.top + r.height / 2 - hostRect.top;
  if (role === "dot") {
    return {
      x: r.left + bl - geom.gap - hostRect.left,
      y,
    };
  }
  // Elbow ends at the child's outer left edge (colored border).
  return { x: r.left - hostRect.left, y };
}

function ensurePipeSvg(host) {
  let svg = host.querySelector(":scope > .ev-pipes");
  if (!svg) {
    svg = document.createElementNS(SVG_NS, "svg");
    svg.classList.add("ev-pipes");
    svg.setAttribute("aria-hidden", "true");
    host.insertBefore(svg, host.firstChild);
    hierarchyPipeRo?.observe(host);
  }
  return svg;
}

function clearGroupPipes(host) {
  const svg = host.querySelector(":scope > .ev-pipes");
  if (svg) svg.replaceChildren();
}

function drawGroupHierarchyPipes(host) {
  if (!feed.classList.contains("vertical") || host.closest(".block-group.f-hide")) {
    clearGroupPipes(host);
    return;
  }
  const cards = [...host.querySelectorAll(":scope > .card")].filter(cardPipeVisible);
  const runs = [];
  for (let i = 0; i < cards.length; i++) {
    const parent = cards[i];
    if (parent.classList.contains("ev-child")) continue;
    const kids = [];
    for (let j = i + 1; j < cards.length && cards[j].classList.contains("ev-child"); j++) {
      kids.push(cards[j]);
    }
    if (kids.length) runs.push({ parent, kids });
  }
  if (!runs.length) {
    clearGroupPipes(host);
    return;
  }

  const hostRect = host.getBoundingClientRect();
  const geom = hierarchyGeom();
  const svg = ensurePipeSvg(host);
  const w = Math.max(1, host.scrollWidth || host.clientWidth);
  const h = Math.max(1, host.scrollHeight || host.clientHeight);
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));
  svg.replaceChildren();

  for (const { parent, kids } of runs) {
    const p = hierarchyAnchor(parent, hostRect, geom, "dot");
    const elbows = kids.map((k) => hierarchyAnchor(k, hostRect, geom, "elbow"));
    if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) continue;
    // Subpixel positions from getBoundingClientRect — matches the CSS event-dot.
    const railX = p.x;
    const y0 = p.y + geom.dot / 2 - 0.5;
    const y1 = elbows[elbows.length - 1].y;
    let d = `M${railX} ${y0}V${y1}`;
    for (const e of elbows) {
      if (!Number.isFinite(e.x) || !Number.isFinite(e.y)) continue;
      d += `M${railX} ${e.y}H${e.x}`;
    }
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("d", d);
    svg.appendChild(path);
  }
}

function drawAllHierarchyPipes() {
  if (!feed.classList.contains("vertical")) {
    feed.querySelectorAll(".ev-pipes").forEach((el) => el.remove());
    return;
  }
  // Avoid RO feedback while we mutate SVG geometry.
  hierarchyPipeRo?.disconnect();
  try {
    for (const host of feed.querySelectorAll(".group-events")) {
      try {
        drawGroupHierarchyPipes(host);
      } catch (err) {
        console.warn("hierarchy pipes:", err);
      }
    }
  } finally {
    if (hierarchyPipeRo) {
      hierarchyPipeRo.observe(feed);
      for (const host of feed.querySelectorAll(".group-events")) {
        hierarchyPipeRo.observe(host);
      }
    }
  }
}

function flushHierarchyPipes() {
  if (hierarchyPipeTimer) {
    clearTimeout(hierarchyPipeTimer);
    hierarchyPipeTimer = 0;
  }
  if (hierarchyPipeMaxTimer) {
    clearTimeout(hierarchyPipeMaxTimer);
    hierarchyPipeMaxTimer = 0;
  }
  if (hierarchyPipeDrawing) {
    scheduleHierarchyPipes();
    return;
  }
  hierarchyPipeDrawing = true;
  requestAnimationFrame(() => {
    try {
      drawAllHierarchyPipes();
    } finally {
      hierarchyPipeDrawing = false;
    }
  });
}

function scheduleHierarchyPipes() {
  // Trailing debounce — but history loading mutates the DOM continuously, so
  // also force a draw every 200ms or pipes never appear until the feed goes quiet.
  if (hierarchyPipeTimer) clearTimeout(hierarchyPipeTimer);
  hierarchyPipeTimer = setTimeout(flushHierarchyPipes, 64);
  if (!hierarchyPipeMaxTimer) {
    hierarchyPipeMaxTimer = setTimeout(flushHierarchyPipes, 200);
  }
}

function ensureHierarchyPipeObserver() {
  if (typeof ResizeObserver !== "undefined" && !hierarchyPipeRo) {
    hierarchyPipeRo = new ResizeObserver(() => scheduleHierarchyPipes());
    hierarchyPipeRo.observe(feed);
    for (const host of feed.querySelectorAll(".group-events")) {
      hierarchyPipeRo.observe(host);
    }
  }
  if (typeof MutationObserver !== "undefined" && !hierarchyPipeMo) {
    // childList only — class churn (enter anim / light-cone) would never settle.
    hierarchyPipeMo = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.type !== "childList") continue;
        const nodes = [...m.addedNodes, ...m.removedNodes];
        if (
          nodes.length
          && nodes.every((n) => n.nodeType === 1 && (
            n.classList?.contains("ev-pipes")
            || n.closest?.(".ev-pipes")
          ))
        ) {
          continue;
        }
        scheduleHierarchyPipes();
        return;
      }
    });
    hierarchyPipeMo.observe(feed, { childList: true, subtree: true });
  }
}

/* ── Enter animations (tip slide then fade / history fade) ────────────── */

/** `tip` | `hist` | null — controls markEnter on newly built cards. */
let enterMode = null;
const FEED_ENTER_MS = 700;
const reduceMotion = () =>
  typeof matchMedia === "function"
  && matchMedia("(prefers-reduced-motion: reduce)").matches;

function withEnterMode(mode, fn) {
  const prev = enterMode;
  enterMode = mode;
  try {
    return fn();
  } finally {
    enterMode = prev;
  }
}

function markEnter(el) {
  if (!el || !enterMode) return;
  if (enterMode === "tip") {
    // Stay invisible until the list finishes sliding, then fade in.
    el.classList.remove("enter-tip", "enter-hist");
    el.classList.add("enter-tip-pending");
    return;
  }
  el.classList.remove("enter-tip", "enter-hist", "enter-tip-pending");
  void el.offsetWidth;
  el.classList.add("enter-hist");
  const clear = () => el.classList.remove("enter-hist");
  el.addEventListener("animationend", clear, { once: true });
  setTimeout(clear, FEED_ENTER_MS + 100);
}

function captureGroupRects() {
  const first = new Map();
  for (const el of feed.querySelectorAll(":scope > .block-group")) {
    if (el.classList.contains("f-hide")) continue;
    first.set(el, el.getBoundingClientRect());
  }
  return first;
}

/** Invert existing groups to their pre-insert positions (must run before paint). */
function applyFlipInvert(first) {
  const movers = [];
  for (const el of feed.querySelectorAll(":scope > .block-group")) {
    const a = first.get(el);
    if (!a || el.classList.contains("f-hide")) continue;
    const b = el.getBoundingClientRect();
    const dx = a.left - b.left;
    const dy = a.top - b.top;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    el.classList.remove("flip-moving");
    el.style.transition = "none";
    el.style.transform = `translate(${dx}px, ${dy}px)`;
    movers.push(el);
  }
  return movers;
}

function clearFlip(movers) {
  for (const el of movers) {
    el.classList.remove("flip-moving");
    el.style.transition = "";
    el.style.transform = "";
  }
}

function fadeInTipCards(cards) {
  for (const card of cards) {
    if (!card.isConnected) continue;
    card.classList.remove("enter-tip-pending");
    if (reduceMotion()) continue;
    void card.offsetWidth;
    card.classList.add("enter-tip");
    const clear = () => card.classList.remove("enter-tip");
    card.addEventListener("animationend", clear, { once: true });
    setTimeout(clear, FEED_ENTER_MS + 100);
  }
}

/** Serialize tip slide+fade so a second batch doesn't capture mid-FLIP rects. */
let tipAnimBusy = false;
let tipAnimQueue = [];

let tipAnimBusyWatch = 0;
function releaseTipAnim() {
  if (tipAnimBusyWatch) {
    clearTimeout(tipAnimBusyWatch);
    tipAnimBusyWatch = 0;
  }
  tipAnimBusy = false;
  if (!tipAnimQueue.length) return;
  const next = tipAnimQueue.splice(0);
  insertTipEvents(next);
}

/** Tip cards that will actually show under the current filters/search. */
function tipCardIsVisible(card) {
  if (!card || !card.isConnected) return false;
  if (card.classList.contains("f-hide")) return false;
  if (card.closest(".block-group.f-hide")) return false;
  // Category chips use a stylesheet rule (display:none), not .f-hide.
  if (!settings.filters[card.dataset.category]) return false;
  if (card.dataset.category === "finance" && !financeAppEnabled(financeNameOf(card))) return false;
  if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return false;
  if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return false;
  const q = $("search").value.trim().toLowerCase();
  if (q && !(card.dataset.search || "").includes(q)) return false;
  return true;
}

/**
 * Tip insert: 1) mount new cards invisible, 2) slide existing groups down,
 * 3) fade new cards in. History uses fade-only via insertHistEvents.
 *
 * Off-filter / off-search tip events still mount (for retention) but must not
 * FLIP-slide or re-park scroll — that wobbles a sparse filtered list when
 * unrelated live traffic arrives.
 */
function insertTipEvents(events) {
  if (!events.length) return;

  if (tipAnimBusy) {
    tipAnimQueue.push(...events);
    return;
  }
  tipAnimBusy = true;
  // Failsafe: never leave the tip pipeline wedged (missed transitionend on mobile).
  if (tipAnimBusyWatch) clearTimeout(tipAnimBusyWatch);
  tipAnimBusyWatch = setTimeout(() => {
    if (tipAnimBusy) releaseTipAnim();
  }, FEED_ENTER_MS + 400);

  const stayAtTip = !isPaused();
  // Capture before insert — tipScrollY/stick checks use pre-insert geometry.
  const stickScroll = stayAtTip && shouldStickScrollToTip();

  if (reduceMotion()) {
    withEnterMode(null, () => {
      suppressFeedResort = true;
      try {
        for (const ev of events) routeEvent(ev);
      } finally {
        suppressFeedResort = false;
      }
      resortFeedBySlot();
    });
    // Hide off-filter cards before any scroll/layout stick.
    applyFilters();
    const showed = [...feed.querySelectorAll(".card.enter-tip-pending")].some(tipCardIsVisible);
    for (const card of feed.querySelectorAll(".card.enter-tip-pending")) {
      if (!tipCardIsVisible(card)) {
        card.classList.remove("enter-tip-pending", "enter-tip", "enter-hist");
      }
    }
    if (stickScroll && showed) scrollFeedToTip("auto");
    pruneTipDomGroups();
    releaseTipAnim();
    return;
  }

  const first = captureGroupRects();

  withEnterMode("tip", () => {
    suppressFeedResort = true;
    try {
      for (const ev of events) routeEvent(ev);
    } finally {
      suppressFeedResort = false;
    }
    resortFeedBySlot();
  });

  // Apply filters synchronously so off-filter tip groups collapse before FLIP
  // measures — otherwise visible groups slide down, then jump back up.
  applyFilters();

  const pendingCards = [...feed.querySelectorAll(".card.enter-tip-pending")];
  const visiblePending = pendingCards.filter(tipCardIsVisible);
  for (const card of pendingCards) {
    if (!tipCardIsVisible(card)) {
      card.classList.remove("enter-tip-pending", "enter-tip", "enter-hist");
    }
  }

  if (!visiblePending.length) {
    // Nothing new on the active filter — keep layout/scroll still.
    pruneTipDomGroups();
    scheduleHierarchyPipes();
    releaseTipAnim();
    return;
  }

  if (stickScroll) scrollFeedToTip("auto");
  pruneTipDomGroups();

  // Invert before the browser paints the jumped layout.
  const movers = applyFlipInvert(first);
  // Flush so the inverted frame is committed (still with transition:none).
  void feed.offsetWidth;

  const afterSlide = () => {
    fadeInTipCards(visiblePending);
    scheduleHierarchyPipes();
    // Free the lock after fade so the next batch measures settled layout.
    setTimeout(releaseTipAnim, FEED_ENTER_MS + 50);
  };

  if (!movers.length) {
    afterSlide();
    return;
  }

  requestAnimationFrame(() => {
    for (const el of movers) {
      // Must clear inline transition:none or .flip-moving never animates.
      el.style.transition = "";
      el.classList.add("flip-moving");
    }
    // Ensure transition is active before releasing the invert.
    void feed.offsetWidth;
    for (const el of movers) {
      el.style.transform = "";
    }
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      clearFlip(movers);
      afterSlide();
    };
    let left = movers.length;
    for (const el of movers) {
      el.addEventListener("transitionend", function te(e) {
        if (e.target !== el) return;
        if (e.propertyName && e.propertyName !== "transform") return;
        el.removeEventListener("transitionend", te);
        if (--left <= 0) finish();
      });
    }
    setTimeout(finish, FEED_ENTER_MS + 50);
  });
}

/** Insert history events with fade-in only (no list jump animation). */
function insertHistEvents(run) {
  withEnterMode("hist", () => {
    suppressFeedResort = true;
    try {
      run();
    } finally {
      suppressFeedResort = false;
    }
    resortFeedBySlot();
  });
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
  const byId = new Map();
  for (const { ev } of retentionCache.values()) {
    if (!eventPassesFeedFilters(ev)) continue;
    byId.set(ev.id, ev);
    // Keep the parent Transaction with its children (bypass min-ADA only).
    const parent = parentTransactionEvent(ev);
    if (
      parent
      && !byId.has(parent.id)
      && eventPassesFeedFilters(parent, { ignoreMinAda: true })
    ) {
      byId.set(parent.id, parent);
    }
  }
  feedHitIds = new Set(byId.keys());
  const list = sortEventsNewestFirst([...byId.values()]);
  feedHitBuffer = list;
  reindexFeedHits();
  return list;
}

/**
 * Fold newly-cached events into the existing hit list.
 *
 * The only work proportional to cache size is one Set lookup per entry;
 * filtering and sorting touch just the delta. Hydration pages backwards, so new
 * hits are older than the current tail and concatenate directly; a merged
 * re-sort covers any other ordering.
 */
function appendNewFeedHits() {
  const byId = new Map();
  const consider = (ev, opts) => {
    if (!ev || feedHitIds.has(ev.id) || byId.has(ev.id)) return;
    if (!eventPassesFeedFilters(ev, opts)) return;
    byId.set(ev.id, ev);
  };
  for (const { ev } of retentionCache.values()) {
    if (feedHitIds.has(ev.id)) continue;
    consider(ev);
    if (byId.has(ev.id)) consider(parentTransactionEvent(ev), { ignoreMinAda: true });
  }
  if (!byId.size) return;
  const added = sortEventsNewestFirst([...byId.values()]);
  for (const id of byId.keys()) feedHitIds.add(id);
  const tail = feedHitBuffer[feedHitBuffer.length - 1];
  if (!tail || cmpSlotIdDesc(tail.slot || 0, tail.id, added[0].slot || 0, added[0].id) <= 0) {
    const base = feedHitBuffer.length;
    feedHitBuffer = feedHitBuffer.concat(added);
    for (let i = 0; i < added.length; i++) {
      const id = added[i]?.id;
      if (id != null) feedHitIndexById.set(id, base + i);
    }
  } else {
    feedHitBuffer = sortEventsNewestFirst(feedHitBuffer.concat(added));
    reindexFeedHits();
  }
}

/**
 * Align the paging cursors with what is mounted.
 *
 * The feed shows a window over feedHitBuffer: `feedHitStart` is the first
 * mounted hit and `feedHitOffset` is one past the last. Both are derived from
 * the mounted cards, so unmounting either end moves the corresponding edge and
 * paging continues outward from there in both directions.
 */
/**
 * Mount the page of newer events immediately above the window, mirroring the
 * downward page. Events route into their block groups and resortFeedBySlot
 * places them by slot, so they land above whatever is on screen.
 */
function renderNewerPage() {
  if (feedHitStart <= 0) return 0;
  const batch = [];
  const batchIds = new Set();
  while (batch.length < FEED_PAGE_SIZE && feedHitStart > 0) {
    const ev = feedHitBuffer[--feedHitStart];
    if (ev?.id == null || seenEventIds.has(ev.id) || batchIds.has(ev.id)) continue;
    if (ev.kind !== "block" && ev.block_hash && settings.filters.block) {
      const blockEv = findBlockEvent(ev.block_hash);
      if (blockEv && !seenEventIds.has(blockEv.id) && !batchIds.has(blockEv.id)) {
        batch.push(blockEv);
        batchIds.add(blockEv.id);
      }
    }
    if (ev.kind !== "block" && ev.kind !== "transaction" && settings.filters.transaction) {
      const parentTx = parentTransactionEvent(ev);
      if (parentTx && !seenEventIds.has(parentTx.id) && !batchIds.has(parentTx.id)) {
        batch.push(parentTx);
        batchIds.add(parentTx.id);
      }
    }
    batch.push(ev);
    batchIds.add(ev.id);
  }
  if (!batch.length) return 0;
  suppressFeedResort = true;
  try {
    for (const ev of batch) routeEvent(ev);
  } finally {
    suppressFeedResort = false;
  }
  resortFeedBySlot();
  return batch.length;
}

/** Refill the window upward when the reader approaches the newest mounted event. */
function maybeLoadNewer() {
  if ($("search").value.trim()) return;
  if (historyLoading || feedRebuilding) return;
  if (feedHitStart <= 0) return;
  const first = groupOrder[0];
  if (!first) return;
  // Only once the top of the mounted window is within reach of the viewport.
  if (first.getBoundingClientRect().top < -1200) return;
  withAnchoredScroll(() => {
    renderNewerPage();
    applyFilters();
  });
  pruneTipDomGroups();
  updateLoadedEventCount();
}

function syncFeedHitOffset() {
  if (!feedHitBuffer.length) {
    feedHitStart = 0;
    feedHitOffset = 0;
    retentionHistoryDone = false;
    return;
  }
  let lo = Infinity;
  let hi = -1;
  for (const id of seenEventIds) {
    const i = feedHitIndexById.get(id);
    if (i == null) continue;
    if (i < lo) lo = i;
    if (i > hi) hi = i;
  }
  if (hi < 0) {
    feedHitStart = Math.min(feedHitStart, feedHitBuffer.length);
    feedHitOffset = Math.max(feedHitStart, Math.min(feedHitOffset, feedHitBuffer.length));
  } else {
    feedHitStart = lo;
    feedHitOffset = hi + 1;
  }
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
  if (!force && !feedHitsDirty && key === feedHitKey && feedHitBuffer.length) {
    syncFeedHitOffset();
    return;
  }
  // Same filters, list already built: fold in what hydration added. A full
  // rebuild is reserved for a filter change or an explicit force.
  if (!force && feedHitsDirty && key === feedHitKey && feedHitBuffer.length) {
    feedHitsDirty = false;
    appendNewFeedHits();
    syncFeedHitOffset();
    return;
  }
  feedHitsDirty = false;
  // Preserve paint cursor across rebuilds when filters are unchanged (force refresh
  // after hydrate). Filter changes start from the tip again.
  const prevOffset = key === feedHitKey ? feedHitOffset : 0;
  feedHitKey = key;
  feedHitBuffer = collectFeedHits();
  feedHitOffset = Math.min(prevOffset, feedHitBuffer.length);
  syncFeedHitOffset();
}

/**
 * Render the next FEED_PAGE_SIZE *new* matches from feedHitBuffer into the DOM.
 * Returns how many cards were added.
 * Pass `{ skipApplyFilters: true }` when draining multiple pages; call applyFilters once after.
 */
function renderFeedPage(opts = {}) {
  const animate = opts.animate !== false;
  const batch = [];
  const batchIds = new Set();
  while (batch.length < FEED_PAGE_SIZE && feedHitOffset < feedHitBuffer.length) {
    const ev = feedHitBuffer[feedHitOffset++];
    if (ev?.id == null || seenEventIds.has(ev.id) || batchIds.has(ev.id)) continue;
    // Pull the parent Block into this page when we hit its children first —
    // only when Blocks are in the active filter set (otherwise it's hidden work).
    if (ev.kind !== "block" && ev.block_hash && settings.filters.block) {
      const blockEv = findBlockEvent(ev.block_hash);
      if (
        blockEv
        && !seenEventIds.has(blockEv.id)
        && !batchIds.has(blockEv.id)
      ) {
        batch.push(blockEv);
        batchIds.add(blockEv.id);
      }
    }
    // Same for the parent Transaction — children must not mount as orphans
    // when Transactions are visible.
    if (
      ev.kind !== "block"
      && ev.kind !== "transaction"
      && settings.filters.transaction
    ) {
      const parentTx = parentTransactionEvent(ev);
      if (
        parentTx
        && !seenEventIds.has(parentTx.id)
        && !batchIds.has(parentTx.id)
      ) {
        batch.push(parentTx);
        batchIds.add(parentTx.id);
      }
    }
    batch.push(ev);
    batchIds.add(ev.id);
  }
  retentionHistoryDone = feedHitOffset >= feedHitBuffer.length;
  if (!batch.length) return 0;
  const paint = () => {
    for (const ev of batch) routeEvent(ev);
  };
  if (animate) {
    insertHistEvents(paint);
  } else {
    suppressFeedResort = true;
    try {
      paint();
    } finally {
      suppressFeedResort = false;
    }
    resortFeedBySlot();
  }
  prefetchUnitsFromEvents(batch);
  if (!opts.skipApplyFilters) applyFilters();
  return batch.length;
}

/** Drop painted cards but keep the retention cache (filter / order rebuild). */
function clearFeedDom() {
  clearLightCone();
  feed.querySelectorAll(".block-group").forEach((g) => {
    groupViewIo?.unobserve(g);
    g.remove();
  });
  groups.clear();
  groupOrder.length = 0;
  seenEventIds.clear();
  oldestEventId = null;
  feedHitOffset = 0;
  retentionHistoryDone = false;
  pinHistoryLoader();
}

/**
 * Paint pages from feedHitBuffer. Yields periodically so large match sets stay
 * responsive. Pass `{ all: true }` to drain the whole 24h hit list.
 * Pass `{ gen }` to abort if a newer rebuild has started.
 * Pass `{ sync: true }` to never yield (used for small sparse filters).
 */
async function paintFeedHitPages(opts = {}) {
  // Paint exactly `pages` pages of FEED_PAGE_SIZE matches each.
  const maxPages = Math.max(1, opts.pages || 1);
  const gen = opts.gen;
  const sync = !!opts.sync;
  let pages = 0;
  // Bound the walk over already-seen gaps so a long run can't spin.
  let guard = 0;
  while (feedHitOffset < feedHitBuffer.length && pages < maxPages) {
    if (gen != null && gen !== feedRebuildGen) return pages;
    if (++guard > maxPages * 20) break;
    const before = feedHitOffset;
    const n = renderFeedPage({ animate: false, skipApplyFilters: true });
    // All-seen gaps: renderFeedPage advances offset and returns 0 — keep going.
    if (feedHitOffset === before) break;
    if (n) pages++;
    if (!sync && pages > 0 && pages % PAINT_YIELD_EVERY === 0) {
      await yieldToBrowser();
    }
  }
  return pages;
}

/**
 * After a filter change: rebuild the visible feed from the current retention
 * cache (partial mid-hydrate is fine). Sparse filters paint every match;
 * dense views fill the viewport; the rest stays in RAM for instant scroll.
 */
async function onFeedFiltersChanged() {
  if ($("search").value.trim()) return;
  const gen = ++feedRebuildGen;
  historyLoadArmed = true;
  feedRebuilding = true;
  try {
    ensureFeedHitBuffer(true);
    if (gen !== feedRebuildGen) return;

    // No matches in what we've cached yet — don't wipe the tip to blank.
    // Hydrate will absorb hits as chunks arrive; disk only after retentionReady.
    if (!feedHitBuffer.length) {
      applyFilters();
      if (retentionReady) queueMicrotask(() => maybeLoadHistory());
      return;
    }

    clearFeedDom();
    feedHitOffset = 0;
    retentionHistoryDone = false;

    await paintFeedHitPages({ pages: 1, gen, sync: true });
    if (gen !== feedRebuildGen) return;

    resortFeedBySlot();
    applyFilters();
    pruneTipDomGroups();
    // Leave short views for the user to wheel/scroll — no auto network spinner.
  } finally {
    if (gen === feedRebuildGen) feedRebuilding = false;
  }
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
  // indent it under its parent. Skip indent when Transactions are filtered off
  // (parent card isn't mounted).
  if (
    ev.parent_id != null
    && ev.kind !== "block"
    && ev.kind !== "transaction"
    && settings.filters.transaction
  ) {
    card.classList.add("ev-child");
  }
  if (ev.slot != null) card.dataset.slot = String(ev.slot);
  if (ev.tx_hash) card.dataset.tx = ev.tx_hash;
  if (ev.data && ev.data.ada != null) card.dataset.ada = ev.data.ada;
  // `data-dex` also drives order→settlement pill matching, so keep it whenever
  // the event names a venue, not just for the filter.
  if (ev.data?.dex) card.dataset.dex = String(ev.data.dex);
  if (ev.data?.dapp) card.dataset.dapp = String(ev.data.dapp);
  if (ev.category === "finance") {
    card.dataset.finance = String(ev.data?.dex || ev.data?.dapp || "");
  }
  if (ev.category === "governance") {
    const gt = govTypeKey(ev);
    if (gt) card.dataset.govType = gt;
  }

  const title = ev.kind === "block"
    ? `Block <span class="height">${fmtInt(ev.height)}</span>`
    : esc(formatEventTitle(ev));

  let iconHtml = iconFor(ev.kind, ev.category, ev.data?.side, ev.data?.vote);
  let iconClass = "ev-icon";
  let iconStyle = "";
  const scam = ev.kind === "token_transfer" && !!ev.data?.scam;
  const branded =
    (ev.data?.dapp && dappIconHtml && dappIconHtml(ev.data.dapp))
    || (ev.data?.dex && dexIconHtml && dexIconHtml(ev.data.dex))
    || null;
  if (branded) {
    iconHtml = branded.html;
    iconClass = "ev-icon has-ev-logo" + (branded.badge ? " has-ev-badge" : "");
    if (branded.plate) {
      iconStyle = ` style="--ev-plate:${esc(branded.plate)}"`;
    }
  }
  if (scam) iconClass += " has-scam";

  card.innerHTML = `
    <div class="${iconClass}"${iconStyle}>${iconHtml}${
      scam ? `<span class="scam-flag" title="Known scam token" aria-label="scam">🚩</span>` : ""
    }</div>
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
  enrichHandles(card);
  enrichGovActions(card);
  ensureTxStakesObserver();
  const stakesEl = card.querySelector(".tx-stakes");
  if (stakesEl) {
    txStakesRo?.observe(stakesEl);
    // Two rAFs: first layout assigns width, second measures pills.
    requestAnimationFrame(() => requestAnimationFrame(() => fitTxStakes(stakesEl)));
  }
  markEnter(card);
  return card;
}

/* ── Feed assembly: block groups on the chain spine ───────────────────── */

/** Mark a block-group as viewed now (mount, intersection, or tip activity). */
function touchGroupViewed(g, when = Date.now()) {
  if (!g) return;
  g.dataset.lastViewed = String(when);
}

function groupNearViewport(g) {
  if (!g || !g.isConnected) return false;
  const r = g.getBoundingClientRect();
  const margin = 240;
  return r.bottom > -margin && r.top < window.innerHeight + margin;
}

/** IntersectionObserver — refresh lastViewed while a group is on screen. */
let groupViewIo = null;
function ensureGroupViewObserver() {
  if (groupViewIo || typeof IntersectionObserver === "undefined") return;
  groupViewIo = new IntersectionObserver(
    (entries) => {
      const now = Date.now();
      for (const e of entries) {
        if (e.isIntersecting) touchGroupViewed(e.target, now);
      }
    },
    { root: null, rootMargin: "200px 0px", threshold: 0 },
  );
}

function observeGroupView(g) {
  ensureGroupViewObserver();
  groupViewIo?.observe(g);
}

/** Remove one mounted block-group and free its event ids for remount. */
function dropBlockGroup(g) {
  if (!g) return;
  groupViewIo?.unobserve(g);
  const idx = groupOrder.indexOf(g);
  if (idx >= 0) groupOrder.splice(idx, 1);
  g.querySelectorAll(".card[data-eid]").forEach((card) => {
    const id = Number(card.dataset.eid);
    if (Number.isFinite(id)) seenEventIds.delete(id);
  });
  if (g.dataset.block) groups.delete(g.dataset.block);
  g.remove();
}

function refreshOldestEventId() {
  oldestEventId = null;
  for (const g of groupOrder) {
    g.querySelectorAll(".card[data-eid]").forEach((card) => {
      const id = Number(card.dataset.eid);
      if (!Number.isFinite(id)) return;
      if (oldestEventId == null || id < oldestEventId) oldestEventId = id;
    });
  }
}

/**
 * Bound the mounted feed:
 *  - Always keep the newest TIP_DOM_MIN_GROUPS.
 *  - Drop any older group that has not been viewed in VIEW_STALE_MS and is not
 *    near the viewport (history the user is still looking at stays).
 *  - Hard-cap at TIP_DOM_GROUPS by shedding groups opposite the viewport —
 *    never the oldest end while the user is loading/reading history (that used
 *    to discard each newly loaded page immediately).
 * Events stay in retentionCache and remount on history scroll.
 */
function pruneTipDomGroups() {
  if (feedRebuilding) return;
  if (!groupOrder.length) return;

  const now = Date.now();
  const vh = window.innerHeight || 0;

  // One layout read per group, reused for every decision below.
  const rows = groupOrder.map((g) => {
    const r = g.getBoundingClientRect();
    return {
      g,
      r,
      cards: g.querySelectorAll(".card").length,
      near: r.bottom > -DOM_WINDOW_MARGIN_PX && r.top < vh + DOM_WINDOW_MARGIN_PX,
    };
  });

  let mounted = 0;
  for (const row of rows) {
    mounted += row.cards;
    if (row.near) touchGroupViewed(row.g, now);
  }

  // How far a group sits outside the viewport; the furthest go first.
  const distance = (r) => (r.bottom < 0 ? -r.bottom : (r.top > vh ? r.top - vh : 0));

  // The newest groups are held only while the reader is at the tip, so the feed
  // stays populated there. Reading history, they are evicted like any other
  // distant group, which keeps the mounted range contiguous around the viewport
  // and lets it refill from either edge.
  const atTip = (window.scrollY || 0) < vh;
  const evictable = rows
    .map((row, i) => ({ ...row, i }))
    .filter((row) => !row.near && (!atTip || row.i >= TIP_DOM_MIN_GROUPS))
    .sort((a, b) => distance(b.r) - distance(a.r));

  let dropped = false;
  const vhNow = vh;
  let anchor = null;
  for (const g of groupOrder) {
    const r = g.getBoundingClientRect();
    if (r.bottom > 0 && r.top < vhNow) { anchor = g; break; }
  }
  const anchorTop = anchor ? anchor.getBoundingClientRect().top : null;
  for (const row of evictable) {
    const overBudget = mounted > DOM_WINDOW_CARDS || groupOrder.length > TIP_DOM_GROUPS;
    const idle = now - Number(row.g.dataset.lastViewed || 0) > VIEW_STALE_MS;
    if (!overBudget && !idle) continue;
    mounted -= row.cards;
    dropBlockGroup(row.g);
    dropped = true;
  }

  if (!dropped) return;
  if (anchor && anchor.isConnected && anchorTop != null && settings.layout === "vertical") {
    const delta = anchor.getBoundingClientRect().top - anchorTop;
    if (Math.abs(delta) > 0.5) {
      window.scrollBy(0, delta);
      lastScrollPos = feedScrollPos();
    }
  }
  clearLightCone();
  refreshOldestEventId();
  if (typeof syncFeedHitOffset === "function") syncFeedHitOffset();
  pinHistoryLoader();
  updateLoadedEventCount();
  scheduleHierarchyPipes();
}

/** Periodic prune so an idle tip tab can't accumulate hours of DOM. */
let pruneDomTimer = 0;
function scheduleDomPruneLoop() {
  if (pruneDomTimer) return;
  pruneDomTimer = setInterval(() => {
    if (document.hidden) return;
    pruneTipDomGroups();
  }, 30_000);
}

/** Best-known chain height for a block-group (for spine gap detection). */
function groupChainHeight(g) {
  if (!g) return 0;
  let best = Number(g.dataset.height || 0);
  if (best <= 0) {
    const hash = g.dataset.block;
    if (hash) {
      const be = blocksByHash.get(hash);
      best = Number(be?.height || 0);
    }
  }
  if (best <= 0) {
    for (const card of g.querySelectorAll(".card[data-eid]")) {
      const row = retentionCache.get(Number(card.dataset.eid));
      const h = Number(row?.ev?.height || 0);
      if (h > best) best = h;
    }
  }
  if (best > 0) g.dataset.height = String(best);
  return best > 0 ? best : 0;
}

/** True when two visible groups are distinct blocks (ellipsis is inter-block only). */
function spineDifferentBlocks(a, b) {
  if (!a || !b || a === b) return false;
  const ha = a.dataset.block || "";
  const hb = b.dataset.block || "";
  if (ha && hb) return ha !== hb;
  const h1 = groupChainHeight(a);
  const h2 = groupChainHeight(b);
  if (h1 > 0 && h2 > 0) return h1 !== h2;
  // Standalone / unknown-hash groups: different DOM groups ⇒ treat as different.
  return true;
}

/**
 * Heights/slots of retained events hidden by the current filters.
 * Block headers are excluded — turning Blocks off shouldn't dotted-line every
 * empty inter-block stretch.
 */
function collectFilteredSpineMarks() {
  const heights = [];
  const slots = [];
  for (const { ev } of retentionCache.values()) {
    if (!ev || ev.kind === "block") continue;
    if (eventPassesFeedFilters(ev)) continue;
    const h = Number(ev.height || 0);
    if (h > 0) heights.push(h);
    const s = Number(ev.slot || 0);
    if (s > 0) slots.push(s);
  }
  heights.sort((a, b) => a - b);
  slots.sort((a, b) => a - b);
  return { heights, slots };
}

/** True if a sorted numeric list has any value in (lo, hi) exclusive. */
function sortedHasBetween(arr, lo, hi) {
  if (!arr.length || !(hi > lo + 1)) return false;
  let loI = 0;
  let hiI = arr.length;
  while (loI < hiI) {
    const mid = (loI + hiI) >> 1;
    if (arr[mid] <= lo) loI = mid + 1;
    else hiI = mid;
  }
  return loI < arr.length && arr[loI] < hi;
}

/**
 * Filtered non-block content lies on an intermediate block between two groups.
 * Same-block filtered siblings never qualify (exclusive height/slot range).
 */
function spineHasFilteredBetween(gNew, gOld, marks) {
  if (!spineDifferentBlocks(gNew, gOld)) return false;
  const hNew = groupChainHeight(gNew);
  const hOld = groupChainHeight(gOld);
  if (hNew > 0 && hOld > 0) {
    const lo = Math.min(hNew, hOld);
    const hi = Math.max(hNew, hOld);
    if (sortedHasBetween(marks.heights, lo, hi)) return true;
  }
  const sNew = Number(gNew.dataset.slot || 0);
  const sOld = Number(gOld.dataset.slot || 0);
  if (sNew > 0 && sOld > 0) {
    const lo = Math.min(sNew, sOld);
    const hi = Math.max(sNew, sOld);
    if (sortedHasBetween(marks.slots, lo, hi)) return true;
  }
  return false;
}

function noteGroupHeight(g, ev) {
  if (!g || ev?.height == null) return;
  const h = Number(ev.height);
  if (!Number.isFinite(h) || h <= 0) return;
  const prev = Number(g.dataset.height || 0);
  if (h >= prev) g.dataset.height = String(h);
}

function newGroup(blockHash, ev) {
  const g = document.createElement("div");
  g.className = "block-group";
  if (blockHash) g.dataset.block = blockHash;
  g.dataset.slot = String(ev?.slot || 0);
  g.dataset.eid = String(ev?.id || 0);
  noteGroupHeight(g, ev);
  // Resolve height from the block index when the first painted event omits it.
  if (blockHash && !g.dataset.height) groupChainHeight(g);
  touchGroupViewed(g);
  const evs = document.createElement("div");
  evs.className = "group-events";
  g.appendChild(evs);
  hierarchyPipeRo?.observe(evs);
  observeGroupView(g);
  // Temporary placement; scheduleFeedResort() establishes final slot order.
  feed.prepend(g);
  groupOrder.unshift(g);
  if (blockHash) groups.set(blockHash, g);
  while (groupOrder.length > MAX_GROUPS) {
    dropBlockGroup(groupOrder[groupOrder.length - 1]);
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

function findBlockEvent(blockHash) {
  if (!blockHash) return null;
  const hit = blocksByHash.get(blockHash);
  if (hit) return hit;
  for (const { ev } of retentionCache.values()) {
    if (ev.kind === "block" && ev.block_hash === blockHash) {
      blocksByHash.set(blockHash, ev);
      return ev;
    }
  }
  for (const ev of feedHitBuffer) {
    if (ev.kind === "block" && ev.block_hash === blockHash) return ev;
  }
  return null;
}

/**
 * Make sure a block-group has its Block card. Paged feeds can otherwise mount
 * txs/tokens first and leave the header missing until a later page.
 * Structural cards skip tip/hist enter animation so they don't leave an
 * invisible card-sized hole above their children.
 * When Blocks are filtered off, only ensure the group exists — mounting a
 * hidden header for every sparse match freezes the tab.
 */
function ensureBlockCard(blockHash) {
  if (!blockHash) return;
  let g = groups.get(blockHash);
  if (!settings.filters.block) {
    if (g) return;
    const blockEv = findBlockEvent(blockHash);
    if (blockEv) newGroup(blockHash, blockEv);
    else newGroup(blockHash, { slot: 0, id: 0, block_hash: blockHash });
    return;
  }
  if (g?.querySelector(".card-block")) return;
  const blockEv = findBlockEvent(blockHash);
  if (!blockEv) return;
  if (!g) g = newGroup(blockHash, blockEv);
  if (g.querySelector(".card-block")) return;
  // Card missing even if we already counted the id (DOM rebuild / page skew).
  if (!seenEventIds.has(blockEv.id)) {
    sessionEvents++;
    noteEventId(blockEv);
  }
  withEnterMode(null, () => {
    g.prepend(buildCard(blockEv));
  });
}

/**
 * Mount the parent Transaction before a child so hierarchy sort can cluster
 * DEX / token / metadata under it (and not above the next unrelated tx).
 * Skip when Transactions are filtered off — children still group under the block.
 */
function ensureParentTransaction(ev) {
  if (!settings.filters.transaction) {
    if (ev.block_hash) ensureBlockCard(ev.block_hash);
    return;
  }
  const parent = parentTransactionEvent(ev);
  if (!parent) return;
  const blockHash = parent.block_hash || ev.block_hash;
  if (blockHash) ensureBlockCard(blockHash);
  let g = blockHash ? groups.get(blockHash) : null;
  if (!g) g = newGroup(blockHash, parent);
  if (g.querySelector(`.card[data-eid="${parent.id}"]`)) return;
  if (!seenEventIds.has(parent.id)) {
    sessionEvents++;
    noteEventId(parent);
  }
  withEnterMode(null, () => {
    g.querySelector(".group-events").appendChild(buildCard(parent));
    markGroupDirty(g);
  });
}

/**
 * Groups that somehow lost their Block header (or have a stuck opacity-0 tip
 * pending on it) leave a dead gap on the spine. Repair before each resort.
 */
function repairBlockGroup(g) {
  if (!g) return;
  // Repair cost scales with the group's cards, so groups untouched since their
  // last repair are skipped. Pending enter animations still need a look.
  if (g.dataset.repairClean === "1" && !g.querySelector(".enter-tip-pending")) return;
  const hash = g.dataset.block;
  if (hash && !g.querySelector(".card-block")) {
    ensureBlockCard(hash);
  }
  const host = g.querySelector(".group-events");
  if (!host) return;

  // One scan builds every index this function needs.
  const cards = [...host.querySelectorAll(":scope > .card")];
  const mountedEids = new Set();
  const childrenByParent = new Map();
  let hasVisibleEvent = false;
  for (const card of cards) {
    if (card.dataset.eid) mountedEids.add(card.dataset.eid);
    const pid = card.dataset.parent;
    if (pid) {
      let list = childrenByParent.get(pid);
      if (!list) childrenByParent.set(pid, (list = []));
      list.push(card);
    }
    if (
      !hasVisibleEvent
      && !card.classList.contains("enter-tip-pending")
      && !card.classList.contains("f-hide")
    ) {
      hasVisibleEvent = true;
    }
  }

  // Children whose Transaction card never mounted (or was lost) sort above every
  // other tx and look like block-level orphans — remount the parent.
  if (settings.filters.transaction) {
    const missingParents = new Set();
    for (const card of cards) {
      if (card.dataset.kind === "block" || card.dataset.kind === "transaction") continue;
      const pid = card.dataset.parent;
      if (!pid) continue;
      if (!mountedEids.has(pid)) missingParents.add(pid);
    }
    for (const pid of missingParents) {
      const row = retentionCache.get(Number(pid));
      const parent = row?.ev?.kind === "transaction" ? row.ev : null;
      if (!parent) continue;
      if (!seenEventIds.has(parent.id)) {
        sessionEvents++;
        noteEventId(parent);
      }
      withEnterMode(null, () => {
        host.appendChild(buildCard(parent));
      });
    }
  }
  // Header still invisible while children already show → card-sized hole.
  if (hasVisibleEvent) {
    g.querySelectorAll(".card-block.enter-tip-pending").forEach((card) => {
      card.classList.remove("enter-tip-pending", "enter-tip", "enter-hist");
    });
  }
  for (const tx of cards) {
    if (tx.dataset.kind !== "transaction") continue;
    if (!tx.classList.contains("enter-tip-pending")) continue;
    const eid = tx.dataset.eid;
    if (!eid) continue;
    const kidVisible = (childrenByParent.get(eid) || []).some(
      (c) => !c.classList.contains("enter-tip-pending") && !c.classList.contains("f-hide"),
    );
    if (kidVisible) tx.classList.remove("enter-tip-pending", "enter-tip", "enter-hist");
  }
  g.dataset.repairClean = "1";
}

/** Mark a group as needing repair after its card set changes. */
function markGroupDirty(g) {
  if (g && g.dataset) delete g.dataset.repairClean;
}

function routeEvent(ev) {
  if (!keepDexEvent(ev) || isNoopRedelegation(ev)) return;
  if (ev?.id != null && seenEventIds.has(ev.id)) {
    // Id was recorded but the card never landed (group eviction / ensure raced).
    // Remount so children aren't orphaned under a spine gap.
    if (ev.kind === "block" && ev.block_hash) {
      const g = groups.get(ev.block_hash);
      if (g && !g.querySelector(".card-block")) {
        withEnterMode(null, () => g.prepend(buildCard(ev)));
        scheduleFeedResort();
      }
    } else if (ev.kind === "transaction" && ev.block_hash && settings.filters.transaction) {
      const g = groups.get(ev.block_hash);
      const host = g?.querySelector(".group-events");
      if (host && !host.querySelector(`.card[data-eid="${ev.id}"]`)) {
        withEnterMode(null, () => host.appendChild(buildCard(ev)));
        markGroupDirty(host.parentElement);
        scheduleFeedResort();
      }
    }
    return;
  }
  sessionEvents++;
  noteEventId(ev);

  if (ev.kind === "block") {
    let g = ev.block_hash ? groups.get(ev.block_hash) : null;
    if (!g) g = newGroup(ev.block_hash, ev);
    else touchGroupViewed(g);
    noteGroupHeight(g, ev);
    // Don't mount a Block header when Blocks are filtered off.
    if (settings.filters.block && !g.querySelector(".card-block")) {
      withEnterMode(null, () => g.prepend(buildCard(ev)));
    }
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

  if (ev.block_hash) ensureBlockCard(ev.block_hash);
  if (ev.kind !== "transaction") ensureParentTransaction(ev);
  let g = ev.block_hash ? groups.get(ev.block_hash) : null;
  if (!g) g = newGroup(ev.block_hash, ev);
  else touchGroupViewed(g);
  noteGroupHeight(g, ev);
  g.querySelector(".group-events").appendChild(buildCard(ev));
  markGroupDirty(g);
  scheduleFeedResort();
}

function standaloneCard(ev) {
  const g = newGroup(null, ev);
  g.querySelector(".group-events").appendChild(buildCard(ev));
  markGroupDirty(g);
  scheduleFeedResort();
}

/**
 * Insert a page of events. Final order always comes from resortFeedBySlot().
 */
function routeHistoricalBatch(events) {
  insertHistEvents(() => {
    const anchors = new Map();
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
      else touchGroupViewed(g);
      noteGroupHeight(g, ev);
      if (ev.block_hash) ensureBlockCard(ev.block_hash);
      if (ev.kind !== "block" && ev.kind !== "transaction") {
        ensureParentTransaction(ev);
      }

      if (ev.kind === "block") {
        if (settings.filters.block && !g.querySelector(".card-block")) {
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
  });
}

/* min-ADA / search / category filters must also apply to fresh cards */
const applySoon = (() => {
  let t;
  return () => { clearTimeout(t); t = setTimeout(applyFilters, 120); };
})();

/* ── Pause-on-read buffering ──────────────────────────────────────────── */

/**
 * "At the tip" means the top of the event list is still on screen (under the
 * sticky header / below the filters) — not that document scrollY is ~0.
 * Scrolling the filters off-screen is fine; pausing only starts once the
 * first feed cards have gone under the header.
 */
function tipAnchorY() {
  const header = document.querySelector("header");
  return header ? header.getBoundingClientRect().bottom : 0;
}

/**
 * Scroll Y that parks the feed tip just under the sticky header.
 * Used after tip inserts so scroll-anchoring can't shove us into "paused".
 */
function tipScrollY() {
  return Math.max(0, window.scrollY + feed.getBoundingClientRect().top - tipAnchorY());
}

function scrollFeedToTip(behavior = "auto") {
  window.scrollTo({ top: tipScrollY(), behavior });
}

/**
 * Only re-park window scroll when the feed tip is already docked under the
 * header. At document top (filters / chrome still above the list) live inserts
 * must not call scrollFeedToTip — that yanks the viewport down to the events.
 */
function shouldStickScrollToTip() {
  if (settings.layout !== "vertical") return false;
  const feedTop = feed.getBoundingClientRect().top;
  return feedTop <= tipAnchorY() + 24;
}

function isPaused() {
  if (settings.layout === "vertical") {
    // Require real downward scroll. Tip inserts + scroll anchoring can move
    // feedTop under the header while scrollY is still ~0, which used to freeze
    // live updates into `pending` forever on mobile.
    if (window.scrollY < 48) return false;
    const feedTop = feed.getBoundingClientRect().top;
    // Small slack so rubber-band / subpixel scroll doesn't flicker the pill.
    return feedTop < tipAnchorY() - 12;
  }
  return feed.scrollLeft > 60;
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
    tipBatch.push(ev);
    scheduleTipFlush();
  }
}

/** Coalesce live tip events into one FLIP+fade frame (avoids per-event flash). */
let tipBatch = [];
let tipFlushQueued = false;
function scheduleTipFlush() {
  if (tipFlushQueued) return;
  tipFlushQueued = true;
  requestAnimationFrame(() => {
    tipFlushQueued = false;
    if (isPaused()) {
      // Scrolled away mid-frame — park anything still queued.
      while (tipBatch.length) {
        pending.push(tipBatch.shift());
        if (pending.length > 800) pending.shift();
      }
      updateNewPill();
      return;
    }
    const batch = tipBatch.splice(0);
    if (!batch.length) return;
    insertTipEvents(batch);
    applySoon();
  });
}

function flushPending() {
  if (!pending.length) return;
  const batch = pending.splice(0);
  insertTipEvents(batch);
  updateNewPill();
  applySoon();
  pruneTipDomGroups();
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
  if (settings.layout === "vertical") {
    scrollFeedToTip("smooth");
  } else {
    feed.scrollTo({ left: 0, behavior: "smooth" });
  }
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
    if (historyLoaderUserPinned && !historyLoading) {
      historyLoaderUserPinned = false;
      setHistoryLoading(false, false);
    }
    return false;
  }
  if (!historyLoadArmed) return false;
  historyLoadArmed = false;
  pinHistoryLoaderForUser();
  return true;
}
/** Minimum gap between scroll-driven prunes (ms). */
const SCROLL_PRUNE_MIN_MS = 400;
let lastScrollPruneAt = 0;
function onScrollDirection() {
  const pos = feedScrollPos();
  // Require clear movement toward history — ignore jitter / overscroll bounce.
  const towardHistory = pos > lastScrollPos + 2;
  const towardTip = pos < lastScrollPos - 2;
  lastScrollPos = pos;
  if (towardTip) {
    scheduleFlushPending();
    maybeLoadNewer();
  }
  // Keep the mounted window bounded whether at tip or reading history. Throttled
  // because the prune reads a rect per mounted group; it also runs on a 30s timer
  // and after every page load.
  const nowMs = Date.now();
  if (nowMs - lastScrollPruneAt > SCROLL_PRUNE_MIN_MS) {
    lastScrollPruneAt = nowMs;
    pruneTipDomGroups();
  }
  if (!towardHistory) {
    if (!nearHistoryEnd()) {
      historyLoadArmed = true;
      if (historyLoaderUserPinned && !historyLoading) {
        historyLoaderUserPinned = false;
        setHistoryLoading(false, false);
      }
    }
    return;
  }
  const q = $("search").value.trim();
  if (q) {
    if (searchPriming || searchExtending) return;
    if (!visibleFeedFillsPage() || consumeHistoryLoadArm()) extendSearchHistory();
    return;
  }
  if (consumeHistoryLoadArm()) {
    scheduleLoadHistory();
  }
}
/**
 * Scroll fires many times per frame and the handler reads layout
 * (`getBoundingClientRect` per group in the prune, `scrollHeight` in
 * nearHistoryEnd), so coalesce both listeners into one run per frame.
 */
let scrollRaf = 0;
function onScrollEvent() {
  if (scrollRaf) return;
  scrollRaf = requestAnimationFrame(() => {
    scrollRaf = 0;
    onScrollDirection();
  });
}
addEventListener("scroll", onScrollEvent, { passive: true });
feed.addEventListener("scroll", onScrollEvent, { passive: true });

// Feeds that don't fill the viewport can't scroll - treat wheel-down as load-more.
addEventListener("wheel", (e) => {
  if (e.deltaY <= 0) return;
  if (searchPriming || searchExtending) return;
  if (visibleFeedFillsPage()) return;
  // Short view: one wheel burst → one fill attempt (latch re-arms when short).
  if (!historyLoadArmed) return;
  historyLoadArmed = false;
  // Spinner follows the user reaching the bottom — not the network start.
  pinHistoryLoaderForUser();
  if ($("search").value.trim()) extendSearchHistory();
  else scheduleLoadHistory();
}, { passive: true });

/** Show the end-of-feed spinner because the viewer reached the bottom. */
function pinHistoryLoaderForUser() {
  historyLoaderUserPinned = true;
  setHistoryLoading(true);
}

/** Hide the spinner when a user-triggered load finishes (or leave "end" state). */
function releaseHistoryLoaderForUser(exhausted = false) {
  historyLoading = false;
  if (exhausted) {
    historyLoaderUserPinned = false;
    setHistoryLoading(false, true);
    return;
  }
  // Stay pinned only while still at the bottom and another load may follow;
  // otherwise clear so the next arrival at the bottom animates again.
  if (historyLoaderUserPinned && (nearHistoryEnd() || !visibleFeedFillsPage())) {
    setHistoryLoading(false, false);
    // Keep user-pin so a chained wait can show again only on next reach —
    // clear pin so the next scroll/wheel re-triggers the animation.
    historyLoaderUserPinned = false;
  } else {
    historyLoaderUserPinned = false;
    setHistoryLoading(false, false);
  }
}

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

/** True when the tip paint budget is exhausted (group count, not scrollHeight). */
function tipPaintBudgetExceeded() {
  return groupOrder.length >= TIP_PAINT_MAX_GROUPS;
}

/**
 * True when visible (filter-matching) cards fill at least one viewport.
 * Must NOT use tipPaintBudgetExceeded — sparse filters often hit the 20-group
 * tip cap before the page is scrollable, which used to disable wheel-to-load
 * and left the feed stuck at ~20 minutes of history.
 */
function visibleFeedFillsPage() {
  let n = 0;
  document.querySelectorAll("#feed .card").forEach((card) => {
    if (card.classList.contains("f-hide")) return;
    if (!settings.filters[card.dataset.category]) return;
    if (card.dataset.category === "finance" && !financeAppEnabled(financeNameOf(card))) return;
    if (card.dataset.category === "dapp" && !dappAppEnabled(card.dataset.dapp)) return;
    if (card.dataset.category === "governance" && !govTypeEnabled(card.dataset.govType)) return;
    n++;
  });
  if (!n) return false;
  // Force layout so mobile WebKit has up-to-date scroll metrics mid-paint.
  void document.documentElement.offsetHeight;
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
    if (card.dataset.category === "finance" && !financeAppEnabled(financeNameOf(card))) return;
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
  historyLoaderUserPinned = false;
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
  historyLoadTimer = setTimeout(maybeLoadHistory, 40);
}

function maybeLoadHistory() {
  // Active search pages matches from searchHitBuffer only.
  if ($("search").value.trim()) return;
  if (searchPriming || searchExtending || historyLoading || oldestEventId == null) return;
  // Nothing left in the 24h page buffer and disk history is exhausted.
  if (retentionHistoryDone && retentionReady && historyExhausted) return;
  // Never auto-drain history just because the filtered view looks "short"
  // (sparse filters + mobile scrollHeight flakes). Require an intentional
  // approach to the history end, or a nearly empty tip that still needs a seed.
  const atHistoryEnd = nearHistoryEnd() || historyLoaderUserPinned;
  if (!atHistoryEnd) {
    if (groupOrder.length >= TIP_DOM_MIN_GROUPS || tipPaintBudgetExceeded()) return;
  } else if (visibleFeedFillsPage() && !nearHistoryEnd() && !historyLoaderUserPinned) {
    return;
  }
  loadHistory();
}

/**
 * Pin viewport across a *synchronous* DOM mutation that would otherwise jump.
 *
 * Async work (network / yieldToBrowser) is not pinned: restoring a scrollY
 * captured before `await` snaps the user back to the bottom if they scroll
 * toward the tip while history is still loading.
 */
/**
 * Hold the reader's position across a DOM change.
 *
 * A group intersecting the viewport is measured before and after `fn`, and the
 * scroll is corrected by the difference, so whatever is on screen stays on
 * screen while content is added below or removed above. Groups touching the
 * viewport are never evicted, so the anchor survives the mutation.
 */
function withAnchoredScroll(fn) {
  if (settings.layout !== "vertical") return fn();
  const vh = window.innerHeight || 0;
  let anchor = null;
  for (const g of groupOrder) {
    const r = g.getBoundingClientRect();
    if (r.bottom > 0 && r.top < vh) { anchor = g; break; }
  }
  const before = anchor ? anchor.getBoundingClientRect().top : null;
  const result = fn();
  if (anchor && anchor.isConnected && before != null) {
    const delta = anchor.getBoundingClientRect().top - before;
    if (Math.abs(delta) > 0.5) window.scrollBy(0, delta);
  }
  return result;
}

function withPinnedFeedScroll(fn) {
  const y = window.scrollY;
  const x = feed.scrollLeft;
  const result = fn();
  if (result != null && typeof result.then === "function") {
    return result;
  }
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
 * Load older events when the user approaches the history end.
 * Appends one useful page (buffer or disk). Does not auto-drain the 24h window
 * just because the viewport looks short — that caused mobile/sparse-filter dumps.
 */
async function loadHistory() {
  if (searchPriming) return;
  if ($("search").value.trim()) return;
  if (historyLoading) return;

  if (!retentionReady) {
    startRetentionPreload();
    // Serve whatever the partially-hydrated cache already holds; wait for the
    // 24h load only when it holds nothing this filter can use.
    ensureFeedHitBuffer();
    if (feedHitOffset >= feedHitBuffer.length) {
      // Spinner already shown by scroll/wheel if the user is at the bottom.
      await whenRetentionReady();
      if ($("search").value.trim()) {
        releaseHistoryLoaderForUser(historyExhausted);
        return;
      }
    }
  }

  historyLoading = true;
  // Refresh from retention when the cache moved (dirty) — a stale hit list was
  // getting drained then skipped via seekPastRetentionWindow, freezing history
  // around the tip window (~20 minutes on sparse filters).
  ensureFeedHitBuffer();
  const beforeVisible = visibleMatchCount();
  const userAtEnd = nearHistoryEnd() || historyLoaderUserPinned;

  // One page per trigger; reaching the bottom again asks for the next page.
  if (feedHitOffset < feedHitBuffer.length) {
    if (!userAtEnd && groupOrder.length >= TIP_DOM_MIN_GROUPS) {
      releaseHistoryLoaderForUser(false);
      return;
    }
    withAnchoredScroll(() => {
      renderFeedPage();
      resortFeedBySlot();
      applyFilters();
    });
    pruneTipDomGroups();
    historyLoadArmed = true;
    releaseHistoryLoaderForUser(false);
    return;
  }

  // In-memory hit list drained. If 24h is still hydrating, wait — do not hit disk.
  if (!retentionReady) {
    await whenRetentionReady();
    if ($("search").value.trim()) {
      releaseHistoryLoaderForUser(historyExhausted);
      return;
    }
    ensureFeedHitBuffer(true);
    if (feedHitOffset < feedHitBuffer.length) {
      historyLoadArmed = true;
      historyLoading = false;
      pinHistoryLoaderForUser();
      setTimeout(() => maybeLoadHistory(), 0);
      return;
    }
    releaseHistoryLoaderForUser(false);
    return;
  }

  // One more rebuild now that retention is complete — then maybe paint, not disk.
  ensureFeedHitBuffer(true);
  if (feedHitOffset < feedHitBuffer.length) {
    historyLoadArmed = true;
    historyLoading = false;
    pinHistoryLoaderForUser();
    setTimeout(() => maybeLoadHistory(), 0);
    return;
  }

  // Past the 24h filtered buffer — disk history (user-initiated only).
  retentionHistoryDone = true;
  if (!userAtEnd) {
    releaseHistoryLoaderForUser(false);
    return;
  }
  if (historyExhausted) {
    releaseHistoryLoaderForUser(true);
    return;
  }

  // Jump past the in-memory window. oldestEventId is often a mid-window match
  // (e.g. oldest Iagon in 24h); without this, /api/events re-walks tens of
  // thousands of non-matching retention rows and never reaches disk.
  seekPastRetentionWindow();

  const ac = new AbortController();
  historyAbort = ac;
  try {
    // Seek until at least one new visible match lands (or a small page budget).
    const targetVisible = beforeVisible + 1;
    const maxPages = 40;
    let pages = 0;
    while (
      !historyExhausted
      && pages < maxPages
      && visibleMatchCount() < targetVisible
    ) {
      const events = await withPinnedFeedScroll(() =>
        fetchHistoryPage(ac.signal, {
          matchesOnly: true,
          limit: HISTORY_SEEK_PAGE_SIZE,
        }),
      );
      pages++;
      if (events == null) break;
      pruneTipDomGroups();
      if (visibleMatchCount() >= targetVisible) break;
      await yieldToBrowser();
    }
    pruneTipDomGroups();
    historyLoadArmed = true;
  } catch (e) {
    if (e?.name !== "AbortError") { /* best-effort */ }
    historyLoadArmed = true;
  }
  if (historyAbort === ac) historyAbort = null;
  releaseHistoryLoaderForUser(historyExhausted);
}

/** Smallest event id currently held in the 24h client cache. */
function retentionFloorId() {
  let min = null;
  for (const id of retentionCache.keys()) {
    if (min == null || id < min) min = id;
  }
  return min;
}

/**
 * Point history pagination at the oldest cached retention id so the next
 * /api/events page is older than the 24h window (disk), not a re-scan of it.
 */
function seekPastRetentionWindow() {
  const floor = retentionFloorId();
  if (floor == null) return;
  if (oldestEventId == null || oldestEventId > floor) {
    oldestEventId = floor;
  }
}

/** Fetch one older disk page into the feed. Returns events, or null on abort/empty. */
async function fetchHistoryPage(signal, opts = {}) {
  if (oldestEventId == null || historyExhausted) return null;
  const limit = opts.limit || HISTORY_PAGE_SIZE;
  const r = await fetch(`/api/events?before=${oldestEventId}&limit=${limit}`, {
    signal,
  });
  const m = await r.json();
  const events = m.events || [];
  if (m.exhausted || !events.length) historyExhausted = true;
  if (events.length) {
    if (opts.matchesOnly) {
      // Index everything so pagination/cache stay correct, but only mount hits.
      for (const ev of events) {
        if (ev?.id == null) continue;
        retentionIndex(ev);
        if (oldestEventId == null || ev.id < oldestEventId) oldestEventId = ev.id;
      }
      const matches = events.filter((ev) => eventPassesFeedFilters(ev));
      if (matches.length) {
        routeHistoricalBatch(matches);
        prefetchUnitsFromEvents(matches);
        applyFilters();
      }
    } else {
      routeHistoricalBatch(events);
      prefetchUnitsFromEvents(events);
      applyFilters();
    }
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
    const text = label || "Loading more events…";
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
const handleMeta = new Map(Object.entries(store.get("co_handles_v1", {})));
const handleWaiters = new Map(); // stake → elements waiting on in-flight fetch
let handlesDisabled = false;

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

function persistHandleCache() {
  const obj = {};
  let i = 0;
  for (const [k, v] of handleMeta) {
    if (!v || !v.handle) continue;
    if (i++ > 700) break;
    obj[k] = { handle: v.handle };
  }
  store.set("co_handles_v1", obj);
}

function enrichHandles(root) {
  if (handlesDisabled) return;
  root.querySelectorAll("[data-handle], [data-stake]").forEach((el) => {
    const id = el.dataset.handle || el.dataset.stake;
    if (!id || !isLookupHandleAddr(id)) return;
    const cached = handleMeta.get(id);
    if (cached && cached.handle) {
      paintHandle(el, cached);
      return;
    }
    if (cached && cached.handle === null) return; // negative cache
    if (handleWaiters.has(id)) {
      handleWaiters.get(id).push(el);
      return;
    }
    handleWaiters.set(id, [el]);
    fetch(`/api/handle/${encodeURIComponent(id)}`)
      .then((r) => r.json())
      .then((meta) => {
        if (meta && meta.disabled) {
          handlesDisabled = true;
          handleWaiters.delete(id);
          return;
        }
        if (meta && meta.handle) {
          handleMeta.set(id, { handle: String(meta.handle) });
          persistHandleCache();
        } else {
          handleMeta.set(id, { handle: null }); // negative cache
        }
        const waiters = handleWaiters.get(id) || [];
        handleWaiters.delete(id);
        const all = new Set([
          ...waiters,
          ...document.querySelectorAll(
            `[data-handle="${CSS.escape(id)}"], [data-stake="${CSS.escape(id)}"]`
          ),
        ]);
        all.forEach((e) => paintHandle(e, handleMeta.get(id)));
      })
      .catch(() => {
        handleWaiters.delete(id);
        if ($("search").value.trim()) scheduleFilterRefresh();
      });
  });
}

function paintHandle(el, meta) {
  if (!meta) return;
  const handle = typeof meta.handle === "string" && meta.handle ? meta.handle : "";
  const addr = el.dataset.handle || el.dataset.stake || "";
  const card = el.closest(".card");
  if (card) {
    appendCardSearch(card, addr, handle, handle ? `$${handle}` : "");
    const eid = Number(card.dataset.eid);
    if (Number.isFinite(eid) && retentionCache.has(eid)) {
      retentionIndex(retentionCache.get(eid).ev);
    }
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
  if (!handle) return; // leave truncated address as fallback
  el.innerHTML = `<span class="ada-handle-dollar">$</span>${esc(handle)}`;
  el.classList.remove("hash");
  el.classList.add("ada-handle");
  // Keep copyable titles; otherwise show the underlying address on hover.
  el.title = el.classList.contains("copyable") ? "click to copy" : addr;
  // Handle labels change pill width — refit the +N overflow.
  const stakesEl = el.closest(".tx-stakes");
  if (stakesEl) requestAnimationFrame(() => fitTxStakes(stakesEl));
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
  mTitle.textContent = formatEventTitle(ev);
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
    const detail = await r.json().catch(() => ({}));
    if (!r.ok) {
      mBody.innerHTML =
        `<div class="m-empty">Full transaction details are unavailable right now.</div>${renderEventDetail(ev)}${explorers(hash)}`;
      await hydrateVoteRationales(ev, null);
      return;
    }
    mBody.innerHTML = detail.tx
      ? renderOgmiosTx(hash, detail.tx, detail.block, ev)
      : renderBlockfrostTx(hash, detail.blockfrost, ev);
    enrichAssets(mBody);
    enrichHandles(mBody);
    await hydrateVoteRationales(ev, detail.tx || null);
  } catch {
    mBody.innerHTML =
      `<div class="m-empty">Transaction details are unavailable.</div>${renderEventDetail(ev)}${explorers(hash)}`;
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
      <div>${addressSpan(o.address || "?")}</div>
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
    ${voteRationaleMountHtml(ev, tx)}
    ${jsonSection("Metadata", tx.metadata?.labels)}
    ${jsonSection("Required signers", tx.requiredExtraSignatories)}
    <div class="m-section"><details class="raw"><summary>Raw transaction JSON</summary>
      <pre class="json">${esc(JSON.stringify(tx, null, 2))}</pre></details></div>
    ${explorers(hash)}`;
}

/** Unique CIP-100/136 anchor URLs attached to this vote / tx. */
function voteAnchorUrls(ev, tx) {
  const urls = [];
  const push = (u) => {
    const s = typeof u === "string" ? u.trim() : "";
    if (s && !urls.includes(s)) urls.push(s);
  };
  if (ev?.kind === "gov_vote") push(ev.data?.anchorUrl);
  for (const v of tx?.votes || []) push(v?.metadata?.url);
  // Also surface from the event when the modal was opened on a vote card
  // even if Ogmios omitted metadata on the cached tx shape.
  push(ev?.data?.anchorUrl);
  return urls;
}

function voteRationaleMountHtml(ev, tx) {
  if (!voteAnchorUrls(ev, tx).length) return "";
  return `<div class="m-section" id="m-vote-rationale">
    <h3>Vote rationale</h3>
    <div class="rationale-host"><div class="m-empty" style="padding:12px 0">Loading rationale…</div></div>
  </div>`;
}

async function hydrateVoteRationales(ev, tx) {
  const urls = voteAnchorUrls(ev, tx);
  if (!urls.length) return;
  let host = mBody.querySelector("#m-vote-rationale .rationale-host");
  // Tx-fetch failure path still has the event's anchorUrl — mount the section.
  if (!host) {
    const wrap = document.createElement("div");
    wrap.className = "m-section";
    wrap.id = "m-vote-rationale";
    wrap.innerHTML =
      `<h3>Vote rationale</h3><div class="rationale-host"><div class="m-empty" style="padding:12px 0">Loading rationale…</div></div>`;
    mBody.insertBefore(wrap, mBody.firstChild);
    host = wrap.querySelector(".rationale-host");
  }
  if (!host) return;

  const parts = await Promise.all(
    urls.map(async (url) => {
      try {
        const r = await fetch(`/api/vote-rationale?url=${encodeURIComponent(url)}`);
        const data = await r.json().catch(() => ({}));
        if (!r.ok) {
          return rationaleMissHtml(url);
        }
        return renderVoteRationale(data);
      } catch {
        return rationaleMissHtml(url);
      }
    }),
  );
  host.innerHTML = parts.join("") || `<div class="m-empty" style="padding:12px 0">No rationale available.</div>`;
}

function rationaleMissHtml(url) {
  return `<div class="rationale-card">
    <div class="rationale-meta">Rationale could not be loaded.
      <a class="hash" href="${esc(url)}" target="_blank" rel="noopener">${esc(short(url, 28, 12))}</a>
    </div>
  </div>`;
}

function renderVoteRationale(d) {
  const fields = [
    ["Summary", d.summary],
    ["Comment", d.comment],
    ["Rationale", d.rationaleStatement],
    ["Precedent", d.precedentDiscussion],
    ["Counterarguments", d.counterargumentDiscussion],
    ["Conclusion", d.conclusion],
  ].filter(([, v]) => typeof v === "string" && v.trim());

  const authors = Array.isArray(d.authors) && d.authors.length
    ? `<div class="rationale-meta">By ${d.authors.map((a) => esc(a)).join(", ")}</div>`
    : "";
  const refs = Array.isArray(d.references) && d.references.length
    ? `<div class="rationale-refs">${d.references.map((r) => {
        const label = r.label || r.uri || "link";
        const href = r.uri || "#";
        return `<a href="${esc(href)}" target="_blank" rel="noopener">${esc(label)}</a>`;
      }).join("")}</div>`
    : "";
  const source = d.url
    ? `<div class="rationale-meta"><a class="hash" href="${esc(d.resolvedUrl || d.url)}" target="_blank" rel="noopener" title="${esc(d.url)}">${esc(short(d.url, 36, 14))}</a></div>`
    : "";

  if (!fields.length) {
    return `<div class="rationale-card">${authors}${source}
      <div class="rationale-meta">Anchor has no comment / rationale text.</div>${refs}</div>`;
  }

  const body = fields
    .map(([label, text]) => {
      const showLabel = fields.length > 1 || label !== "Comment";
      return `<div class="rationale-block">${
        showLabel ? `<div class="rationale-label">${esc(label)}</div>` : ""
      }<div class="rationale-text">${formatRationaleText(text)}</div></div>`;
    })
    .join("");

  return `<div class="rationale-card">${authors}${body}${refs}${source}</div>`;
}

/** Escape, light markdown links, and paragraph breaks for CIP-100 body text. */
function formatRationaleText(raw) {
  const src = String(raw || "").replace(/\r\n/g, "\n").trim();
  if (!src) return "";
  return src
    .split(/\n{2,}/)
    .map((para) => {
      let t = esc(para);
      // Reference-style defs: [pdf-link]: https://…
      t = t.replace(
        /^\[([^\]]+)\]:\s*(https?:\/\/\S+)\s*$/gm,
        '<span class="rationale-meta"><a href="$2" target="_blank" rel="noopener">$1</a></span>',
      );
      // Inline [label](https://…)
      t = t.replace(
        /\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g,
        '<a href="$2" target="_blank" rel="noopener">$1</a>',
      );
      // Bare URLs (skip ones already inside href=")
      t = t.replace(
        /(^|[\s>(])(https?:\/\/[^\s<]+)/g,
        (m, pre, url) => {
          if (pre === "=" || pre.endsWith("=\"")) return m;
          return `${pre}<a href="${url}" target="_blank" rel="noopener">${url}</a>`;
        },
      );
      return `<p>${t.replace(/\n/g, "<br>")}</p>`;
    })
    .join("");
}

function renderBlockfrostTx(hash, bf, ev) {
  if (!bf || !bf.tx) return `<div class="m-empty">No details available.</div>${explorers(hash)}`;
  const tx = bf.tx;
  const utxos = bf.utxos || {};
  const io = (list, dir) => (list || []).map((u) => `
    <div class="utxo">
      <div>${addressSpan(u.address || "?")}</div>
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
    ${voteRationaleMountHtml(ev, null)}
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
        blocksByHash.clear();
        txByHash.clear();
        dexOrderSettlement.clear();
        pendingDexSettlements.length = 0;
        orphanedBlocks.clear();
        historyLoadArmed = true;
        historyLoaderUserPinned = false;
        feedRebuildGen++;
        feedRebuilding = false;
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
ensureHierarchyPipeObserver();
ensureGroupViewObserver();
scheduleDomPruneLoop();
applyFilters();
loadRegistryMeta();
loadDrepMeta();
loadGovMeta();
connect();
window.addEventListener("resize", scheduleHierarchyPipes);


