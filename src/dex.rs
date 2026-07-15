//! DEX transaction detection for every major Cardano DEX.
//!
//! Detection is deliberately stateless-first so it works from Ogmios
//! chain-sync alone (which resolves outputs but not input addresses):
//!
//! 1. **Order placements** - an output paying to a known DEX order/batch
//!    script credential is a swap order being placed. What the user deposits
//!    tells us the direction: ADA only → buying tokens; one token → selling
//!    that token; several tokens → token↔token swap.
//!    When an inline datum is present (Minswap / WingRiders / Sundae / VyFi)
//!    we also read the minimum-receive amount so the UI can show both sides,
//!    e.g. `₳ 73 → ≥16,490 COCK`. Same-block place+fill collapses to Swap.
//!    Liquidity contracts (and any UTxO carrying LP-like assets) emit a
//!    separate `dex_lp` event: deposit (ADA+tokens in) or redeem (LP shares in).
//! 2. **Fills** - batcher transactions recreate the pool UTxO. A tx that
//!    spends a tracked order *and* touches a pool emits one `dex_fill` (or
//!    filled `dex_lp`) per order.
//! 3. **Cancellations** - a tx that spends a tracked order *without* touching
//!    a pool is a cancel.
//!
//! Contract constants were sourced from the open-source Iris DEX indexer
//! (IndigoProtocol/iris) and each DEX's own published deployments. VyFi
//! derives one order address per pool, so its address set is fetched from
//! the official VyFi API at startup and refreshed periodically.

use crate::model::ChainEvent;
use bech32::primitives::decode::CheckedHrpstring;
use bech32::Bech32;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// What a matched order/batch script is for.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Swap,
    Liquidity,
}

/// bech32 order/batch contract addresses: (address, dex, role)
const ORDER_ADDRESSES: &[(&str, &str, Role)] = &[
    // Minswap V1 (order V1, V2 and V3 deployments)
    ("addr1wyx22z2s4kasd3w976pnjf9xdty88epjqfvgkmfnscpd0rg3z8y6v", "Minswap", Role::Swap),
    ("addr1wxn9efv2f6w82hagxqtn62ju4m293tqvw0uhmdl64ch8uwc0h43gt", "Minswap", Role::Swap),
    ("addr1zxn9efv2f6w82hagxqtn62ju4m293tqvw0uhmdl64ch8uw6j2c79gy9l76sdg0xwhd7r0c0kna0tycz4y5s6mlenh8pq6s3z70", "Minswap", Role::Swap),
    // SundaeSwap V1
    ("addr1wxaptpmxcxawvr3pzlhgnpmzz3ql43n2tc8mn3av5kx0yzs09tqh8", "SundaeSwap", Role::Swap),
    // MuesliSwap order book + AMM batch orders
    ("addr1w84psng20ejqcj6a4gljemu9re65waefct7cnahlhmtcwnq63kxyq", "MuesliSwap", Role::Swap),
    ("addr1wy2mjh76em44qurn5x73nzqrxua7ataasftql0u2h6g88lc3gtgpz", "MuesliSwap", Role::Swap),
    ("addr1z8c7eyxnxgy80qs5ehrl4yy93tzkyqjnmx0cfsgrxkfge27q47h8tv3jp07j8yneaxj7qc63zyzqhl933xsglcsgtqcqxzc2je", "MuesliSwap", Role::Swap),
    ("addr1z8l28a6jsx4870ulrfygqvqqdnkdjc5sa8f70ys6dvgvjqc3r6dxnzml343sx8jweqn4vn3fz2kj8kgu9czghx0jrsyqxyrhvq", "MuesliSwap", Role::Swap),
    ("addr1zyq0kyrml023kwjk8zr86d5gaxrt5w8lxnah8r6m6s4jp4g3r6dxnzml343sx8jweqn4vn3fz2kj8kgu9czghx0jrsyqqktyhv", "MuesliSwap", Role::Swap),
    ("addr1w9e7m6yn74r7m0f9mf548ldr8j4v6q05gprey2lhch8tj5gsvyte9", "MuesliSwap", Role::Swap),
    // Spectrum / Splash - swap order + liquidity deposit/redeem contracts
    ("addr1wynp362vmvr8jtc946d3a3utqgclfdl5y9d3kn849e359hsskr20n", "Splash", Role::Swap),
    ("addr1wyr4uz0tp75fu8wrg6gm83t20aphuc9vt6n8kvu09ctkugqpsrmeh", "Splash", Role::Liquidity),
    ("addr1wxpa5704x8qel88ympf4natfdzn59nc9esj7609y3sczmmsasees8", "Splash", Role::Liquidity),
    ("addr1w95q755yrsr0xt8vmn007tpqee4hps49yjdef5dzknhl99qntsmh0", "Splash", Role::Liquidity),
    ("addr1wymhr2l96gm22xkwz0rn3zz79xz9l400nm5sa580kssdyagr5z7wq", "Splash", Role::Liquidity),
    ("addr1wxrl2p9s0tweu8t54cgz75at070ly3tda6yh5s7cufanfzc52gv39", "Splash", Role::Liquidity),
    ("addr1wxu29wa80fd4ptpfwqe20vpxrum45f57ud3r6egh9vuyhfc2a3jhj", "Splash", Role::Liquidity),
];

