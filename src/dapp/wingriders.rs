//! WingRiders V1 / V2 order + pool detection.

use super::dex::{
    option_int, output_datum_cbor, AssetRef, CborReader, CborVal, Role, WantedOut,
};
use serde_json::Value;

pub const NAME: &str = "WingRiders";
pub const ORDERBOOK: bool = false;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[
    ("86ae9eebd8b97944a45201e4aec1330a72291af2d071644bba015959", Role::Swap), // V1
    ("c5e0385012d5f010b1dc7ab42ba632944052de232051ec6ce3bfd72e", Role::Swap), // V1
    ("c134d839a64a5dfb9b155869ef3f34280751a622f69958baa8ffd29c", Role::Swap), // V2
    ("23680ea6701b56f2c12ae79d8af94fd36f509b7b007029c7ce114840", Role::Swap), // V2
];

pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];

pub const POOL_NFT_POLICIES: &[&str] = &[
    "026a18d04a0c642759bb3d83b12e3344894e5c1c7b2aeb1a2113a570", // V1
    "6fdc63a1d71dc2c65502b79baae7fb543185702b12c3c5fb639ed737", // V2
];

pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_want(output, tx)
}
/// WingRiders V1/V2: datum carries the **pool pair** (A, B) + min receive —
/// not offer/ask. Ask = the pool side that is *not* sitting in the order UTxO.
/// V2 flat layout: `[deposit, dest…, expiration, APol, AName, BPol, BName, minConstr, …]`
/// V1 nested layout: `[… poolPair [A,B] …, minConstr]`.
fn parse_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;

    // V2: pool A/B are consecutive byte pairs after the deadline int.
    if fields.len() >= 11 {
        if let (Some(a_pol), Some(a_name), Some(b_pol), Some(b_name), Some(min_c)) = (
            fields.get(6).and_then(CborVal::bytes),
            fields.get(7).and_then(CborVal::bytes),
            fields.get(8).and_then(CborVal::bytes),
            fields.get(9).and_then(CborVal::bytes),
            fields.get(10),
        ) {
            let a = AssetRef {
                policy: hex::encode(a_pol),
                name_hex: hex::encode(a_name),
            };
            let b = AssetRef {
                policy: hex::encode(b_pol),
                name_hex: hex::encode(b_name),
            };
            if a != b {
                if let Some(qty) = extract_min_qty(min_c) {
                    if let Some(ask) = wr_ask_from_pair(output, &a, &b)
                        .or_else(|| wr_ask_from_direction(min_c, &a, &b))
                    {
                        return Some(WantedOut {
                            qty,
                            min: true,
                            policy: ask.policy,
                            name_hex: ask.name_hex,
                            resolved: true,
                            lp_unit: None,
                            a_to_b: None,
                        });
                    }
                }
            }
        }
    }

    // Nested V1 shape: pool pair + min qty.
    if fields.len() >= 2 {
        if let (Some((a, b)), Some(qty)) = (
            fields.first().and_then(find_wr_asset_pair),
            fields.get(1).and_then(extract_min_qty),
        ) {
            let ask = wr_ask_from_pair(output, &a, &b)
                .or_else(|| wr_ask_from_direction(fields.get(1)?, &a, &b))?;
            return Some(WantedOut {
                qty,
                min: true,
                policy: ask.policy,
                name_hex: ask.name_hex,
                resolved: true,
                lp_unit: None,
                a_to_b: None,
            });
        }
    }
    None
}

/// Prefer the pool-pair side that is *not* deposited in the order UTxO.
pub(super) fn wr_ask_from_pair(output: &Value, a: &AssetRef, b: &AssetRef) -> Option<AssetRef> {
    let mut assets: Vec<(String, String, i128)> = Vec::new();
    crate::parse::collect_assets(output.get("value"), &mut assets);
    for (pol, name, _) in &assets {
        let dep = AssetRef {
            policy: pol.clone(),
            name_hex: name.clone(),
        };
        if dep == *a {
            return Some(b.clone());
        }
        if dep == *b {
            return Some(a.clone());
        }
    }
    // ADA-only order → buying the non-ADA side of the pair.
    if assets.is_empty() {
        if a.policy.is_empty() && a.name_hex.is_empty() {
            return Some(b.clone());
        }
        if b.policy.is_empty() && b.name_hex.is_empty() {
            return Some(a.clone());
        }
    }
    None
}

