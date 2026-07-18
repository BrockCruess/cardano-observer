/**
 * DEX UI helpers.
 *
 * Keep `DEX_VENUES` in sync with scanners in `src/dex.rs`.
 * Logos in `/dex/logos/` — Cardano.org app icons + official site marks.
 * `badge: true` paints `plate` on the icon frame behind the unchanged artwork.
 */

/** DEX venues emitted as `data.dex`. */
export const DEX_VENUES = [
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

/**
 * @type {Record<string, { src: string, badge?: boolean, plate?: string }>}
 */
const DEX_LOGOS = {
  Minswap: { src: "/dex/logos/minswap.svg?v=1" },
  SundaeSwap: {
    src: "/dex/logos/sundaeswap.png?v=1",
    badge: true,
    plate: "#f5c5ab",
  },
  WingRiders: {
    src: "/dex/logos/wingriders.png?v=1",
    badge: true,
    plate: "#000000",
  },
  MuesliSwap: {
    src: "/dex/logos/muesliswap.png?v=1",
    badge: true,
    plate: "#262234",
  },
  Splash: { src: "/dex/logos/splash.svg?v=1" },
  VyFinance: {
    src: "/dex/logos/vyfinance.png?v=1",
    badge: true,
    plate: "#000000",
  },
  CSWAP: {
    src: "/dex/logos/cswap.png?v=1",
    badge: true,
    plate: "#000000",
  },
  GeniusYield: {
    src: "/dex/logos/geniusyield.png?v=1",
    badge: true,
    plate: "#50f0be",
  },
  ChadSwap: {
    src: "/dex/logos/chadswap.png?v=1",
    badge: true,
    plate: "#2d302d",
  },
  "Dano Finance": {
    src: "/dex/logos/danofinance.png?v=1",
    badge: true,
    plate: "#000000",
  },
};

/**
 * Brand icon for a DEX event card, or null when unknown/missing.
 * @param {string|undefined|null} dexName `data.dex`
 * @returns {{ html: string, badge: boolean, plate: string }|null}
 */
export function dexIconHtml(dexName) {
  const logo = DEX_LOGOS[String(dexName || "")];
  if (!logo) return null;
  const cls = logo.badge ? "ev-logo ev-logo-badge" : "ev-logo";
  const name = escAttr(String(dexName));
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