/// Raw order-script payment credentials (blake2b-224 hex): (hash, dex, role)
const ORDER_SCRIPT_HASHES: &[(&str, &str, Role)] = &[
    ("c3e28c36c3447315ba5a56f33da6a6ddc1770a876a8d9f0cb3a97c4c", "Minswap", Role::Swap), // V2
    ("fa6a58bbe2d0ff05534431c8e2f0ef2cbdc1602a8456e4b13c8f3077", "SundaeSwap", Role::Swap), // V3
    ("86ae9eebd8b97944a45201e4aec1330a72291af2d071644bba015959", "WingRiders", Role::Swap), // V1
    ("c5e0385012d5f010b1dc7ab42ba632944052de232051ec6ce3bfd72e", "WingRiders", Role::Swap), // V1
    ("c134d839a64a5dfb9b155869ef3f34280751a622f69958baa8ffd29c", "WingRiders", Role::Swap), // V2
    ("23680ea6701b56f2c12ae79d8af94fd36f509b7b007029c7ce114840", "WingRiders", Role::Swap), // V2
    ("2025463437ee5d64e89814a66ce7f98cb184a66ae85a2fbbfd750106", "Splash", Role::Swap),
    ("464eeee89f05aff787d40045af2a40a83fd96c513197d32fbc54ff02", "Splash", Role::Swap),
];

/// Pool contract addresses - an output paying here is a pool state update.
const POOL_ADDRESSES: &[(&str, &str)] = &[
    ("addr1w9qzpelu9hn45pefc0xr4ac4kdxeswq7pndul2vuj59u8tqaxdznu", "SundaeSwap"), // V1
    ("addr1x8nz307k3sr60gu0e47cmajssy4fmld7u493a4xztjrll0aj764lvrxdayh2ux30fl0ktuh27csgmpevdu89jlxppvrswgxsta", "Splash"), // Spectrum pool V1
    ("addr1x94ec3t25egvhqy2n265xfhq882jxhkknurfe9ny4rl9k6dj764lvrxdayh2ux30fl0ktuh27csgmpevdu89jlxppvrst84slu", "Splash"), // Spectrum pool V2
];

/// Pool-NFT policy ids - any output carrying an asset under these policies
/// is the recreated pool UTxO of a settlement tx. Only policies whose tokens
/// never sit in user wallets are listed (LP-token policies would misfire).
const POOL_NFT_POLICIES: &[(&str, &str)] = &[
    ("5178cc70a14405d3248e415d1a120c61d2aa74b4cee716d475b1495e", "Minswap"), // pool NFT V1
    ("0be55d262b29f564998ff81efe21bdc0022621c12f15af08d0f2ddb1", "Minswap"), // pool NFT V2 deployment
    ("026a18d04a0c642759bb3d83b12e3344894e5c1c7b2aeb1a2113a570", "WingRiders"), // V1
    ("6fdc63a1d71dc2c65502b79baae7fb543185702b12c3c5fb639ed737", "WingRiders"), // V2
    ("909133088303c49f3a30f1cc8ed553a73857a29779f6c6561cd8093f", "MuesliSwap"),
    ("7a8041a0693e6605d010d5185b034d55c79eaf7ef878aae3bdcdbf67", "MuesliSwap"),
];

/// Exact pool-validity units (policy+nameHex) where the policy alone is
/// shared with user-held LP tokens.
const POOL_NFT_UNITS: &[(&str, &str)] = &[
    // Minswap V2 "MSP" pool-validity token
    ("f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c4d5350", "Minswap"),
];

/// (policy, asset-name-hex prefix) pool NFTs - SundaeSwap V3 shares one
/// policy between pool NFTs (CIP-67 label 000de140) and LP tokens (0014df1…).
const POOL_NFT_PREFIXES: &[(&str, &str, &str)] = &[
    ("e0302560ced2fdcbfcb2602697df970cd0d6a38f94b32703f51c312b", "000de140", "SundaeSwap"),
];

const ORDER_TRACKER_CAP: usize = 30_000;

