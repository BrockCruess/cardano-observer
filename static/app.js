/* cardano-observer frontend - zero dependencies, one WebSocket. */
"use strict";

/* ── Category & icon registry ─────────────────────────────────────────── */

const CATS = [
  { id: "block",       label: "Blocks" },
  { id: "token",       label: "Tokens" },
  { id: "transaction", label: "Transactions" },
  { id: "dex",         label: "DEX" },
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
  vote_delegation: svg('<path d="M12 3l8 4-8 4-8-4 8-4z"/><path d="M4 13l8 4 8-4"/><path d="M9 18.5l2 2 4-4.5"/>'),
  stake_registration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 8h5M19.5 5.5v5" stroke-width="1.6"/>'),
  stake_deregistration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 8h5" stroke-width="1.6"/>'),
  withdrawal: svg('<path d="M5 11v8h14v-8"/><path d="M12 16V4"/><path d="M8.5 7.5L12 4l3.5 3.5"/>'),
  pool: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/>'),
  pool_registration: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/>'),
  pool_retirement: svg('<rect x="6" y="6" width="12" height="12" rx="1.5"/><path d="M9 9h6v6H9z"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/><path d="M4 4l16 16"/>'),
  gov_proposal: svg('<path d="M12 4v16"/><path d="M5 7h14"/><path d="M5 7l-2.6 5.2a3 3 0 0 0 5.9 0L5.7 7"/><path d="M18.3 7l-2.6 5.2a3 3 0 0 0 5.9 0L19 7"/><path d="M8 20h8"/>'),
  gov_vote: svg('<rect x="5" y="4" width="14" height="16" rx="2"/><path d="M9 12l2.2 2.2L15.5 9.5"/>'),
  drep_registration: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M16.5 3.5l2 2 3.5-4" stroke-width="1.6"/>'),
  drep_update: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/>'),
  drep_retirement: svg('<circle cx="12" cy="8" r="4"/><path d="M4 20c1.5-3.8 4.8-5.5 8-5.5s6.5 1.7 8 5.5"/><path d="M17 5h5" stroke-width="1.6"/>'),
  committee_auth: svg('<circle cx="8" cy="8" r="3.4"/><circle cx="16" cy="8" r="3.4"/><path d="M2.5 20c1.2-3.2 3.4-4.6 5.5-4.6M21.5 20c-1.2-3.2-3.4-4.6-5.5-4.6"/>'),
  committee_resign: svg('<circle cx="8" cy="8" r="3.4"/><circle cx="16" cy="8" r="3.4"/><path d="M2.5 20c1.2-3.2 3.4-4.6 5.5-4.6M21.5 20c-1.2-3.2-3.4-4.6-5.5-4.6"/><path d="M10 12l4-4"/>'),
  tx_metadata: svg('<path d="M4 5h16v11H9.5L4 20V5z"/><path d="M8 9h8M8 12h5"/>'),
  certificate: svg('<path d="M7 3h7l5 5v13H7V3z"/><path d="M14 3v5h5"/><path d="M10 13h5M10 16h5"/>'),
  rollback: svg('<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>'),
  orphaned_block: svg('<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z"/><path d="M4.5 6L19.5 18" stroke-dasharray="2.5 2.5"/>'),
  slot_battle: svg('<path d="M13 2L4.5 13.5h5.6L9 22l8.5-11.5h-5.6L13 2z"/>'),
  dex: svg('<circle cx="12" cy="12" r="9"/><path d="M8 10h7M12.5 7.5L15 10l-2.5 2.5"/><path d="M16 14.5H9M11.5 12l-2.5 2.5L11.5 17"/>'),
  dex_order: svg('<circle cx="12" cy="12" r="9"/><path d="M12 8v8M8 12h8"/>'),
  dex_fill: svg('<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.5 2.5L16.5 9"/>'),
  dex_lp: svg('<path d="M12 3v18"/><path d="M5 8h14"/><path d="M7 8c0 4 2.5 8 5 10"/><path d="M17 8c0 4-2.5 8-5 10"/><path d="M8 14h8"/>'),
  dex_lp_redeem: svg('<path d="M5 11v8h14v-8"/><path d="M12 16V4"/><path d="M8.5 7.5L12 4l3.5 3.5"/>'),
  dex_cancel: svg('<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>'),
};
const iconFor = (kind, category, side) => {
  if (kind === "dex_lp" && side === "redeem") return ICONS.dex_lp_redeem;
  return ICONS[kind] || ICONS[{ token: "token_transfer", staking: "delegation", governance: "gov_proposal", metadata: "tx_metadata", alert: "slot_battle", dex: "dex", pool: "pool", mint: "mint" }[category]] || ICONS.transaction;
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

/** Format on-chain qty only when decimals are known - never invent M/B from raw units. */
function fmtTokenQty(qtyStr, decimals) {
  if (decimals == null || decimals === "" || Number.isNaN(Number(decimals))) return "…";
  return fmtQty(qtyStr, Number(decimals));
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

/* ── Global state ─────────────────────────────────────────────────────── */

let NETWORK = "mainnet";
const feed = $("feed");
const groups = new Map();      // block hash -> .block-group element
const groupOrder = [];         // newest first
const MAX_GROUPS = 50_000; // keep history while scrolling back; soft safety cap
const pending = [];            // buffered events while user is reading
let oldestEventId = null;      // smallest id currently in the feed
let historyExhausted = false;
let historyLoading = false;
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
const SEARCH_PRIME_LOOKBACK = 300;
const catCounts = Object.fromEntries(CATS.map((c) => [c.id, 0]));
const eventTimes = [];         // timestamps (ms) for epm/sparkline
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
  // min-ADA + search need per-card logic
  const q = $("search").value.trim().toLowerCase();
  let visibleEvents = 0;
  for (const g of groupOrder) {
    let visible = 0;
    g.querySelectorAll(".card").forEach((card) => {
      let hide = false;
      if (minL > 0 && card.dataset.category === "transaction" && Number(card.dataset.ada || 0) < minL) hide = true;
      if (q && !(card.dataset.search || "").includes(q)) hide = true;
      card.classList.toggle("f-hide", hide);
      if (!hide && settings.filters[card.dataset.category]) {
        visible++;
        visibleEvents++;
      }
    });
    g.classList.toggle("f-hide", q !== "" && visible === 0);
  }
  store.set("co_filters_v1", settings.filters);
  store.set("co_minada_v1", settings.minAda);
  updateVisibleEventCount(visibleEvents);
  if (!searchPriming && $("search").value.trim()) {
    updateSearchEmptyPrompt();
  } else if (!$("search").value.trim()) {
    hideSearchPrompts();
  }
}

function updateVisibleEventCount(n) {
  const el = $("ft-session");
  if (!el) return;
  let shown = n;
  if (shown == null) {
    shown = 0;
    document.querySelectorAll("#feed .card:not(.f-hide)").forEach((card) => {
      if (settings.filters[card.dataset.category]) shown++;
    });
  }
  el.textContent = `${fmtInt(shown)} event${shown === 1 ? "" : "s"}`;
}

/** Pre-fill search from the URL: `?minswap`, `?q=minswap`, or `?search=NUTS`. */
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

function buildToolbar() {
  const chips = $("chips");
  for (const c of CATS) {
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
      // Typed search: offer load-more if the loaded tip window has no hits.
      // Wait for pool tickers before declaring empty (same grace as URL prime).
      if (!searchPriming && !urlSearchPreset) {
        searchScanned = Math.max(searchScanned, document.querySelectorAll("#feed .card").length);
        armSearchEmptyGrace();
        updateSearchEmptyPrompt();
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

function bumpCatCount(cat) {
  catCounts[cat] = (catCounts[cat] || 0) + 1;
}
setInterval(() => {
  for (const c of CATS) {
    const el = document.querySelector(`[data-cat-n="${c.id}"]`);
    if (el) el.textContent = catCounts[c.id] ? fmtInt(catCounts[c.id]) : "";
  }
}, 1200);

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
      const meta = unit ? assetMeta.get(unit) : null;
      const label = meta?.ticker || meta?.name || a.name
        || (a.fingerprint ? short(a.fingerprint, 10, 4) : short(a.unit, 10, 4));
      const decimals = meta?.decimals != null ? Number(meta.decimals) : null;
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
  const meta = unit ? assetMeta.get(unit) : null;
  const decimals = meta?.decimals != null ? Number(meta.decimals) : null;
  const label = meta?.ticker || meta?.name || a.name
    || (a.fingerprint ? short(a.fingerprint, 8, 4) : short(a.unit, 8, 4));
  const qty = `<span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span>`;
  const name = `<span class="t">${esc(label)}</span>`;
  // data-unit lets enrichAssets apply on-chain token decimals (fixes 344.35M → 344.35).
  return `<b class="dex-amt" data-unit="${esc(unit)}">${min ? "≥" : ""}${qty} ${name}</b>`;
}
function dexAmtTokens(assets) {
  const items = (assets && assets.items) || [];
  return items.map((a) => {
    const unit = a.unit || "";
    const meta = unit ? assetMeta.get(unit) : null;
    const decimals = meta?.decimals != null ? Number(meta.decimals) : null;
    const label = meta?.ticker || meta?.name || a.name
      || (a.fingerprint ? short(a.fingerprint, 8, 4) : short(a.unit, 8, 4));
    return `<b class="dex-amt" data-unit="${esc(unit)}"><span class="q" data-qty="${esc(a.qty)}">${fmtTokenQty(a.qty, decimals)}</span> <span class="t">${esc(label)}</span></b>`;
  }).join(" + ");
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
  const want = wantAda || wantTok || (d.wantQty != null && d.wantQty !== ""
    ? `<b>${d.wantMin ? "≥" : ""}${fmtTokenQty(d.wantQty, null)}</b>`
    : "");
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
      const fmtDrep = (id) => {
        if (!id) return "";
        const label = id.length > 24 ? short(id, 10, 5) : id;
        return `<b title="${esc(id)}">${esc(label)}</b>`;
      };
      const from = d.fromDrep ? fmtDrep(d.fromDrep) : (d.stake ? `<span class="hash">${esc(short(d.stake, 12, 5))}</span>` : "");
      const to = fmtDrep(d.drep);
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
    case "gov_proposal":
      return sub([
        d.deposit ? `deposit <b>${fmtAda(d.deposit)}</b>` : "",
        d.anchorUrl ? `<span class="hash">${esc(short(d.anchorUrl, 22, 0))}</span>` : "",
      ]);
    case "gov_vote": {
      const v = String(d.vote || "").toLowerCase();
      const cls = v === "yes" ? "yes" : v === "no" ? "no" : "abstain";
      return sub([
        `<span class="badge ${cls}">${esc(v.toUpperCase())}</span>`,
        d.role ? esc(roleLabel(d.role)) : "",
        d.voter ? `<span class="hash">${esc(short(d.voter, 12, 5))}</span>` : "",
        d.proposalTx ? `on <span class="hash">${esc(short(d.proposalTx, 8, 4))}#${esc(String(d.proposalIndex ?? 0))}</span>` : "",
      ]);
    }
    case "drep_registration":
    case "drep_update":
    case "drep_retirement":
      return d.drep ? `<span class="hash">${esc(short(String(d.drep), 14, 6))}</span>` : "";
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
    default:
      return esc(ev.summary || "");
  }
}

const KIND_LABEL = {
  block: "block", transaction: "tx", token_transfer: "tokens", mint: "mint", burn: "burn",
  delegation: "stake", vote_delegation: "governance", stake_registration: "stake",
  stake_deregistration: "stake", withdrawal: "rewards", pool_registration: "pool",
  pool_retirement: "pool", gov_proposal: "governance", gov_vote: "vote",
  drep_registration: "drep", drep_update: "drep", drep_retirement: "drep",
  committee_auth: "committee", committee_resign: "committee", tx_metadata: "metadata",
  certificate: "cert", rollback: "fork", orphaned_block: "fork", slot_battle: "battle",
  dex_order: "dex", dex_fill: "dex", dex_lp: "dex", dex_cancel: "dex",
};

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
  return bits.filter(Boolean).join(" ").toLowerCase().slice(0, 4000);
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
  if (ev.tx_hash) card.dataset.tx = ev.tx_hash;
  if (ev.data && ev.data.ada != null) card.dataset.ada = ev.data.ada;

  const title = ev.kind === "block"
    ? `Block <span class="height">${fmtInt(ev.height)}</span>`
    : esc(titleCaseWords(ev.title));

  card.innerHTML = `
    <div class="ev-icon">${iconFor(ev.kind, ev.category, ev.data?.side)}</div>
    <div class="ev-body">
      <div class="ev-head">
        <span class="ev-title">${title}</span>
        <span class="ev-kind">${esc(KIND_LABEL[ev.kind] || ev.category)}</span>
        <span class="ev-time" data-ts="${ev.timestamp}" title="${esc(clock(ev.timestamp))}">${timeAgo(ev.timestamp)}</span>
      </div>
      <div class="ev-sub">${cardBody(ev)}</div>
    </div>`;

  card.dataset.search = cardSearchText(ev);

  card.addEventListener("click", () => openModal(ev));
  enrichAssets(card);
  enrichPools(card);
  return card;
}

/* ── Feed assembly: block groups on the chain spine ───────────────────── */

function newGroup(blockHash, atEnd = false) {
  const g = document.createElement("div");
  g.className = "block-group";
  if (blockHash) g.dataset.block = blockHash;
  const evs = document.createElement("div");
  evs.className = "group-events";
  g.appendChild(evs);
  if (atEnd) {
    feed.appendChild(g);
    groupOrder.push(g);
  } else {
    feed.prepend(g);
    groupOrder.unshift(g);
  }
  if (blockHash) groups.set(blockHash, g);
  while (groupOrder.length > MAX_GROUPS) {
    const old = atEnd ? groupOrder.shift() : groupOrder.pop();
    if (old?.dataset.block) groups.delete(old.dataset.block);
    old?.remove();
  }
  return g;
}

function noteEventId(ev) {
  if (ev?.id == null) return;
  if (oldestEventId == null || ev.id < oldestEventId) oldestEventId = ev.id;
}

function routeEvent(ev) {
  sessionEvents++;
  bumpCatCount(ev.category);
  eventTimes.push(Date.now());
  noteEventId(ev);

  if (ev.kind === "block") {
    let g = ev.block_hash ? groups.get(ev.block_hash) : null;
    if (!g) g = newGroup(ev.block_hash);
    g.prepend(buildCard(ev));
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
  if (!g) g = newGroup(ev.block_hash);
  g.querySelector(".group-events").appendChild(buildCard(ev));
}

function standaloneCard(ev, atEnd = false) {
  const g = newGroup(null, atEnd);
  g.querySelector(".group-events").appendChild(buildCard(ev));
}

/** Append a page of older events (oldest→newest) below the current feed. */
function routeHistoricalBatch(events) {
  // Per-group insert anchor: older cards go before the first card that was
  // already on screen for that block.
  const anchors = new Map();
  for (const ev of events) {
    sessionEvents++;
    bumpCatCount(ev.category);
    noteEventId(ev);

    if (ev.kind === "rollback" || ev.kind === "slot_battle" || ev.kind === "orphaned_block") {
      if (ev.kind === "orphaned_block") {
        const g = ev.block_hash && groups.get(ev.block_hash);
        if (g) g.classList.add("orphaned");
      }
      standaloneCard(ev, true);
      continue;
    }

    const key = ev.block_hash || `__id_${ev.id}`;
    let g = ev.block_hash ? groups.get(ev.block_hash) : null;
    const created = !g;
    if (!g) g = newGroup(ev.block_hash, true);

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

function onEvent(ev) {
  if (isPaused()) {
    pending.push(ev);
    if (pending.length > 800) pending.shift();
    $("newpill-n").textContent = fmtInt(pending.length);
    $("newpill").classList.add("show");
  } else {
    routeEvent(ev);
    applySoon();
  }
}

function flushPending() {
  while (pending.length) routeEvent(pending.shift());
  $("newpill").classList.remove("show");
  applySoon();
}

$("newpill").onclick = () => {
  if (settings.layout === "vertical") window.scrollTo({ top: 0, behavior: "smooth" });
  else feed.scrollTo({ left: 0, behavior: "smooth" });
  setTimeout(flushPending, 350);
};
addEventListener("scroll", () => {
  if (!isPaused() && pending.length) flushPending();
  maybeLoadHistory();
}, { passive: true });
feed.addEventListener("scroll", () => {
  if (!isPaused() && pending.length) flushPending();
  maybeLoadHistory();
}, { passive: true });

// Filtered feeds with few hits often aren't tall enough to scroll. Treat
// wheel-down as "load more history" in that case (same chunk as the button).
addEventListener("wheel", (e) => {
  if (e.deltaY <= 0) return;
  if (!$("search").value.trim()) return;
  if (searchPriming || searchExtending || historyLoading || historyExhausted) return;
  if (visibleFeedFillsPage()) return;
  extendSearchHistory();
}, { passive: true });

function nearHistoryEnd() {
  if (settings.layout === "vertical") {
    const room = document.documentElement.scrollHeight - window.scrollY - window.innerHeight;
    return room < 800;
  }
  return feed.scrollWidth - feed.scrollLeft - feed.clientWidth < 800;
}

/** True when visible (filter-matching) cards fill at least one viewport. */
function visibleFeedFillsPage() {
  const shown = document.querySelectorAll("#feed .card:not(.f-hide)");
  if (!shown.length) return false;
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
    if (settings.filters[card.dataset.category]) n++;
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
  if (historyExhausted) {
    if (text) {
      text.textContent = `No results found in the past ${fmtInt(n)} events. Beginning of recorded history.`;
    }
    if (btn) btn.hidden = true;
  } else {
    if (text) {
      text.textContent = `No results found in the past ${fmtInt(n)} events.`;
    }
    if (btn) {
      btn.hidden = false;
      btn.disabled = false;
      btn.textContent = "Load more history?";
    }
  }
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
  const matchLabel = `${fmtInt(matches)} match${matches === 1 ? "" : "es"}`;
  if (historyExhausted) {
    if (text) {
      text.textContent = `${matchLabel} in the past ${fmtInt(n)} events. Beginning of recorded history.`;
    }
    if (btn) btn.hidden = true;
  } else {
    if (text) {
      text.textContent = `${matchLabel} in the past ${fmtInt(n)} events.`;
    }
    if (btn) {
      btn.hidden = false;
      btn.disabled = false;
      btn.textContent = "Load more history";
    }
  }
}

function hideSearchPrompts() {
  setSearchEmpty(false);
  setSearchMore(false);
}

/** Pool/asset metadata still resolving - search haystacks may gain tickers shortly. */
function enrichmentPending() {
  return poolWaiters.size > 0;
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
  if (searchPriming) {
    hideSearchPrompts();
    return;
  }
  const q = $("search").value.trim();
  if (!q) {
    hideSearchPrompts();
    return;
  }
  const matches = visibleMatchCount();
  if (matches > 0) {
    setSearchEmpty(false);
    if (!searchPriming) setSearchPrime(false);
    if (searchScanned > 0) {
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
  if (searchScanned <= 0) {
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

function cancelSearchPrime() {
  searchPrimeGen++;
  if (historyAbort) {
    try { historyAbort.abort(); } catch { /* ignore */ }
    historyAbort = null;
  }
  searchPriming = false;
  historyLoading = false;
  searchScanned = 0;
  searchEmptyGraceUntil = 0;
  clearTimeout(emptyRecheckTimer);
  setSearchPrime(false);
  hideSearchPrompts();
  applyFilters();
  queueMicrotask(() => maybeLoadHistory());
}

/**
 * Pull another SEARCH_PRIME_LOOKBACK chunk for the active search.
 * Used by both the empty-state and "already have matches" load-more controls.
 */
async function extendSearchHistory() {
  if (searchPriming || searchExtending || historyLoading || historyExhausted) return;
  if (!$("search").value.trim()) return;
  const gen = ++searchPrimeGen;
  const hadMatches = visibleMatchCount() > 0;
  searchExtending = true;
  const btns = [$("search-empty-more"), $("search-more-btn")].filter(Boolean);
  for (const btn of btns) {
    btn.disabled = true;
    btn.textContent = "Loading…";
  }
  // Keep existing matches visible - only use the full-feed spinner when empty.
  if (hadMatches) {
    setSearchEmpty(false);
    setSearchMore(true, searchScanned, visibleMatchCount());
    const btn = $("search-more-btn");
    if (btn) {
      btn.disabled = true;
      btn.textContent = "Loading…";
    }
  } else {
    searchPriming = true;
    hideSearchPrompts();
    setSearchPrime(true);
  }

  try {
    const target = searchScanned + SEARCH_PRIME_LOOKBACK;
    while (
      gen === searchPrimeGen
      && $("search").value.trim()
      && searchScanned < target
      && !historyExhausted
      && oldestEventId != null
    ) {
      const batch = await fetchAndRouteHistoryPage();
      if (gen !== searchPrimeGen) return;
      if (!batch.length) break;
      searchScanned += batch.length;
      if (!hadMatches) {
        armSearchEmptyGrace();
        await waitForEnrichment(gen);
        if (gen !== searchPrimeGen) return;
      } else {
        applyFilters();
      }
    }
  } finally {
    if (gen === searchPrimeGen) {
      searchExtending = false;
      searchPriming = false;
      setSearchPrime(false);
      applyFilters();
      updateSearchEmptyPrompt();
    } else {
      searchExtending = false;
    }
  }
}

function maybeLoadHistory() {
  if (searchPriming || searchExtending || historyLoading || historyExhausted || oldestEventId == null) return;
  if (!nearHistoryEnd()) return;
  loadHistory();
}

/** Fetch one older page and route it. Returns the events array (maybe empty). */
async function fetchAndRouteHistoryPage() {
  if (historyLoading || historyExhausted || oldestEventId == null) return [];
  historyLoading = true;
  if (!searchPriming) setHistoryLoading(true);
  const ac = new AbortController();
  historyAbort = ac;
  let events = [];
  try {
    const r = await fetch(`/api/events?before=${oldestEventId}&limit=100`, {
      signal: ac.signal,
    });
    const m = await r.json();
    events = m.events || [];
    if (m.exhausted || !events.length) historyExhausted = true;
    if (events.length) {
      routeHistoricalBatch(events);
      prefetchUnitsFromEvents(events);
      applyFilters();
    }
  } catch (e) {
    if (e?.name === "AbortError") {
      historyLoading = false;
      if (historyAbort === ac) historyAbort = null;
      if (!searchPriming) setHistoryLoading(false, historyExhausted);
      return [];
    }
    /* history is best-effort */
  }
  if (historyAbort === ac) historyAbort = null;
  historyLoading = false;
  if (!searchPriming) setHistoryLoading(false, historyExhausted);
  return events;
}

async function loadHistory() {
  if (searchPriming) return;
  const q = $("search").value.trim();
  const batch = await fetchAndRouteHistoryPage();
  if (q && batch.length) {
    searchScanned += batch.length;
    updateSearchEmptyPrompt();
  }
  if (historyExhausted || !nearHistoryEnd()) return;
  // Under an active search the filtered feed often stays short, so nearHistoryEnd()
  // is almost always true - a microtask chain would re-scan the whole JSONL
  // (~1.5s/page) forever. Only auto-chain when unfiltered; search uses scroll
  // (one page per gesture) or the explicit "Load more history" button.
  if (q) return;
  queueMicrotask(() => maybeLoadHistory());
}

/**
 * After a URL search preset loads, pull history until the viewport is full of
 * matches or we've considered SEARCH_PRIME_LOOKBACK events from the tip.
 * Cleared search (x) cancels via cancelSearchPrime().
 */
async function runSearchPrime(snapshotCount) {
  if (searchPriming) return;
  if (!$("search").value.trim()) return;
  const gen = ++searchPrimeGen;
  searchPriming = true;
  // Snapshot is already the tip window - count it toward the lookback cap.
  let scanned = snapshotCount || 0;
  searchScanned = scanned;
  armSearchEmptyGrace();
  setSearchPrime(true);
  applyFilters();
  // Let pool ticker/name fetches land before we decide the tip has no hits
  // (otherwise ?ST3AK flashes "No results" then the match appears).
  await waitForEnrichment(gen);
  if (gen !== searchPrimeGen) return;

  // Only fetch older pages if we still have lookback budget and the filtered
  // view doesn't fill the viewport yet.
  while (
    gen === searchPrimeGen
    && $("search").value.trim()
    && scanned < SEARCH_PRIME_LOOKBACK
    && !historyExhausted
    && !visibleFeedFillsPage()
    && oldestEventId != null
  ) {
    const batch = await fetchAndRouteHistoryPage();
    if (gen !== searchPrimeGen) return;
    if (!batch.length) break;
    scanned += batch.length;
    searchScanned = scanned;
    await waitForEnrichment(gen);
    if (gen !== searchPrimeGen) return;
  }

  if (gen !== searchPrimeGen) return;
  searchScanned = scanned;
  searchPriming = false;
  setSearchPrime(false);
  applyFilters();
  updateSearchEmptyPrompt();
  // Never kick off unbounded history crawl after a search prime.
  if (!$("search").value.trim()) {
    queueMicrotask(() => maybeLoadHistory());
  }
}

function setHistoryLoading(on, exhausted = false) {
  let el = $("history-loader");
  if (!el) {
    el = document.createElement("div");
    el.id = "history-loader";
    el.className = "history-loader";
    el.setAttribute("aria-live", "polite");
    feed.appendChild(el);
  } else if (el.parentElement !== feed) {
    feed.appendChild(el);
  }
  if (on) {
    el.innerHTML = `<span class="hist-spin" aria-hidden="true"></span><span class="hist-t">Loading older events…</span>`;
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

setInterval(() => {
  const now = Date.now();
  while (eventTimes.length && eventTimes[0] < now - 180_000) eventTimes.shift();
  const perMin = eventTimes.filter((t) => t > now - 60_000).length;
  $("st-epm").textContent = fmtInt(perMin);
  $("st-act").textContent = fmtInt(eventTimes.length);

  // sparkline: 36 × 5s buckets over 3 minutes
  const buckets = new Array(36).fill(0);
  for (const t of eventTimes) {
    const i = Math.floor((now - t) / 5000);
    if (i >= 0 && i < 36) buckets[35 - i]++;
  }
  const max = Math.max(4, ...buckets);
  const W = 110, H = 34, P = 3;
  const pts = buckets.map((v, i) => [
    P + (i * (W - 2 * P)) / 35,
    H - P - (v / max) * (H - 2 * P),
  ]);
  const line = "M" + pts.map((p) => p[0].toFixed(1) + " " + p[1].toFixed(1)).join(" L");
  document.querySelector("#spark .line").setAttribute("d", line);
  document.querySelector("#spark .area").setAttribute(
    "d", line + ` L${(W - P).toFixed(1)} ${H - P} L${P} ${H - P} Z`
  );

  updateVisibleEventCount();
}, 2000);

setInterval(() => {
  document.querySelectorAll(".ev-time[data-ts]").forEach((el) => {
    el.textContent = timeAgo(Number(el.dataset.ts));
  });
}, 20_000);

/* ── Asset & pool metadata enrichment ─────────────────────────────────── */

const assetMeta = new Map(Object.entries(store.get("co_assets_v2", {})));
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
      const decimals = m.decimals == null || m.decimals === "" ? null : Number(m.decimals);
      const meta = {
        name: m.name || null,
        ticker: m.ticker || null,
        decimals: Number.isFinite(decimals) ? decimals : null,
        logo: m.logo || null,
      };
      if (meta.decimals != null) {
        assetMeta.set(unit, meta);
        persistAssetCache();
      }
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
}

const poolMeta = new Map(Object.entries(store.get("co_pools_v1", {})));
const poolWaiters = new Map(); // poolId → elements waiting on in-flight fetch

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
    if ($("search").value.trim()) scheduleFilterRefresh();
  }
  if (!ticker && !name) return; // leave truncated pool id as fallback
  el.textContent = ticker || name;
  el.classList.remove("hash");
  el.classList.add("pool-ticker");
  el.title = [name && name !== ticker ? name : null, poolId].filter(Boolean).join(" · ");
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
  mTitle.textContent = titleCaseWords(ev.title);
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
        feed.innerHTML = "";
        groups.clear();
        groupOrder.length = 0;
        oldestEventId = null;
        historyExhausted = false;
        historyLoading = false;
        searchPrimeGen++;
        if (historyAbort) {
          try { historyAbort.abort(); } catch { /* ignore */ }
          historyAbort = null;
        }
        searchPriming = false;
        setSearchPrime(false);
        hideSearchPrompts();
        searchScanned = 0;
        for (const ev of m.events || []) routeEvent(ev);
        setTip(m.tip);
        applyFilters();
        prefetchUnitsFromEvents(m.events || []);
        {
          const snapN = (m.events || []).length;
          const wantPrime = !!(urlSearchPreset && $("search").value.trim());
          queueMicrotask(() => {
            if (wantPrime) runSearchPrime(snapN);
            else maybeLoadHistory();
          });
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

/* ── Boot ─────────────────────────────────────────────────────────────── */

buildToolbar();
applyFilters();
connect();
