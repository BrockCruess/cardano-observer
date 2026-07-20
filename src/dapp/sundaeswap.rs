//! SundaeSwap V1 / V3 order + pool detection.
//!
//! V3 uses an offer/ask datum shared with other "scoop" style DEXes, so the
//! generic [`super::dex::parse_offer_ask_want`] is tried as a fallback.

use super::dex::{
    offer_ask_from_val, option_int, output_datum_cbor, parse_offer_ask_want, CborReader, Role,
    WantedOut,
};
use serde_json::Value;

pub const NAME: &str = "SundaeSwap";
pub const ORDERBOOK: bool = false;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1wxaptpmxcxawvr3pzlhgnpmzz3ql43n2tc8mn3av5kx0yzs09tqh8", Role::Swap), // V1
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[
    ("fa6a58bbe2d0ff05534431c8e2f0ef2cbdc1602a8456e4b13c8f3077", Role::Swap), // V3
];

pub const POOL_ADDRESSES: &[&str] =
    &["addr1w9qzpelu9hn45pefc0xr4ac4kdxeswq7pndul2vuj59u8tqaxdznu"]; // V1

pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];

/// V3 shares one policy between pool NFTs (CIP-67 label `000de140`) and LP
/// tokens (`0014df1…`), so only the pool-NFT prefix identifies a pool.
pub const POOL_NFT_PREFIXES: &[(&str, &str)] =
    &[("e0302560ced2fdcbfcb2602697df970cd0d6a38f94b32703f51c312b", "000de140")];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_want(output, tx).or_else(|| parse_offer_ask_want(output, tx))
}
/// Sundae V1 swap: `[poolId, dest…, fee, Swap{direction, amount, Some min}]`.
/// Asset A is ADA - `direction=true` (B→A) means selling a token for ADA.
fn parse_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    // Prefer offer/ask (V3) when present.
    if let Some(w) = offer_ask_from_val(&root) {
        return Some(w);
    }
    let fields = root.constr_fields(0)?;
    for f in fields.iter().rev() {
        let Some(sf) = f.constr_fields(0) else { continue };
        if sf.len() < 3 {
            continue;
        }
        let Some(b_to_a) = sf[0].as_bool() else { continue };
        if sf[1].as_int().is_none() {
            continue;
        }
        let Some(min_qty) = option_int(&sf[2]) else { continue };
        // B→A: want ADA. A→B: want token qty only (pool id would be needed for the asset).
        return Some(WantedOut {
            qty: min_qty,
            min: true,
            policy: String::new(),
            name_hex: String::new(),
            resolved: b_to_a, // only ADA is known without a pool lookup
            lp_unit: None,
            a_to_b: None,
        });
    }
    None
}
