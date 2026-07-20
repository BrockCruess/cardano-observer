//! Minswap V1 / V2 order + pool detection.
//!
//! V2 order datums carry the LP asset and an `OrderStep` tag, so swap
//! direction and min-receive are readable inline; naming the *out* asset
//! still needs the LP→pair map, which is warmed from Minswap's public
//! market-data API (see [`PoolCache`]).

use super::dex::{AssetRef, CborReader, CborVal, PoolPair, Role, WantedOut, output_datum_cbor};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub const NAME: &str = "Minswap";
pub const ORDERBOOK: bool = false;

/// Order V1, V2 and V3 deployments.
pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1wyx22z2s4kasd3w976pnjf9xdty88epjqfvgkmfnscpd0rg3z8y6v", Role::Swap),
    ("addr1wxn9efv2f6w82hagxqtn62ju4m293tqvw0uhmdl64ch8uwc0h43gt", Role::Swap),
    ("addr1zxn9efv2f6w82hagxqtn62ju4m293tqvw0uhmdl64ch8uw6j2c79gy9l76sdg0xwhd7r0c0kna0tycz4y5s6mlenh8pq6s3z70", Role::Swap),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[
    ("c3e28c36c3447315ba5a56f33da6a6ddc1770a876a8d9f0cb3a97c4c", Role::Swap), // V2
];

pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];

/// Pool NFT policies - tokens that never sit in user wallets.
pub const POOL_NFT_POLICIES: &[&str] = &[
    "5178cc70a14405d3248e415d1a120c61d2aa74b4cee716d475b1495e", // pool NFT V1
    "0be55d262b29f564998ff81efe21bdc0022621c12f15af08d0f2ddb1", // pool NFT V2 deployment
];

/// V2 "MSP" pool-validity token - the policy alone is shared with LP tokens.
pub const POOL_NFT_UNITS: &[&str] =
    &["f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c4d5350"];

pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

/// LP unit (policy||name) → pool asset pair, warmed from Minswap's API so
/// buy orders can show `₳ → ≥TOKEN` without a per-order fetch race.
#[derive(Default)]
pub struct PoolCache {
    pools: Arc<RwLock<HashMap<String, PoolPair>>>,
}

impl PoolCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the datum for min-receive, then name the out asset via the cache.
    pub fn want(&self, output: &Value, tx: &Value) -> Option<WantedOut> {
        let (a_to_b, qty, min, lp_policy, lp_name) = parse_v2_order_datum(output, tx)?;
        let lp_unit = format!("{lp_policy}{lp_name}");
        let mut want = if let Some(p) = self.lookup(&lp_unit) {
            let out = if a_to_b { p.asset_b } else { p.asset_a };
            WantedOut::from_ref(qty, min, out)
        } else if !a_to_b {
            // Minswap sorts ADA first; B→A always wants ADA without a pool lookup.
            WantedOut::ada(qty, min)
        } else {
            // A→B still needs the pool to name token B.
            WantedOut::unresolved(qty, min)
        };
        want.lp_unit = Some(lp_unit);
        want.a_to_b = Some(a_to_b);
        Some(want)
    }

    pub fn lookup(&self, lp_unit: &str) -> Option<PoolPair> {
        if let Some(p) = self.pools.read().unwrap().get(lp_unit).cloned() {
            return Some(p);
        }
        // Cache miss: never block the chain-sync task. Prefetch in the
        // background so a later order on the same pool can resolve.
        let pools = Arc::clone(&self.pools);
        let lp = lp_unit.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Ok(pair) = fetch_pool_pair(&lp).await {
                    pools.write().unwrap().insert(lp, pair);
                }
            });
        }
        None
    }

    /// Prefetch pool pairs every 6 hours. Failures are non-fatal.
    pub async fn refresh_loop(pools: Arc<RwLock<HashMap<String, PoolPair>>>) {
        loop {
            match warm_pool_cache(&pools).await {
                Ok(n) => tracing::info!("minswap: cached {n} pool pairs"),
                Err(e) => tracing::debug!("minswap pool warm failed: {e:#}"),
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    }

    pub fn pools(&self) -> Arc<RwLock<HashMap<String, PoolPair>>> {
        Arc::clone(&self.pools)
    }
}

async fn fetch_pool_pair(lp_unit: &str) -> anyhow::Result<PoolPair> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let url = format!("https://api-mainnet-prod.minswap.org/v1/pools/{lp_unit}/metrics");
    let v: Value = client.get(url).send().await?.error_for_status()?.json().await?;
    parse_pool_metrics(&v)
}

