//! VyFinance order detection.
//!
//! VyFi derives one order address per pool, so the address set can't be a
//! constant: it is fetched from the official VyFi API at startup and
//! refreshed periodically (see [`OrderAddrs`]).

use super::dex::{output_datum_cbor, AssetRef, CborReader, PoolPair, Role, WantedOut};
use super::wingriders::wr_ask_from_pair;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;

pub const NAME: &str = "VyFinance";
pub const ORDERBOOK: bool = false;

// Order addresses are dynamic - see `OrderAddrs`.
pub const ORDER_ADDRESSES: &[(&str, Role)] = &[];
pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[];
pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

/// Full bech32 order address → pool pair, refreshed from the VyFi API.
#[derive(Default)]
pub struct OrderAddrs {
    map: RwLock<HashMap<String, PoolPair>>,
}

impl OrderAddrs {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when this address is a known VyFi per-pool order contract.
    pub fn contains(&self, addr: &str) -> bool {
        self.map.read().unwrap().contains_key(addr)
    }

    /// The datum has min-receive qty + an action tag; the out asset comes
    /// from the order-address → pool-pair map.
    pub fn want(&self, output: &Value, tx: &Value) -> Option<WantedOut> {
        let (tag, qty) = parse_qty(output, tx)?;
        if tag == 4 {
            return Some(WantedOut::ada(qty, true));
        }
        let addr = output.get("address").and_then(Value::as_str)?;
        let pair = self.map.read().unwrap().get(addr).cloned()?;
        let ask = wr_ask_from_pair(output, &pair.asset_a, &pair.asset_b)?;
        Some(WantedOut::from_ref(qty, true, ask))
    }

    pub fn replace(&self, map: HashMap<String, PoolPair>) {
        *self.map.write().unwrap() = map;
    }
}

/// Fetch VyFi's per-pool order addresses (mainnet) every 6 hours. Failures
/// are non-fatal - VyFi detection is simply inactive until one succeeds.
pub async fn refresh_loop(addrs: std::sync::Arc<super::dex::DexRegistry>) {
    loop {
        match fetch_order_pools().await {
            Ok(map) if !map.is_empty() => {
                tracing::info!("vyfi: loaded {} order addresses", map.len());
                addrs.vyfinance().replace(map);
            }
            Ok(_) => tracing::debug!("vyfi: empty address list"),
            Err(e) => tracing::debug!("vyfi address fetch failed: {e:#}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
    }
}
async fn fetch_order_pools() -> anyhow::Result<HashMap<String, PoolPair>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let pools: Vec<Value> = client
        .get("https://api.vyfi.io/lp?networkId=1&v2=true")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut map = HashMap::new();
    for p in pools {
        let Some(addr) = p
            .get("orderValidatorUtxoAddress")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(units) = p.get("unitsPair").and_then(Value::as_str) else {
            continue;
        };
        let Some(pair) = parse_units_pair(units) else {
            continue;
        };
        map.insert(addr, pair);
    }
    Ok(map)
}

fn parse_units_pair(units: &str) -> Option<PoolPair> {
    let (a, b) = units.split_once('/')?;
    Some(PoolPair {
        asset_a: parse_unit(a)?,
        asset_b: parse_unit(b)?,
    })
}

fn parse_unit(unit: &str) -> Option<AssetRef> {
    if unit.is_empty() || unit.eq_ignore_ascii_case("lovelace") {
        return Some(AssetRef {
            policy: String::new(),
            name_hex: String::new(),
        });
    }
    if unit.len() < 56 {
        return None;
    }
    Some(AssetRef {
        policy: unit[..56].to_string(),
        name_hex: unit[56..].to_string(),
    })
}

/// VyFinance: `Constr 0 [ poolId(56), Constr tag [ minReceive ] ]`.
/// Tag 4 (observed) = sell token for ADA. Other tags need the order→pair map.
fn parse_qty(output: &Value, tx: &Value) -> Option<(u64, i128)> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    if fields.len() < 2 {
        return None;
    }
    if fields[0].bytes()?.len() != 56 {
        return None;
    }
    let (tag, step) = fields[1].as_constr()?;
    let qty = step.first()?.as_int()?;
    if qty <= 0 {
        return None;
    }
    Some((tag, qty))
}
