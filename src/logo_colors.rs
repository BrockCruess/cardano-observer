//! Durable per-logo colour cache.
//!
//! Samples a dark, a mid-tone, and a light accent colour from each DEX / dApp
//! brand logo so the UI can tint those event cards with a subtle three-point
//! gradient that carries real contrast (see [`pick_three`]). Colours are
//! computed once per logo on first boot and stored in `DATA_DIR/logo-colors.json`;
//! an entry already in that file is never recomputed, so the palette stays
//! stable across restarts and can be hand-edited to override an auto-pick.
//!
//! Logo bytes are read from the copies embedded in the binary (the same ones the
//! server serves), so extraction needs no filesystem access. Keys match the
//! logo's served path (e.g. `dex/logos/minswap.svg`) so the browser can look up
//! a card's colours from its logo `src`.

use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const CACHE_FILE: &str = "logo-colors.json";

type Rgb = [u8; 3];

/// A logo embedded in the binary: served-path key, raw bytes, SVG or raster.
struct Logo {
    key: &'static str,
    bytes: &'static [u8],
    svg: bool,
}

macro_rules! png_logo {
    ($key:literal, $path:literal) => {
        Logo {
            key: $key,
            bytes: include_bytes!($path),
            svg: false,
        }
    };
}
macro_rules! svg_logo {
    ($key:literal, $path:literal) => {
        Logo {
            key: $key,
            bytes: include_bytes!($path),
            svg: true,
        }
    };
}

fn logos() -> Vec<Logo> {
    #[allow(unused_mut)]
    let mut v = vec![
        svg_logo!("dex/logos/minswap.svg", "../static/dex/logos/minswap.svg"),
        png_logo!("dex/logos/sundaeswap.png", "../static/dex/logos/sundaeswap.png"),
        png_logo!("dex/logos/wingriders.png", "../static/dex/logos/wingriders.png"),
        png_logo!("dex/logos/muesliswap.png", "../static/dex/logos/muesliswap.png"),
        svg_logo!("dex/logos/splash.svg", "../static/dex/logos/splash.svg"),
        png_logo!("dex/logos/vyfinance.png", "../static/dex/logos/vyfinance.png"),
        png_logo!("dex/logos/cswap.png", "../static/dex/logos/cswap.png"),
        png_logo!("dex/logos/geniusyield.png", "../static/dex/logos/geniusyield.png"),
        png_logo!("dex/logos/chadswap.png", "../static/dex/logos/chadswap.png"),
        png_logo!("dex/logos/danofinance.png", "../static/dex/logos/danofinance.png"),
    ];
    #[cfg(has_dapp)]
    v.extend([
        png_logo!("dapp/logos/iagon.png", "../static/dapp/logos/iagon.png"),
        png_logo!("dapp/logos/indigo.png", "../static/dapp/logos/indigo.png"),
        png_logo!("dapp/logos/fluidtokens.png", "../static/dapp/logos/fluidtokens.png"),
        png_logo!("dapp/logos/liqwid.png", "../static/dapp/logos/liqwid.png"),
        svg_logo!("dapp/logos/optim.svg", "../static/dapp/logos/optim.svg"),
        png_logo!("dapp/logos/dano.png", "../static/dapp/logos/dano.png"),
        png_logo!("dapp/logos/strike.png", "../static/dapp/logos/strike.png"),
        png_logo!("dapp/logos/surf.png", "../static/dapp/logos/surf.png"),
        svg_logo!("dapp/logos/wayup.svg", "../static/dapp/logos/wayup.svg"),
    ]);
    v
}

/// Load the cache, compute colours for any logo not already present, persist if
/// anything was added, and return the full `{ key: ["#..","#..","#.."] }` map for
/// the browser.
pub fn build(data_dir: Option<&Path>) -> Value {
    let path = data_dir.map(|d| d.join(CACHE_FILE));
    let mut map: BTreeMap<String, [String; 3]> = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut added = 0usize;
    for logo in logos() {
        if map.contains_key(logo.key) {
            continue;
        }
        match extract(&logo) {
            Some(cols) => {
                map.insert(logo.key.to_string(), cols.map(hex));
                added += 1;
            }
            None => tracing::warn!("logo colors: could not extract from {}", logo.key),
        }
    }

    if added > 0 {
        if let Some(p) = &path {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(text) = serde_json::to_string_pretty(&map) {
                if let Err(e) = std::fs::write(p, text) {
                    tracing::warn!("logo colors: could not write {}: {e}", p.display());
                }
            }
            tracing::info!(
                "logo colors: computed {added} new, {} total ({})",
                map.len(),
                p.display()
            );
        } else {
            tracing::info!("logo colors: computed {added} (no DATA_DIR - not persisted)");
        }
    } else {
        tracing::info!("logo colors: {} cached", map.len());
    }
    json!(map)
}