pub struct DexRegistry {
    /// payment credential (28-byte hex) -> (dex, role)
    order_creds: HashMap<String, (&'static str, Role)>,
    pool_creds: HashMap<String, &'static str>,
    pool_nft_policies: HashMap<&'static str, &'static str>,
    pool_nft_units: HashMap<&'static str, &'static str>,
    /// full bech32 address -> dex (VyFi's per-pool order addresses)
    dyn_order_addrs: RwLock<HashSet<String>>,
    /// Minswap LP unit (policy||name) -> pool asset pair
    minswap_pools: Arc<RwLock<HashMap<String, PoolPair>>>,
    /// "txhash#index" -> dex, for cancel/settle attribution
    tracker: Mutex<OrderTracker>,
}

struct OrderTracker {
    map: HashMap<String, OrderInfo>,
    order: VecDeque<String>,
}

/// What we know about a placed order, kept so its settlement or cancellation
/// can show the fill details.
#[derive(Clone)]
struct OrderInfo {
    dex: &'static str,
    side: &'static str,
    ada: u64,
    assets: Value,
    /// Minimum (or exact) output the order wants, when we could read the datum.
    want: Option<WantedOut>,
    /// Block height where the order UTxO was created (for same-block → Swap).
    placed_height: u64,
}

/// Per-block buffer: order placements are deferred until the end of the block
/// so a place+fill in the same block collapses to a single Swap.
#[derive(Default)]
pub struct BlockDexBuf {
    /// outpoint → (placement tx hash, pending order hit)
    deferred: HashMap<String, (String, DexHit)>,
}

#[derive(Clone)]
struct WantedOut {
    qty: i128,
    /// True → display as ≥qty (limit / slippage floor).
    min: bool,
    policy: String,
    name_hex: String,
    /// False until the LP→pair cache resolved the out asset.
    resolved: bool,
}

fn attach_want(obj: &mut serde_json::Map<String, Value>, want: &Option<WantedOut>, filled: bool) {
    let Some(w) = want else { return };
    obj.insert("wantQty".into(), json!(w.qty.to_string()));
    // ≥ only on open orders - fills already executed, so a floor looks like a lie.
    obj.insert("wantMin".into(), json!(w.min && !filled));
    if !w.resolved {
        return;
    }
    if w.policy.is_empty() && w.name_hex.is_empty() {
        // Buying/selling for ADA - keep lovelace so the UI can fmtAda.
        obj.insert("wantAda".into(), json!(w.qty as u64));
    } else {
        let refs = [(w.policy.clone(), w.name_hex.clone(), w.qty)];
        obj.insert("want".into(), crate::parse::asset_list(&[&refs[0]]));
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AssetRef {
    policy: String,
    name_hex: String,
}

#[derive(Clone)]
struct PoolPair {
    asset_a: AssetRef,
    asset_b: AssetRef,
}

/// One detected DEX happening within a tx.
pub struct DexHit {
    pub kind: &'static str, // dex_order | dex_fill | dex_lp | dex_cancel
    pub title: String,
    pub data: Value,
}

impl DexRegistry {
    pub fn new() -> Self {
        let mut order_creds = HashMap::new();
        for (addr, dex, role) in ORDER_ADDRESSES {
            if let Some(cred) = payment_credential(addr) {
                order_creds.insert(cred, (*dex, *role));
            }
        }
        for (hash, dex, role) in ORDER_SCRIPT_HASHES {
            order_creds.insert((*hash).to_string(), (*dex, *role));
        }
        let mut pool_creds = HashMap::new();
        for (addr, dex) in POOL_ADDRESSES {
            if let Some(cred) = payment_credential(addr) {
                pool_creds.insert(cred, *dex);
            }
        }
        DexRegistry {
            order_creds,
            pool_creds,
            pool_nft_policies: POOL_NFT_POLICIES.iter().copied().collect(),
            pool_nft_units: POOL_NFT_UNITS.iter().copied().collect(),
            dyn_order_addrs: RwLock::new(HashSet::new()),
            minswap_pools: Arc::new(RwLock::new(HashMap::new())),
            tracker: Mutex::new(OrderTracker {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Scan one transaction within a block. New order placements are staged in
    /// `buf` and only flushed via [`Self::flush_block`] so a same-block fill
    /// becomes a single Swap instead of Pending + Fill.
    pub fn scan_tx(
        &self,
        tx: &Value,
        tx_hash: &str,
        height: u64,
        buf: &mut BlockDexBuf,
    ) -> Vec<DexHit> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);

        let mut hits = Vec::new();
        let mut pools_touched: HashSet<&'static str> = HashSet::new();
        let mut new_orders: Vec<(String, OrderInfo)> = Vec::new();

        for (index, output) in outputs.iter().enumerate() {
            let addr = output.get("address").and_then(Value::as_str).unwrap_or("");

            if let Some(dex) = self.pool_dex_for_output(output, addr) {
                pools_touched.insert(dex);
                continue;
            }

            let matched = payment_credential(addr)
                .and_then(|cred| self.order_creds.get(&cred).copied())
                .or_else(|| {
                    self.dyn_order_addrs
                        .read()
                        .unwrap()
                        .contains(addr)
                        .then_some(("VyFinance", Role::Swap))
                });
            if let Some((dex, role)) = matched {
                let info = self.order_info(dex, role, output, tx, height);
                let outpoint = format!("{tx_hash}#{index}");
                buf.deferred
                    .insert(outpoint.clone(), (tx_hash.to_string(), order_hit(&info)));
                new_orders.push((outpoint, info));
            }
        }

        let consumed = self.take_consumed(inputs);
        for (outpoint, info) in consumed {
            let same_block = buf.deferred.remove(&outpoint).is_some()
                || info.placed_height == height;
            if pools_touched.contains(info.dex) {
                hits.push(fill_hit(&info, same_block));
            } else {
                hits.push(cancel_hit(&info));
            }
        }

        self.remember_orders(new_orders);
        hits
    }

    /// Emit order placements that were not filled/cancelled in this block.
    pub fn flush_block(buf: BlockDexBuf) -> Vec<(String, DexHit)> {
        buf.deferred.into_values().collect()
    }

    fn pool_dex_for_output(&self, output: &Value, addr: &str) -> Option<&'static str> {
        if let Some(dex) = payment_credential(addr).and_then(|c| self.pool_creds.get(&c).copied()) {
            return Some(dex);
        }
        let value = output.get("value").and_then(Value::as_object)?;
        for (policy, assets) in value {
            if policy == "ada" {
                continue;
            }
            if let Some(dex) = self.pool_nft_policies.get(policy.as_str()) {
                return Some(dex);
            }
            let Some(assets) = assets.as_object() else { continue };
            for name_hex in assets.keys() {
                let unit = format!("{policy}{name_hex}");
                if let Some(dex) = self.pool_nft_units.get(unit.as_str()) {
                    return Some(dex);
                }
                for (p, prefix, dex) in POOL_NFT_PREFIXES {
                    if policy == p && name_hex.starts_with(prefix) {
                        return Some(dex);
                    }
                }
            }
        }
        None
    }

    fn remember_orders(&self, orders: Vec<(String, OrderInfo)>) {
        if orders.is_empty() {
            return;
        }
        let mut t = self.tracker.lock().unwrap();
        for (outpoint, info) in orders {
            if t.map.len() >= ORDER_TRACKER_CAP {
                if let Some(old) = t.order.pop_front() {
                    t.map.remove(&old);
                }
            }
            t.order.push_back(outpoint.clone());
            t.map.insert(outpoint, info);
        }
    }

    fn take_consumed(&self, inputs: &[Value]) -> Vec<(String, OrderInfo)> {
        let mut t = self.tracker.lock().unwrap();
        if t.map.is_empty() {
            return Vec::new();
        }
        inputs
            .iter()
            .filter_map(|i| {
                let txid = i.get("transaction")?.get("id")?.as_str()?;
                let index = i.get("index")?.as_u64()?;
                let key = format!("{txid}#{index}");
                t.map.remove(&key).map(|info| (key, info))
            })
            .collect()
    }

    /// Fetch VyFi's per-pool order addresses (mainnet only) and refresh
    /// them every 6 hours. Failures are non-fatal - VyFi detection is
    /// simply inactive until a fetch succeeds.
    pub async fn refresh_vyfi_loop(self: std::sync::Arc<Self>) {
        loop {
            match fetch_vyfi_order_addresses().await {
                Ok(addrs) if !addrs.is_empty() => {
                    tracing::info!("vyfi: loaded {} order addresses", addrs.len());
                    *self.dyn_order_addrs.write().unwrap() = addrs;
                }
                Ok(_) => tracing::debug!("vyfi: empty address list"),
                Err(e) => tracing::debug!("vyfi address fetch failed: {e:#}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    }

    /// Classify a script output as buy / sell / swap, or LP deposit / redeem.
    fn order_info(
        &self,
        dex: &'static str,
        role: Role,
        output: &Value,
        tx: &Value,
        height: u64,
    ) -> OrderInfo {
        let mut assets: Vec<(String, String, i128)> = Vec::new();
        crate::parse::collect_assets(output.get("value"), &mut assets);
        let has_lp = assets.iter().any(|(_, name, _)| is_lp_like_asset(name));
        let ada = output_lovelace(output);

        // Minswap V2 encodes deposit/withdraw in the order datum step - those
        // UTxOs often look like a one-token "sell" (e.g. ADA+NIGHT deposit).
        let minswap_lp = (dex == "Minswap").then(|| minswap_lp_side(output, tx)).flatten();

        let side = if let Some(s) = minswap_lp {
            s
        } else if role == Role::Liquidity || has_lp {
            if has_lp {
                "redeem"
            } else {
                "deposit"
            }
        } else if assets.is_empty() {
            "buy"
        } else if assets.len() == 1 {
            "sell"
        } else {
            "swap"
        };
        let refs: Vec<&(String, String, i128)> = assets.iter().collect();
        let want = if matches!(side, "buy" | "sell" | "swap") {
            self.order_want(dex, output, tx)
        } else {
            None
        };
        OrderInfo {
            dex,
            side,
            ada,
            assets: crate::parse::asset_list(&refs),
            want,
            placed_height: height,
        }
    }

    fn order_want(&self, dex: &'static str, output: &Value, tx: &Value) -> Option<WantedOut> {
        match dex {
            "Minswap" => self.minswap_want(output, tx),
            "SundaeSwap" => parse_sundae_want(output, tx)
                .or_else(|| parse_offer_ask_want(output, tx)),
            "VyFinance" => parse_vyfi_want(output, tx),
            "WingRiders" => parse_wingriders_want(output, tx),
            "Splash" => parse_splash_want(output, tx),
            _ => None,
        }
    }

    /// Read Minswap V2 order datum for min-receive + resolve the out asset
    /// via the LP→pair cache (filled lazily from Minswap's API).
    fn minswap_want(&self, output: &Value, tx: &Value) -> Option<WantedOut> {
        let (a_to_b, qty, min, lp_policy, lp_name) = parse_minswap_v2_order_datum(output, tx)?;
        let lp_unit = format!("{lp_policy}{lp_name}");
        let pair = self.lookup_minswap_pool(&lp_unit);
        match pair {
            Some(p) => {
                let out = if a_to_b { p.asset_b } else { p.asset_a };
                Some(WantedOut {
                    qty,
                    min,
                    policy: out.policy,
                    name_hex: out.name_hex,
                    resolved: true,
                })
            }
            None => Some(WantedOut {
                qty,
                min,
                policy: String::new(),
                name_hex: String::new(),
                resolved: false,
            }),
        }
    }

    fn lookup_minswap_pool(&self, lp_unit: &str) -> Option<PoolPair> {
        if let Some(p) = self.minswap_pools.read().unwrap().get(lp_unit).cloned() {
            return Some(p);
        }
        // Cache miss: never block the chain-sync task. Prefetch in the
        // background so a later order on the same pool can resolve.
        let pools = Arc::clone(&self.minswap_pools);
        let lp = lp_unit.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Ok(pair) = fetch_minswap_pool_pair(&lp).await {
                    pools.write().unwrap().insert(lp, pair);
                }
            });
        }
        None
    }
}

fn is_lp_side(side: &str) -> bool {
    matches!(side, "deposit" | "redeem")
}

fn order_hit(info: &OrderInfo) -> DexHit {
    if is_lp_side(info.side) {
        return DexHit {
            kind: "dex_lp",
            title: format!(
                "LP {} - {}",
                if info.side == "deposit" { "Deposit" } else { "Redeem" },
                info.dex
            ),
            data: order_data(info, false),
        };
    }
    DexHit {
        kind: "dex_order",
        title: match info.side {
            "buy" => format!("Buy Order - {}", info.dex),
            "sell" => format!("Sell Order - {}", info.dex),
            _ => format!("Swap - {}", info.dex),
        },
        data: order_data(info, false),
    }
}

fn fill_hit(info: &OrderInfo, instant_swap: bool) -> DexHit {
    if is_lp_side(info.side) {
        let verb = if info.side == "deposit" { "Deposit" } else { "Redeem" };
        return DexHit {
            kind: "dex_lp",
            title: if instant_swap {
                format!("LP {verb} - {}", info.dex)
            } else {
                format!("LP {verb} Filled - {}", info.dex)
            },
            data: order_data(info, true),
        };
    }
    DexHit {
        kind: "dex_fill",
        title: if instant_swap {
            format!("Swap - {}", info.dex)
        } else {
            format!("Order Fill - {}", info.dex)
        },
        data: order_data(info, true),
    }
}

fn cancel_hit(info: &OrderInfo) -> DexHit {
    let title = if is_lp_side(info.side) {
        format!("LP Cancelled - {}", info.dex)
    } else {
        match info.side {
            "buy" => format!("Buy Cancelled - {}", info.dex),
            "sell" => format!("Sell Cancelled - {}", info.dex),
            _ => format!("Order Cancelled - {}", info.dex),
        }
    };
    DexHit {
        kind: "dex_cancel",
        title,
        data: order_data(info, false),
    }
}

fn order_data(info: &OrderInfo, filled: bool) -> Value {
    let mut data = json!({
        "dex": info.dex,
        "side": info.side,
        "ada": info.ada,
        "assets": info.assets,
        "filled": filled,
    });
    attach_want(data.as_object_mut().unwrap(), &info.want, filled);
    data
}

fn output_lovelace(output: &Value) -> u64 {
    output
        .get("value")
        .and_then(|v| v.get("ada"))
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Heuristic: LP / pool-share tokens (often not in token registries).
/// CIP-67 label `0014df10` = LP tokens; names like `*_LQ` / `*LP` are common too.
fn is_lp_like_asset(name_hex: &str) -> bool {
    let lower = name_hex.to_ascii_lowercase();
    if lower.starts_with("0014df10") {
        return true;
    }
    let Some(name) = crate::parse::decode_asset_name(name_hex) else {
        return false;
    };
    let u = name.to_ascii_uppercase();
    u.contains("_LQ")
        || u.ends_with(" LQ")
        || u.ends_with("_LP")
        || u.ends_with(" LP")
        || u.contains("LP_TOKEN")
        || u.contains("_ADA_LQ")
        || u.ends_with("LQ") && u.contains('_')
}

/// Extract the payment credential (28-byte hex) from a bech32 Shelley
/// address, if its payment part is a script or key hash.
pub fn payment_credential(addr: &str) -> Option<String> {
    if !addr.starts_with("addr1") && !addr.starts_with("addr_test1") {
        return None;
    }
    let checked = CheckedHrpstring::new::<Bech32>(addr).ok()?;
    let bytes: Vec<u8> = checked.byte_iter().collect();
    if bytes.len() < 29 {
        return None;
    }
    Some(hex::encode(&bytes[1..29]))
}

async fn fetch_vyfi_order_addresses() -> anyhow::Result<HashSet<String>> {
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
    Ok(pools
        .iter()
        .filter_map(|p| p.get("orderValidatorUtxoAddress").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

async fn fetch_minswap_pool_pair(lp_unit: &str) -> anyhow::Result<PoolPair> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let url = format!("https://api-mainnet-prod.minswap.org/v1/pools/{lp_unit}/metrics");
    let v: Value = client.get(url).send().await?.error_for_status()?.json().await?;
    let parse_asset = |key: &str| -> Option<AssetRef> {
        let a = v.get(key)?;
        Some(AssetRef {
            policy: a.get("currency_symbol")?.as_str()?.to_string(),
            name_hex: a.get("token_name")?.as_str()?.to_string(),
        })
    };
    Ok(PoolPair {
        asset_a: parse_asset("asset_a").ok_or_else(|| anyhow::anyhow!("missing asset_a"))?,
        asset_b: parse_asset("asset_b").ok_or_else(|| anyhow::anyhow!("missing asset_b"))?,
    })
}

/// Parse Minswap V2 `OrderDatum` from an Ogmios output's inline `datum` (CBOR hex).
/// Returns `(a_to_b, qty, is_minimum, lp_policy, lp_name_hex)`.
fn parse_minswap_v2_order_datum(
    output: &Value,
    tx: &Value,
) -> Option<(bool, i128, bool, String, String)> {
    let (tag, step_fields, lp_policy, lp_name) = minswap_v2_step(output, tx)?;
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
fn minswap_lp_side(output: &Value, tx: &Value) -> Option<&'static str> {
    let (tag, _, _, _) = minswap_v2_step(output, tx)?;
    match tag {
        4 => Some("deposit"),           // Deposit
        5 | 6 | 8 => Some("redeem"),    // Withdraw / ZapOut / WithdrawImbalance
        _ => None,
    }
}

fn minswap_v2_step(
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

/// Sundae V1 swap: `[poolId, dest…, fee, Swap{direction, amount, Some min}]`.
/// Asset A is ADA - `direction=true` (B→A) means selling a token for ADA.
fn parse_sundae_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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
        });
    }
    None
}

/// VyFinance: `Constr 0 [ poolId(56), Constr tag [ minReceive ] ]`.
/// Tag 4 (observed) = sell token for ADA.
fn parse_vyfi_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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
    // Tag 4 → ADA out. Other positive tags: show qty until we learn the asset.
    Some(WantedOut {
        qty,
        min: true,
        policy: String::new(),
        name_hex: String::new(),
        resolved: tag == 4,
    })
}

/// Splash / Spectrum swap order:
/// `Constr 0 [ …, baseAsset, baseAmt, _, quoteAmt, quoteAsset, price, … ]`
/// where each asset is `Constr 0 [ policy, name ]` (ADA = empty bytes).
fn parse_splash_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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
    })
}

/// WingRiders V1/V2: offer asset + ask asset + min receive.
/// V2 layout (common): `[fee, dest…, deadline, offerPol, offerName, askPol, askName, minConstr, …]`
/// Older nested layout: `[… poolPair [A,B] …, minConstr]` where `[A,B]` is pool
/// order (often ADA then token), *not* offer/ask - infer ask from the UTxO.
fn parse_wingriders_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    let fields = root.constr_fields(0)?;