pub(crate) fn parse_pool_metrics(v: &Value) -> anyhow::Result<PoolPair> {
    let parse_asset = |key: &str| -> Option<AssetRef> {
        let a = v.get(key)?;
        let policy = a
            .get("currency_symbol")
            .or_else(|| a.get("policy_id"))
            .and_then(Value::as_str)?
            .to_string();
        let name_hex = a
            .get("token_name")
            .or_else(|| a.get("asset_name"))
            .and_then(Value::as_str)?
            .to_string();
        Some(AssetRef { policy, name_hex })
    };
    Ok(PoolPair {
        asset_a: parse_asset("asset_a").ok_or_else(|| anyhow::anyhow!("missing asset_a"))?,
        asset_b: parse_asset("asset_b").ok_or_else(|| anyhow::anyhow!("missing asset_b"))?,
    })
}

/// Page the Minswap market-data API and fill the LP→pair cache.
async fn warm_pool_cache(
    cache: &Arc<RwLock<HashMap<String, PoolPair>>>,
) -> anyhow::Result<usize> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut search_after: Option<Value> = None;
    let mut loaded = 0usize;
    // Cap pages so a flaky API can't stall boot forever (~4k pools).
    for _ in 0..80 {
        let mut body = json!({
            "sort_field": "liquidity",
            "sort_direction": "desc",
            "limit": 50,
            "protocols": ["MinswapV2", "Minswap"],
        });
        if let Some(sa) = &search_after {
            body.as_object_mut()
                .unwrap()
                .insert("search_after".into(), sa.clone());
        }
        let v: Value = client
            .post("https://api-mainnet-prod.minswap.org/v1/pools/metrics")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let rows = v
            .get("pool_metrics")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        if rows.is_empty() {
            break;
        }
        {
            let mut map = cache.write().unwrap();
            for row in rows {
                let Some(lp) = row.get("lp_asset") else { continue };
                let Some(pol) = lp.get("currency_symbol").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name) = lp.get("token_name").and_then(Value::as_str) else {
                    continue;
                };
                let unit = format!("{pol}{name}");
                if let Ok(pair) = parse_pool_metrics(row) {
                    map.insert(unit, pair);
                    loaded += 1;
                }
            }
        }
        search_after = v.get("search_after").cloned();
        let done = search_after
            .as_ref()
            .map(|s| s.is_null() || s.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(true);
        if done {
            break;
        }
    }
    Ok(loaded)
}

/// Parse Minswap V2 `OrderDatum` from an Ogmios output's inline `datum` (CBOR hex).
/// Returns `(a_to_b, qty, is_minimum, lp_policy, lp_name_hex)`.
pub fn parse_v2_order_datum(
    output: &Value,
    tx: &Value,
) -> Option<(bool, i128, bool, String, String)> {
    let (tag, step_fields, lp_policy, lp_name) = v2_step(output, tx)?;
    // OrderStep: 0=SwapExactIn, 1=StopLoss, 2=OCO, 3=SwapExactOut, 9=SwapMultiRouting
    let (a_to_b, qty, min) = match tag {
        0 | 1 | 2 | 3 => {
            // [direction, amount_opt, receive, …]
            let a_to_b = step_fields.first()?.as_bool()?;
            let receive = step_fields.get(2)?.as_int()?;
            let min = tag != 3; // ExactOut is exact; others are floors (StopLoss is a ceiling - still show as qty)
            (a_to_b, receive, min && tag != 1)
        }
        9 => {
            // SwapMultiRouting: [routings, amount_opt, minimum_receive]
            let routings = step_fields.first()?.as_array()?;
            let a_to_b = routings
                .last()
                .and_then(|h| h.constr_fields(0))
                .and_then(|f| f.get(1)?.as_bool())
                .unwrap_or(true);
            let receive = step_fields.get(2)?.as_int()?;
            (a_to_b, receive, true)
        }
        _ => return None, // Deposit / Withdraw / Donation etc. - handled as LP, not swap want
    };
    Some((a_to_b, qty, min, lp_policy, lp_name))
}

