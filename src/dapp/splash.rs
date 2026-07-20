//! Splash / Spectrum swap + liquidity detection.

use super::dex::{output_datum_cbor, CborReader, Role, WantedOut};
use serde_json::Value;

pub const NAME: &str = "Splash";
pub const ORDERBOOK: bool = false;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1wynp362vmvr8jtc946d3a3utqgclfdl5y9d3kn849e359hsskr20n", Role::Swap),
    ("addr1wyr4uz0tp75fu8wrg6gm83t20aphuc9vt6n8kvu09ctkugqpsrmeh", Role::Liquidity),
    ("addr1wxpa5704x8qel88ympf4natfdzn59nc9esj7609y3sczmmsasees8", Role::Liquidity),
    ("addr1w95q755yrsr0xt8vmn007tpqee4hps49yjdef5dzknhl99qntsmh0", Role::Liquidity),
    ("addr1wymhr2l96gm22xkwz0rn3zz79xz9l400nm5sa580kssdyagr5z7wq", Role::Liquidity),
    ("addr1wxrl2p9s0tweu8t54cgz75at070ly3tda6yh5s7cufanfzc52gv39", Role::Liquidity),
    ("addr1wxu29wa80fd4ptpfwqe20vpxrum45f57ud3r6egh9vuyhfc2a3jhj", Role::Liquidity),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[
    ("2025463437ee5d64e89814a66ce7f98cb184a66ae85a2fbbfd750106", Role::Swap),
    ("464eeee89f05aff787d40045af2a40a83fd96c513197d32fbc54ff02", Role::Swap),
];

pub const POOL_ADDRESSES: &[&str] = &[
    "addr1x8nz307k3sr60gu0e47cmajssy4fmld7u493a4xztjrll0aj764lvrxdayh2ux30fl0ktuh27csgmpevdu89jlxppvrswgxsta", // Spectrum pool V1
    "addr1x94ec3t25egvhqy2n265xfhq882jxhkknurfe9ny4rl9k6dj764lvrxdayh2ux30fl0ktuh27csgmpevdu89jlxppvrst84slu", // Spectrum pool V2
];

pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_want(output, tx)
}
/// Splash / Spectrum swap order:
/// `Constr 0 [ …, baseAsset, baseAmt, _, quoteAmt, quoteAsset, price, … ]`
/// where each asset is `Constr 0 [ policy, name ]` (ADA = empty bytes).
fn parse_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    if fields.len() < 7 {
        return None;
    }
    let qty = fields.get(5)?.as_int()?;
    if qty <= 0 {
        return None;
    }
    let quote = fields.get(6)?.constr_fields(0)?;
    if quote.len() < 2 {
        return None;
    }
    let policy = hex::encode(quote[0].bytes()?);
    let name_hex = hex::encode(quote[1].bytes()?);
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