    // V2: offer/ask are consecutive byte pairs after the deadline int.
    if fields.len() >= 11 {
        if let (Some(ask_pol), Some(ask_name), Some(min_c)) = (
            fields.get(8).and_then(CborVal::bytes),
            fields.get(9).and_then(CborVal::bytes),
            fields.get(10),
        ) {
            // fields[6], [7] should also be bytes (offer)
            if fields.get(6).and_then(CborVal::bytes).is_some()
                && fields.get(7).and_then(CborVal::bytes).is_some()
            {
                if let Some(qty) = extract_min_qty(min_c) {
                    return Some(WantedOut {
                        qty,
                        min: true,
                        policy: hex::encode(ask_pol),
                        name_hex: hex::encode(ask_name),
                        resolved: true,
                    });
                }
            }
        }
    }

    // Nested shape (e.g. SNEK→ADA sell tx 82a10426…): pool pair + min qty.
    // Taking pair.1 as "ask" wrongly labels ADA receives as the token.
    if fields.len() >= 2 {
        if let (Some((a, b)), Some(qty)) = (
            fields.first().and_then(find_wr_asset_pair),
            fields.get(1).and_then(extract_min_qty),
        ) {
            let ask = wr_ask_from_pair(output, &a, &b).or_else(|| {
                wr_ask_from_direction(fields.get(1)?, &a, &b)
            })?;
            return Some(WantedOut {
                qty,
                min: true,
                policy: ask.policy,
                name_hex: ask.name_hex,
                resolved: true,
            });
        }
    }
    None
}