fn extract(logo: &Logo) -> Option<[Rgb; 3]> {
    let hist = if logo.svg {
        svg_histogram(logo.bytes)
    } else {
        png_histogram(logo.bytes)
    }?;
    pick_three(hist)
}

/// Raster path: decode the PNG and bucket opaque pixels into a coarse colour
/// histogram (16-per-channel) so near-identical shades merge.
fn png_histogram(bytes: &[u8]) -> Option<Vec<(Rgb, u32)>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let mut counts: HashMap<Rgb, u32> = HashMap::new();
    for px in img.pixels() {
        let [r, g, b, a] = px.0;
        if a < 128 {
            continue;
        }
        // Quantize to the centre of each 16-wide bucket.
        let q = [(r & 0xF0) | 0x08, (g & 0xF0) | 0x08, (b & 0xF0) | 0x08];
        *counts.entry(q).or_default() += 1;
    }
    (!counts.is_empty()).then(|| counts.into_iter().collect())
}

/// Vector path: collect the explicit colours declared in the SVG markup
/// (`#rgb`, `#rrggbb`, and `rgb(r,g,b)`), weighted by how often each appears.
fn svg_histogram(bytes: &[u8]) -> Option<Vec<(Rgb, u32)>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut counts: HashMap<Rgb, u32> = HashMap::new();
    let chars: Vec<char> = text.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let hexs: String = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if let Some(rgb) = parse_hex(&hexs) {
                *counts.entry(rgb).or_default() += 1;
            }
            i += 1 + hexs.len();
            continue;
        }
        i += 1;
    }
    // rgb( … ) function notation.
    for chunk in text.split("rgb(").skip(1) {
        if let Some(end) = chunk.find(')') {
            if let Some(rgb) = parse_rgb_fn(&chunk[..end]) {
                *counts.entry(rgb).or_default() += 1;
            }
        }
    }

    (!counts.is_empty()).then(|| counts.into_iter().collect())
}

fn parse_hex(h: &str) -> Option<Rgb> {
    let expand = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        6 => Some([expand(&h[0..2])?, expand(&h[2..4])?, expand(&h[4..6])?]),
        3 => {
            let d = |c: &str| expand(&format!("{c}{c}"));
            Some([d(&h[0..1])?, d(&h[1..2])?, d(&h[2..3])?])
        }
        _ => None,
    }
}

fn parse_rgb_fn(s: &str) -> Option<Rgb> {
    let parts: Vec<u8> = s
        .split(',')
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 255.0) as u8)
        .collect();
    (parts.len() >= 3).then(|| [parts[0], parts[1], parts[2]])
}

/// Luminance each gradient stop aims for: a dark, a mid-tone, and a light stop.
/// Kept away from pure 0 / 1 so derived stops stay non-black / non-white.
const TARGET_L: [f64; 3] = [0.16, 0.50, 0.84];

