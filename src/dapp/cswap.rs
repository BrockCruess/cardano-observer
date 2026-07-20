//! CSWAP hybrid-AMM order + pool detection.

use super::dex::{output_datum_cbor, CborReader, CborVal, Role, WantedOut};
use serde_json::Value;

pub const NAME: &str = "CSWAP";
pub const ORDERBOOK: bool = false;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1z8d9k3aw6w24eyfjacy809h68dv2rwnpw0arrfau98jk6nhv88awp8sgxk65d6kry0mar3rd0dlkfljz7dv64eu39vfs38yd9p", Role::Swap),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[];

pub const POOL_ADDRESSES: &[&str] = &[
    "addr1z8ke0c9p89rjfwmuh98jpt8ky74uy5mffjft3zlcld9h7ml3lmln3mwk0y3zsh3gs3dzqlwa9rjzrxawkwm4udw9axhs6fuu6e",
];

pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_want(output, tx)
}
/// CSWAP order: `Constr 0 [ addr, target_assets, input_assets, otype, … ]`
/// where each target is `[policy, name, minQty]` (ADA = empty bytes).
fn parse_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    let targets = match fields.get(1)? {
        CborVal::Array(items) => items,
        _ => return None,
    };
    let mut ada_want: Option<i128> = None;
    let mut tok_want: Option<WantedOut> = None;
    for t in targets {
        let row = match t {
            CborVal::Array(items) if items.len() >= 3 => items,
            _ => continue,
        };
        let pol = hex::encode(row[0].bytes()?);
        let name = hex::encode(row[1].bytes()?);
        let qty = row[2].as_int()?;
        if qty <= 0 {
            continue;
        }
        if pol.is_empty() && name.is_empty() {
            // Skip the 2 ADA min-UTxO floor when a token target exists.
            ada_want = Some(qty);
        } else {
            tok_want = Some(WantedOut {
                qty,
                min: true,
                policy: pol,
                name_hex: name,
                resolved: true,
                lp_unit: None,
                a_to_b: None,
            });
        }
    }
    if let Some(w) = tok_want {
        return Some(w);
    }
    let qty = ada_want.filter(|&q| q > 2_000_000)?;
    Some(WantedOut {
        qty,
        min: true,
        policy: String::new(),
        name_hex: String::new(),
        resolved: true,
        lp_unit: None,
        a_to_b: None,
    })
}