/// Prefer the pool-pair side that is *not* deposited in the order UTxO.
fn wr_ask_from_pair(output: &Value, a: &AssetRef, b: &AssetRef) -> Option<AssetRef> {
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

/// Scoop / Sundae V3 style: a constr holding two `[policy, name, qty]` arrays
/// (offer, ask). The ask is what the order wants to receive.
fn parse_offer_ask_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    offer_ask_from_val(&root)
}

fn offer_ask_from_val(v: &CborVal) -> Option<WantedOut> {
    if let CborVal::Constr { fields, .. } = v {
        if fields.len() == 2 {
            if let (Some((_offer, _)), Some((ask, qty))) =
                (asset_amount(&fields[0]), asset_amount(&fields[1]))
            {
                return Some(WantedOut {
                    qty,
                    min: true,
                    policy: ask.policy,
                    name_hex: ask.name_hex,
                    resolved: true,
                });
            }
        }
        for f in fields {
            if let Some(w) = offer_ask_from_val(f) {
                return Some(w);
            }
        }
    }
    if let CborVal::Array(items) = v {
        for i in items {
            if let Some(w) = offer_ask_from_val(i) {
                return Some(w);
            }
        }
    }
    None
}

fn asset_amount(v: &CborVal) -> Option<(AssetRef, i128)> {
    let arr = v.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    Some((
        AssetRef {
            policy: hex::encode(arr[0].bytes()?),
            name_hex: hex::encode(arr[1].bytes()?),
        },
        arr[2].as_int()?,
    ))
}