/// `Constr0 [ Constr N [], qty ]`: N=0 → want B, N=1 → want A (WingRiders pool order).
fn wr_ask_from_direction(min_c: &CborVal, a: &AssetRef, b: &AssetRef) -> Option<AssetRef> {
    let fields = min_c.constr_fields(0)?;
    let (tag, _) = fields.first()?.as_constr()?;
    match tag {
        0 => Some(b.clone()),
        1 => Some(a.clone()),
        _ => None,
    }
}

fn wr_asset_ident(v: &CborVal) -> Option<AssetRef> {
    // Constr 0 [ policy_bytes, name_bytes ] or Constr 0 [ Constr 0 [p,n] ]
    if let Some(fields) = v.constr_fields(0) {
        if fields.len() >= 2 {
            if let (Some(p), Some(n)) = (fields[0].bytes(), fields[1].bytes()) {
                return Some(AssetRef {
                    policy: hex::encode(p),
                    name_hex: hex::encode(n),
                });
            }
        }
        if let Some(inner) = fields.first() {
            return wr_asset_ident(inner);
        }
    }
    None
}

fn find_wr_asset_pair(v: &CborVal) -> Option<(AssetRef, AssetRef)> {
    if let Some(fields) = v.constr_fields(0) {
        if fields.len() == 2 {
            if let (Some(a), Some(b)) = (wr_asset_ident(&fields[0]), wr_asset_ident(&fields[1])) {
                return Some((a, b));
            }
        }
        for f in fields.iter().rev() {
            if let Some(p) = find_wr_asset_pair(f) {
                return Some(p);
            }
        }
    }
    if let CborVal::Array(items) = v {
        for i in items.iter().rev() {
            if let Some(p) = find_wr_asset_pair(i) {
                return Some(p);
            }
        }
    }
    None
}

fn extract_min_qty(v: &CborVal) -> Option<i128> {
    if let Some(n) = option_int(v) {
        return Some(n);
    }
    if let Some(n) = v.as_int() {
        return Some(n);
    }
    if let Some((_, fields)) = v.as_constr() {
        for f in fields.iter().rev() {
            if let Some(n) = extract_min_qty(f) {
                return Some(n);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wingriders_v2_sell_asks_other_pool_side() {
        // Live USDA→ADA order (tx c945fbc9…): pool pair is ADA/USDA, deposit USDA.
        let datum = "d8799f1a001e8480d8799fd8799f581c05ce3b4bf707a1b8002b1421f4dff4121e7ef44e18a0c69620ac2043ffd8799fd8799fd8799f581c74a37b4e9448b2977f2e784a56285c0c726005032b6968791e2929b2ffffffffd8799fd8799f581c05ce3b4bf707a1b8002b1421f4dff4121e7ef44e18a0c69620ac2043ffd8799fd8799fd8799f581c74a37b4e9448b2977f2e784a56285c0c726005032b6968791e2929b2ffffffff80d879801b0000019f65c5feb94040581cfe7c786ab321f41c654ef6c1af7b3250a613c24e4213e0425a7ae4564455534441d8799fd87a801a3b9aca00ff0101ff";
        let output = json!({
            "address": "addr1wxc134d839a64a5dfb9b155869ef3f34280751a622f69958baa8ffd29c",
            "value": {
                "ada": { "lovelace": 4_000_000 },
                "fe7c786ab321f41c654ef6c1af7b3250a613c24e4213e0425a7ae456": { "55534441": 166_010_584 }
            },
            "datum": datum,
        });
        let want = parse_want(&output, &json!({})).expect("want");
        assert!(want.resolved);
        assert!(want.policy.is_empty() && want.name_hex.is_empty(), "should want ADA");
        assert_eq!(want.qty, 1_000_000_000);
    }
}
