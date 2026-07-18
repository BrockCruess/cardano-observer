/**
 * Optional dApp UI pack.
 *
 * Linked from the core frontend via dynamic `import("/dapp/mod.js")`.
 * Keep `DAPP_APPS` in sync with scanners registered in `src/dapp/mod.rs`.
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
