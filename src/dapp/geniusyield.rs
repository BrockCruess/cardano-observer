//! Genius Yield partial-order book detection.
//!
//! Order-book venue: settlement is peer-to-peer, so a spend of a tracked
//! order counts as a fill rather than a cancel (see `ORDERBOOK`).

use super::dex::{output_datum_cbor, CborReader, Role, WantedOut};
use serde_json::Value;

pub const NAME: &str = "GeniusYield";
pub const ORDERBOOK: bool = true;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1wx5d0l6u7nq3wfcz3qmjlxkgu889kav2u9d8s5wyzes6frqktgru2", Role::Swap),
    ("addr1w8kllanr6dlut7t480zzytsd52l7pz4y3kcgxlfvx2ddavcshakwd", Role::Swap),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[];
pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_want(output, tx)
}
/// Genius Yield partial order: out asset + price × leftover offer ≈ min receive.
fn parse_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    if fields.len() < 7 {
        return None;
    }
    let leftover = fields.get(4)?.as_int()?;
    let out = fields.get(5)?.constr_fields(0)?;
    if out.len() < 2 {
        return None;
    }
    let policy = hex::encode(out[0].bytes()?);
    let name_hex = hex::encode(out[1].bytes()?);
    let price = fields.get(6)?.constr_fields(0)?;
    let denom = price.first()?.as_int()?;
    let num = price.get(1)?.as_int()?;
    if leftover <= 0 || denom <= 0 || num <= 0 {
        return None;
    }
    let qty = leftover.saturating_mul(num) / denom;
    if qty <= 0 {
        return None;
    }
    Some(WantedOut {
        qty,
        min: true,
        policy,
        name_hex,
        resolved: true,
        lp_unit: None,
        a_to_b: None,
    })
}