fn option_int(v: &CborVal) -> Option<i128> {
    // Aiken Option: Some = Constr 0 [x], None = Constr 1 []
    match v.as_constr() {
        Some((0, fields)) => fields.first()?.as_int(),
        Some((1, _)) => None,
        _ => v.as_int(), // bare int fallback
    }
}

fn output_datum_cbor(output: &Value, tx: Option<&Value>) -> Option<Vec<u8>> {
    if let Some(d) = output.get("datum") {
        if let Some(s) = d.as_str() {
            return hex::decode(s).ok();
        }
        if let Some(s) = d.get("cbor").and_then(Value::as_str) {
            return hex::decode(s).ok();
        }
    }
    // Companion datums keyed by hash (common for Sundae V1 / VyFi).
    let dh = output.get("datumHash").and_then(Value::as_str)?;
    let entry = tx?.get("datums")?.as_object()?.get(dh)?;
    if let Some(s) = entry.as_str() {
        return hex::decode(s).ok();
    }
    if let Some(s) = entry.get("cbor").and_then(Value::as_str) {
        return hex::decode(s).ok();
    }
    None
}

/// Minimal CBOR reader for Plutus Data (ints, bytes, arrays, Constr tags).
struct CborReader<'a> {
    bytes: &'a [u8],
    i: usize,
}