/// Minswap V2 LP order steps (not buy/sell swaps).
pub fn lp_side(output: &Value, tx: &Value) -> Option<&'static str> {
    let (tag, _, _, _) = v2_step(output, tx)?;
    match tag {
        4 => Some("deposit"),           // Deposit
        5 | 6 | 8 => Some("redeem"),    // Withdraw / ZapOut / WithdrawImbalance
        _ => None,
    }
}

fn v2_step(
    output: &Value,
    tx: &Value,
) -> Option<(u64, Vec<CborVal>, String, String)> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    if fields.len() < 7 {
        return None;
    }
    let lp = fields[5].constr_fields(0)?;
    let lp_policy = hex::encode(lp.first()?.bytes()?);
    let lp_name = hex::encode(lp.get(1)?.bytes()?);
    let (tag, step_fields) = fields[6].as_constr()?;
    Some((tag, step_fields.to_vec(), lp_policy, lp_name))
}

/// Minswap V1 batch order: out asset + min receive are inline in the datum
/// (no LP→pair lookup needed). See CatspersCoffee BatchOrder Types.
pub fn parse_v1_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;
    // [sender, receiver, _, Constr0 [ Action[pol,name] | empty, minReceive ], fee, deposit]
    if fields.len() < 4 {
        return None;
    }
    let step = fields.get(3)?.constr_fields(0)?;
    if step.len() < 2 {
        return None;
    }
    let qty = step.get(1)?.as_int()?;
    if qty <= 0 {
        return None;
    }
    let (_tag, action_fields) = step.first()?.as_constr()?;
    if action_fields.len() >= 2 {
        let policy = hex::encode(action_fields[0].bytes()?);
        let name_hex = hex::encode(action_fields[1].bytes()?);
        return Some(WantedOut {
            qty,
            min: true,
            policy,
            name_hex,
            resolved: true,
            lp_unit: None,
            a_to_b: None,
        });
    }
    // Empty action fields → want ADA.
    if action_fields.is_empty() {
        return Some(WantedOut {
            qty,
            min: true,
            policy: String::new(),
            name_hex: String::new(),
            resolved: true,
            lp_unit: None,
            a_to_b: None,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minswap_v1_buy_reads_out_asset_from_datum() {
        // Live ADA→SNEK V1 order (tx a938ead0…): companion datum, not inline.
        let datum = "d8799fd8799fd8799f581c81c784f7113c761123af5442f282b4ef43a325f3537cf0b9c3542eecffd8799fd8799fd8799f581c87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360ffffffffd8799fd8799f581c81c784f7113c761123af5442f282b4ef43a325f3537cf0b9c3542eecffd8799fd8799fd8799f581c87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360ffffffffd87a80d8799fd8799f581c279c909f348e533da5808898f87f9a14bb2c3dfbbacccd631d927a3f44534e454bff1a00047e85ff1a001e84801a001e8480ff";
        let dh = "e6b7b160426a217ac90fc29da0895557840a7c421f3d742621b4addd8f23a5db";
        let output = json!({
            "address": "addr1zxn9efv2f6w82hagxqtn62ju4m293tqvw0uhmdl64ch8uwu8px9reldfcwsaa3t90nnm6n8sw4lsgaxjeh6qxtdhzdsqq32elw",
            "value": { "ada": { "lovelace": 587_333_290 } },
            "datumHash": dh,
        });
        let tx = json!({ "datums": { dh: datum } });
        let want = parse_v1_want(&output, &tx).expect("want");
        assert!(want.resolved);
        assert_eq!(
            want.policy,
            "279c909f348e533da5808898f87f9a14bb2c3dfbbacccd631d927a3f"
        );
        assert_eq!(want.name_hex, "534e454b"); // SNEK
        assert_eq!(want.qty, 0x47e85);
    }
}