/// Choose one dark, one mid-tone, and one light colour so the gradient carries
/// real contrast while still reading as the brand.
///
/// 1. Keep the best-represented colours (drop the long anti-alias tail).
/// 2. Anchor the brand's **primary** - its most frequent, most vivid colour -
///    into whichever band (dark / mid / light) its own lightness falls in, so
///    the true brand colour is always one of the three stops.
/// 3. Fill the remaining bands from the logo's own colours, each scored by
///    representation × closeness to that band's target lightness, and biased
///    away from near-black / near-white. When the logo has no colour in a band
///    at all, derive that stop by lightening or darkening the primary.
/// 4. "Non-black before black, non-white before white": if a dark/light stop
///    ended up as flat black/white but the brand has a real hue, derive a
///    coloured extreme from the primary instead (monochrome logos keep greys).
/// 5. Guarantee the three are visibly distinct, then order them light→dark.
fn pick_three(hist: Vec<(Rgb, u32)>) -> Option<[Rgb; 3]> {
    if hist.is_empty() {
        return None;
    }
    // Keep the most-present colours; the long tail is mostly anti-alias specks.
    let mut cand = hist;
    cand.sort_by(|a, b| b.1.cmp(&a.1));
    cand.truncate(64);

    // How strongly a colour represents the brand within a band: frequent and
    // vivid both help, but a less-saturated tone still counts if it is present.
    let rep = |c: Rgb, w: u32| w as f64 * (0.30 + 0.70 * saturation(c));
    // The brand's primary is its dominant *colour* - so saturation is weighted
    // hard here, keeping a large black/white background from winning. Falls back
    // to the most frequent tone for a genuinely monochrome logo (all sat ~0).
    let primary = cand
        .iter()
        .copied()
        .max_by(|a, b| {
            let s = |c: Rgb, w: u32| w as f64 * saturation(c).powf(1.3);
            s(a.0, a.1).total_cmp(&s(b.0, b.1))
        })
        .map(|(c, _)| c)?;

    // Which band a lightness belongs to (0 = dark, 1 = mid, 2 = light).
    let slot_of = |l: f64| {
        if l < 0.40 {
            0
        } else if l > 0.60 {
            2
        } else {
            1
        }
    };
    // A colour may fill a slot only if its lightness is within the band (loose
    // enough to catch near-boundary brand colours, strict enough to keep
    // contrast; otherwise the slot is derived from the primary instead).
    let accepts = |slot: usize, l: f64| match slot {
        0 => l < 0.45,
        1 => (0.28..=0.72).contains(&l),
        _ => l > 0.55,
    };

    let mut chosen: [Option<Rgb>; 3] = [None; 3];
    chosen[slot_of(luminance(primary))] = Some(primary);

    for slot in 0..3 {
        if chosen[slot].is_some() {
            continue;
        }
        let target = TARGET_L[slot];
        let taken: Vec<Rgb> = chosen.iter().flatten().copied().collect();
        let best = cand
            .iter()
            .copied()
            .filter(|(c, _)| accepts(slot, luminance(*c)))
            .filter(|(c, _)| taken.iter().all(|t| dist_sq(*t, *c) >= 40.0 * 40.0))
            .map(|(c, w)| {
                let l = luminance(c);
                // Prefer colours near the band's target lightness…
                let lum_w = (-((l - target).powi(2)) / (2.0 * 0.20 * 0.20)).exp();
                // …and away from pure black / white (used only as a last resort).
                let extreme = if !(0.05..=0.95).contains(&l) { 0.35 } else { 1.0 };
                (c, rep(c, w) * lum_w * extreme)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(c, _)| c);
        chosen[slot] = Some(best.unwrap_or_else(|| derive_at(primary, target)));
    }

    // Still in slot order here: [dark, mid, light].
    let mut out = [chosen[0]?, chosen[1]?, chosen[2]?];

    // "Non-black before black, non-white before white": when a logo's only dark
    // (or light) tone is flat black (or white) but the brand has a real hue,
    // derive a coloured extreme from the primary instead. A genuinely monochrome
    // logo keeps its greys.
    if saturation(primary) > 0.20 {
        if luminance(out[0]) < 0.08 && saturation(out[0]) < 0.25 {
            out[0] = derive_at(primary, TARGET_L[0]);
        }
        if luminance(out[2]) > 0.94 && saturation(out[2]) < 0.15 {
            out[2] = derive_at(primary, TARGET_L[2]);
        }
    }

    ensure_distinct(&mut out, primary);
    // Order as a light→dark ramp so the gradient flows.
    out.sort_by(|a, b| luminance(*b).total_cmp(&luminance(*a)));
    Some(out)
}

/// Adjust `base` toward white or black to hit `target` luminance while keeping
/// its hue. `luminance` is linear in the raw channels, so the mix ratio is exact.
fn derive_at(base: Rgb, target: f64) -> Rgb {
    let l = luminance(base);
    if (l - target).abs() < 0.02 {
        return base;
    }
    if target > l {
        mix(base, [255, 255, 255], (target - l) / (1.0 - l))
    } else {
        mix(base, [0, 0, 0], 1.0 - target / l.max(1e-6))
    }
}

/// Ensure the three stops are visibly different; re-derive a clashing stop
/// (never the primary) from the primary at its band's target lightness.
fn ensure_distinct(out: &mut [Rgb; 3], primary: Rgb) {
    const MIN_DIST_SQ: f64 = 30.0 * 30.0;
    for i in 0..3 {
        for j in (i + 1)..3 {
            if dist_sq(out[i], out[j]) < MIN_DIST_SQ {
                let k = if out[j] == primary { i } else { j };
                out[k] = derive_at(primary, TARGET_L[k]);
            }
        }
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn dist_sq(a: Rgb, b: Rgb) -> f64 {
    (0..3)
        .map(|i| {
            let d = a[i] as f64 - b[i] as f64;
            d * d
        })
        .sum()
}

fn luminance(c: Rgb) -> f64 {
    (0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64) / 255.0
}

/// HSV saturation (0 = grey, 1 = fully saturated).
fn saturation(c: Rgb) -> f64 {
    let (mx, mn) = c.iter().fold((0u8, 255u8), |(mx, mn), &v| (mx.max(v), mn.min(v)));
    if mx == 0 {
        0.0
    } else {
        (mx - mn) as f64 / mx as f64
    }
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 * (1.0 - t) + b[0] as f64 * t).round() as u8,
        (a[1] as f64 * (1.0 - t) + b[1] as f64 * t).round() as u8,
        (a[2] as f64 * (1.0 - t) + b[2] as f64 * t).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(parse_hex("ff8800"), Some([0xff, 0x88, 0x00]));
        assert_eq!(parse_hex("f80"), Some([0xff, 0x88, 0x00]));
        assert_eq!(parse_hex("xyz"), None);
    }

    #[test]
    fn svg_colours_extracted() {
        let svg = br##"<svg><path fill="#e0195a"/><stop stop-color="#12ab34"/>
            <rect style="fill:rgb(10, 20, 250)"/></svg>"##;
        let hist = svg_histogram(svg).expect("hist");
        let three = pick_three(hist).expect("three");
        assert_banded(&three);
    }

    /// A dark, a mid, and a light stop with room between them, ordered light→dark.
    fn assert_banded(three: &[Rgb; 3]) {
        assert!(luminance(three[0]) >= luminance(three[1]));
        assert!(luminance(three[1]) >= luminance(three[2]));
        assert!(luminance(three[0]) > 0.55, "no light stop: {three:?}");
        assert!(luminance(three[2]) < 0.45, "no dark stop: {three:?}");
        assert!(dist_sq(three[0], three[2]) >= 40.0 * 40.0, "stops too close");
    }

    #[test]
    fn single_colour_becomes_banded_ramp() {
        // One mid-red logo → light-red, red, dark-red (non-white / non-black).
        let three = pick_three(vec![([200, 30, 30], 100)]).expect("three");
        assert_banded(&three);
        assert!(three[0] != [255, 255, 255], "light stop should not be pure white");
        assert!(three[2] != [0, 0, 0], "dark stop should not be pure black");
    }

    #[test]
    fn prefers_brand_dark_over_black() {
        // Dark navy present alongside black: the dark stop should be the navy.
        let three = pick_three(vec![
            ([0, 0, 0], 200),
            ([20, 30, 90], 120),
            ([120, 170, 255], 80),
        ])
        .expect("three");
        assert_banded(&three);
        let dark = three[2];
        assert!(dark != [0, 0, 0], "dark stop resorted to black: {dark:?}");
    }

    #[test]
    fn deterministic() {
        let h = vec![([200, 30, 30], 50), ([30, 60, 200], 40), ([230, 220, 40], 30)];
        assert_eq!(pick_three(h.clone()), pick_three(h));
    }

    #[test]
    fn extracts_all_embedded_logos() {
        let all = logos();
        assert!(!all.is_empty());
        for logo in &all {
            let cols = extract(logo).unwrap_or_else(|| panic!("no colours for {}", logo.key));
            let hexes = cols.map(hex);
            for c in &hexes {
                assert!(c.len() == 7 && c.starts_with('#'), "bad hex {c}");
            }
            // Print for manual eyeballing under `--nocapture`.
            println!("{:32} {} {} {}", logo.key, hexes[0], hexes[1], hexes[2]);
        }
    }
}