#[derive(Debug, Clone)]
enum CborVal {
    Int(i128),
    Bytes(Vec<u8>),
    Array(Vec<CborVal>),
    Constr { tag: u64, fields: Vec<CborVal> },
    Bool(bool),
    Other,
}

impl CborVal {
    fn as_constr(&self) -> Option<(u64, &Vec<CborVal>)> {
        match self {
            CborVal::Constr { tag, fields } => Some((*tag, fields)),
            _ => None,
        }
    }
    fn constr_fields(&self, expect_tag: u64) -> Option<&Vec<CborVal>> {
        let (tag, fields) = self.as_constr()?;
        (tag == expect_tag).then_some(fields)
    }
    fn as_array(&self) -> Option<&Vec<CborVal>> {
        match self {
            CborVal::Array(a) => Some(a),
            _ => None,
        }
    }
    fn as_int(&self) -> Option<i128> {
        match self {
            CborVal::Int(n) => Some(*n),
            _ => None,
        }
    }
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            CborVal::Bytes(b) => Some(b),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            // Aiken/Plutus Bool: False = Constr 0, True = Constr 1
            CborVal::Constr { tag: 0, fields } if fields.is_empty() => Some(false),
            CborVal::Constr { tag: 1, fields } if fields.is_empty() => Some(true),
            CborVal::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl<'a> CborReader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.i + n > self.bytes.len() {
            return None;
        }
        let s = &self.bytes[self.i..self.i + n];
        self.i += n;
        Some(s)
    }

    fn decode(&mut self) -> Option<CborVal> {
        if self.i >= self.bytes.len() {
            return None;
        }
        let fb = self.bytes[self.i];
        self.i += 1;
        let mt = fb >> 5;
        let ai = fb & 0x1f;
        let arg = self.read_length(ai)?;
        match mt {
            0 => Some(CborVal::Int(arg? as i128)),
            1 => Some(CborVal::Int(-1 - arg? as i128)),
            2 => {
                let n = arg?;
                Some(CborVal::Bytes(self.take(n as usize)?.to_vec()))
            }
            4 => {
                if let Some(n) = arg {
                    let mut items = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        items.push(self.decode()?);
                    }
                    Some(CborVal::Array(items))
                } else {
                    let mut items = Vec::new();
                    while self.i < self.bytes.len() && self.bytes[self.i] != 0xff {
                        items.push(self.decode()?);
                    }
                    self.take(1)?; // break
                    Some(CborVal::Array(items))
                }
            }
            6 => {
                let tag = arg?;
                let inner = self.decode()?;
                if (121..=127).contains(&tag) {
                    let fields = match inner {
                        CborVal::Array(a) => a,
                        _ => return None,
                    };
                    Some(CborVal::Constr {
                        tag: tag - 121,
                        fields,
                    })
                } else if tag == 102 {
                    // alternative: #6.102([tag, fields])
                    let arr = match inner {
                        CborVal::Array(a) if a.len() == 2 => a,
                        _ => return None,
                    };
                    let t = arr[0].as_int()? as u64;
                    let fields = match &arr[1] {
                        CborVal::Array(a) => a.clone(),
                        _ => return None,
                    };
                    Some(CborVal::Constr { tag: t, fields })
                } else {
                    Some(CborVal::Other)
                }
            }
            7 => match ai {
                20 => Some(CborVal::Bool(false)),
                21 => Some(CborVal::Bool(true)),
                _ => Some(CborVal::Other),
            },
            _ => Some(CborVal::Other),
        }
    }

    fn read_length(&mut self, ai: u8) -> Option<Option<u64>> {
        match ai {
            0..=23 => Some(Some(ai as u64)),
            24 => Some(Some(*self.take(1)?.first()? as u64)),
            25 => {
                let b: [u8; 2] = self.take(2)?.try_into().ok()?;
                Some(Some(u64::from(u16::from_be_bytes(b))))
            }
            26 => {
                let b: [u8; 4] = self.take(4)?.try_into().ok()?;
                Some(Some(u64::from(u32::from_be_bytes(b))))
            }
            27 => {
                let b: [u8; 8] = self.take(8)?.try_into().ok()?;
                Some(Some(u64::from_be_bytes(b)))
            }
            31 => Some(None),
            _ => None,
        }
    }
}

/// Turn DexHits into full ChainEvents (helper for parse.rs).
pub fn hit_to_event(
    hit: DexHit,
    slot: u64,
    height: u64,
    block_hash: &str,
    tx_hash: &str,
    timestamp: i64,
) -> ChainEvent {
    ChainEvent {
        id: 0,
        kind: hit.kind.into(),
        category: "dex".into(),
        slot,
        height: Some(height),
        block_hash: Some(block_hash.to_string()),
        tx_hash: Some(tx_hash.to_string()),
        timestamp,
        title: hit.title,
        summary: String::new(),
        data: hit.data,
    }
}

