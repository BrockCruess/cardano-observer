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
  "Strike",
  "Surf",
  "Wayup",
];

/**
 * Brand marks. `badge` logos are circular plates — we paint `plate` on the
 * icon frame so the brand color fills the rounded square behind the
 * unchanged logo artwork. `?v=` busts long-lived browser image caches.
 * @type {Record<string, { src: string, badge?: boolean, plate?: string }>}
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
  const cls = logo.badge ? "ev-logo ev-logo-badge" : "ev-logo";
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
  // Surf/Indigo-style loan cards: `assets`/`ada` = principal, `collateral` = vault.
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
  // Indigo: one stability pool per iAsset — ticker is the pool id.
  const iassetPool = d.iasset
    ? `<span class="hash" title="Stability pool">${esc(d.iasset)} pool</span>`
    : "";
  const ada = d.ada
    ? principalLabel
      ? `<b>${fmtAda(d.ada)}</b> ${principalLabel}`
      : `<b>${fmtAda(d.ada)}</b>`
    : "";
  return (
    sub([
      `<span class="badge contract">${esc(d.dapp || "dApp")}</span>`,
      nodeId,
      iassetPool,
      iag,
      indy,
      fldt,
      ada,
      actorSpan(d),
    ]) +
    chips +
    collateral
  );
}
