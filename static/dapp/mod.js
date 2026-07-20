/**
 * Optional dApp UI pack.
 *
 * Linked from the core frontend via dynamic `import("/dapp/mod.js")`.
 * Keep `DAPP_APPS` in sync with scanners registered in `src/dapp/mod.rs`.
 *
 * Logos in `/dapp/logos/` (sourced from each project's site / Cardano.org
 * app-icon submissions); `badge: true` = solid-plate mark that fills the
 * rounded icon frame (not a circle floating inside it).
 */

/** dApps emitted as `data.dapp`. */
export const DAPP_APPS = [
  "Iagon",
  "Indigo Protocol",
  "FluidTokens",
  "Liqwid",
  "Optim Finance",
  "Dano Finance",
  "Strike",
  "Surf",
  "Wayup",
];

/**
 * Brand marks. `badge` logos are circular plates — we paint `plate` on the
 * icon frame so the brand color fills the rounded square behind the
 * unchanged logo artwork. `?v=` busts long-lived browser image caches.
 * @type {Record<string, { src: string, badge?: boolean, plate?: string, inset?: boolean }>}
 */
const DAPP_LOGOS = {
  Iagon: { src: "/dapp/logos/iagon.png?v=2" },
  "Indigo Protocol": {
    src: "/dapp/logos/indigo.png?v=2",
    badge: true,
    plate: "#4805bb",
  },
  FluidTokens: {
    src: "/dapp/logos/fluidtokens.png?v=2",
    badge: true,
    plate: "#ffffff",
  },
  Liqwid: {
    src: "/dapp/logos/liqwid.png?v=2",
    badge: true,
    plate: "#0389d2",
    // Artwork is edge-to-edge; inset matches other badge marks' built-in padding.
    inset: true,
  },
  "Optim Finance": {
    src: "/dapp/logos/optim.svg?v=2",
    badge: true,
    plate: "#15102E",
    inset: true,
  },
  "Dano Finance": {
    src: "/dapp/logos/dano.png?v=2",
    badge: true,
    plate: "#0a0a0a",
  },
  Strike: {
    src: "/dapp/logos/strike.png?v=2",
    badge: true,
    plate: "#21f9b1",
  },
  Surf: {
    src: "/dapp/logos/surf.png?v=2",
    badge: true,
    plate: "#0e1629",
  },
  Wayup: { src: "/dapp/logos/wayup.svg?v=2" },
};

/**
 * Brand icon for a dApp event card, or null when unknown/missing.
 * @param {string|undefined|null} dappName `data.dapp`
 * @returns {{ html: string, badge: boolean, plate: string }|null}
 */
export function dappIconHtml(dappName) {
  const logo = DAPP_LOGOS[String(dappName || "")];
  if (!logo) return null;
  const cls = [
    "ev-logo",
    logo.badge ? "ev-logo-badge" : "",
    logo.inset ? "ev-logo-inset" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const name = escAttr(String(dappName));
  return {
    html:
      `<img class="${cls}" src="${logo.src}" alt="${name}" ` +
      `title="${name}" decoding="async">`,
    badge: !!logo.badge,
    plate: logo.plate || "",
  };
}

function escAttr(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;");
}

/**
 * Card body for `kind: "dapp_activity"`.
 * @param {object} d event.data
 * @param {{ esc: Function, fmtAda: Function, fmtTokenQty: Function, assetChipsHtml: Function, actorSpan: Function, sub: Function }} ui
 */
export function renderDappActivityHtml(d, ui) {
  const { esc, fmtAda, fmtTokenQty, assetChipsHtml, actorSpan, sub } = ui;
  // Surf-style loan cards: `assets`/`ada` = principal, `collateral` = vault.
  const principalLabel =
    d.eventType === "borrow" ? "borrowed" : d.eventType === "repay" ? "repaid" : "";
  const chips = d.assets
    ? assetChipsHtml(
        d.assets,
        principalLabel ? { text: principalLabel, cls: "plus" } : null,
      )
    : "";
  const collateral = d.collateral
    ? assetChipsHtml(d.collateral, { text: "collateral" })
    : "";
  // Fallback text amounts when no asset chips (Iagon IAG path, ADA-only).
  const iag =
    !chips && d.iag != null ? `<b>${fmtTokenQty(d.iag, 6)}</b> IAG` : "";
  const indy =
    !chips && d.indy != null ? `<b>${fmtTokenQty(d.indy, 6)}</b> INDY` : "";
  const fldt =
    !chips && d.fldt != null ? `<b>${fmtTokenQty(d.fldt, 6)}</b> FLDT` : "";
  const nodeId = d.nodeId
    ? `node id <span class="hash" title="Node ID">${esc(d.nodeId)}</span>`
    : "";
  // Optional pool/market id when stamped by the detector.
  const iassetPool = d.iasset
    ? `<span class="hash" title="Stability pool">${esc(d.iasset)} pool</span>`
    : "";
  const market = d.market
    ? `<span class="hash" title="Market">${esc(d.market)} market</span>`
    : "";
  const qtok =
    !chips && d.qToken != null
      ? `<b>${fmtTokenQty(d.qToken, d.decimals ?? 6)}</b> ${esc(d.qTicker || `q${d.market || ""}`)}`
      : "";
  const ada = d.ada
    ? principalLabel
      ? `<b>${fmtAda(d.ada)}</b> ${principalLabel}`
      : `<b>${fmtAda(d.ada)}</b>`
    : "";
  return (
    sub([
      // No app-name pill: every detector title already ends in the app name
      // ("Open Loan - Surf"), and DEX cards in the same Finance category show
      // no venue pill either. Keep titles self-identifying.
      nodeId,
      iassetPool,
      market,
      iag,
      indy,
      fldt,
      qtok,
      ada,
      actorSpan(d),
    ]) +
    chips +
    collateral
  );
}

/**
 * Apps the UI files under the **Finance** category alongside the DEX venues,
 * so a protocol that both trades and lends shows one filter chip instead of
 * two. Mirrors `model::FINANCE_APPS` on the server — keep the two in sync.
 * Anything here but not in `DAPP_APPS` is ignored; the rest (Iagon, Wayup)
 * keeps the separate dApp chip.
 */
export const FINANCE_APPS = [
  "Dano Finance",
  "FluidTokens",
  "Indigo Protocol",
  "Liqwid",
  "Optim Finance",
  "Strike",
  "Surf",
];
