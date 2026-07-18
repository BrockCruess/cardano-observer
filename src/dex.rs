//! DEX transaction detection for every major Cardano DEX.
//!
//! Detection is deliberately stateless-first so it works from Ogmios
//! chain-sync alone (which resolves outputs but not input addresses):
//!
//! 1. **Order placements** - an output paying to a known DEX order/batch
//!    script credential is a swap order being placed. What the user deposits
//!    tells us the direction: ADA only → buying tokens; one token → selling
//!    that token; several tokens → token↔token swap.
//!    When an inline datum is present (Minswap / WingRiders / Sundae / VyFi /
//!    CSWAP / Genius Yield) we also read the minimum-receive amount so the UI
//!    can show both sides, e.g. `₳ 73 → ≥16,490 COCK`. Same-block place+fill
//!    collapses to Swap. Liquidity contracts (and any UTxO carrying LP-like
//!    assets) emit a separate `dex_lp` event: deposit (ADA+tokens in) or
//!    redeem (LP shares in).
//! 2. **Fills** - batcher transactions recreate the pool UTxO. A tx that
//!    spends a tracked order *and* touches a pool emits one `dex_fill` (or
//!    filled `dex_lp`) per order. Order-book DEXes (Genius Yield, ChadSwap)
//!    settle peer-to-peer, so any spend of a tracked order counts as a fill.
//! 3. **Cancellations** - a tx that spends a tracked order *without* touching
//!    a pool is a cancel (batcher DEXes only).
//! 4. **Direct-pool swaps** - Danogo/Dano CLMM has no order UTxO; we detect
//!    pool rewrites via the pool script credential + withdraw-zero reward
//!    account and diff successive pool values keyed by LP NFT.
//!
//! Contract constants were sourced from the open-source Iris DEX indexer
//! (IndigoProtocol/iris), Charli3 Dendrite, Danogo docs, and each DEX's own
//! published deployments. VyFi derives one order address per pool, so its
//! address set is fetched from the official VyFi API at startup and refreshed
//! periodically.

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
    // CSWAP hybrid AMM order contract
    ("addr1z8d9k3aw6w24eyfjacy809h68dv2rwnpw0arrfau98jk6nhv88awp8sgxk65d6kry0mar3rd0dlkfljz7dv64eu39vfs38yd9p", "CSWAP", Role::Swap),
    // Genius Yield partial-order book
    ("addr1wx5d0l6u7nq3wfcz3qmjlxkgu889kav2u9d8s5wyzes6frqktgru2", "GeniusYield", Role::Swap),
    ("addr1w8kllanr6dlut7t480zzytsd52l7pz4y3kcgxlfvx2ddavcshakwd", "GeniusYield", Role::Swap),
    // ChadSwap order book
    ("addr1w84q0y2wwfj5efd9ch3x492edeh6pdwycvt7g030jfzhagg5ftr54", "ChadSwap", Role::Swap),
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
    // CSWAP pool
    ("addr1z8ke0c9p89rjfwmuh98jpt8ky74uy5mffjft3zlcld9h7ml3lmln3mwk0y3zsh3gs3dzqlwa9rjzrxawkwm4udw9axhs6fuu6e", "CSWAP"),
    // Danogo / Dano Finance CLMM (direct-spend pools; no batcher order UTxO).
    // Wire `data.dex` matches the UI filter chip (`Dano Finance`).
    ("addr1x8vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0w7dwgsd23ac468cjj8rcnyuc3s72rtupu6j9dw0xpw83exsufvrg4", "Dano Finance"),
    ("addr1w8vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0wc7a283u", "Dano Finance"),
];

/// Pool script credentials that aren't derived from POOL_ADDRESSES alone.
const POOL_SCRIPT_HASHES: &[(&str, &str)] = &[
    // Danogo CLMM pool validator (payment credential of both pool addresses)
    ("d8b69fc53637bcfadbc4469083f706bc293f4d9d2296646c5ca167bb", "Dano Finance"),
];

/// Danogo LP / position NFT policy (= pool script hash). Each pool UTxO carries
/// exactly one asset under this policy; we key the pool-value cache on that unit.
const DANOGO_LP_POLICY: &str = "d8b69fc53637bcfadbc4469083f706bc293f4d9d2296646c5ca167bb";

/// Withdraw-zero reward account used by every Danogo swap (detects CLMM txs even
/// when we haven't seen the prior pool UTxO yet).
const DANOGO_REWARD_ACCOUNT: &str = "stake178vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0wczh2pdw";

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
    /// full bech32 order address -> pool pair (VyFi per-pool order contracts)
    dyn_order_addrs: RwLock<HashMap<String, PoolPair>>,
    /// Minswap LP unit (policy||name) -> pool asset pair
    minswap_pools: Arc<RwLock<HashMap<String, PoolPair>>>,
    /// Danogo LP NFT unit -> last seen pool value (for direct-spend swap diffs)
    danogo_pools: Mutex<HashMap<String, Value>>,
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
    /// Placing user's stake address (preferred) or payment address.
    actor: Option<String>,
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

#[derive(Clone)]
struct WantedOut {
    qty: i128,
    /// True → display as ≥qty (limit / slippage floor).
    min: bool,
    policy: String,
    name_hex: String,
    /// False until the LP→pair cache resolved the out asset.
    resolved: bool,
    /// Minswap: re-resolve A→B wants once the pool cache warms.
    lp_unit: Option<String>,
    a_to_b: Option<bool>,
}

impl WantedOut {
    fn ada(qty: i128, min: bool) -> Self {
        Self {
            qty,
            min,
            policy: String::new(),
            name_hex: String::new(),
            resolved: true,
            lp_unit: None,
            a_to_b: None,
        }
    }

    fn token(qty: i128, min: bool, policy: String, name_hex: String) -> Self {
        Self {
            qty,
            min,
            policy,
            name_hex,
            resolved: true,
            lp_unit: None,
            a_to_b: None,
        }
    }

    fn from_ref(qty: i128, min: bool, asset: AssetRef) -> Self {
        Self::token(qty, min, asset.policy, asset.name_hex)
    }
}

fn attach_want(obj: &mut serde_json::Map<String, Value>, want: &Option<WantedOut>, filled: bool) {
    let Some(w) = want else { return };
    // Never publish an orphan qty without a named asset.
    if !w.resolved {
        return;
    }
    obj.insert("wantQty".into(), json!(w.qty.to_string()));
    // ≥ only on open orders — fills already executed, so a floor looks like a lie.
    obj.insert("wantMin".into(), json!(w.min && !filled));
    if w.policy.is_empty() && w.name_hex.is_empty() {
        obj.insert("wantAda".into(), json!(w.qty as u64));
    } else {
        let refs = [(w.policy.clone(), w.name_hex.clone(), w.qty)];
        obj.insert("want".into(), crate::parse::asset_list(&[&refs[0]]));
    }
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
        for (hash, dex) in POOL_SCRIPT_HASHES {
            pool_creds.insert((*hash).to_string(), *dex);
        }
        DexRegistry {
            order_creds,
            pool_creds,
            pool_nft_policies: POOL_NFT_POLICIES.iter().copied().collect(),
            pool_nft_units: POOL_NFT_UNITS.iter().copied().collect(),
            dyn_order_addrs: RwLock::new(HashMap::new()),
            minswap_pools: Arc::new(RwLock::new(HashMap::new())),
            danogo_pools: Mutex::new(HashMap::new()),
            tracker: Mutex::new(OrderTracker {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Scan every transaction in a block (two passes).
    ///
    /// Pass 1 collects all order placements; pass 2 resolves fills/cancels.
    /// That way a place in tx A and fill in tx B of the *same* block always
    /// collapse to one `Swap` — regardless of tx order in the block.
    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DexHit)> {
        let empty = Vec::new();
        // outpoint → (placement tx hash, order info)
        let mut deferred: HashMap<String, (String, OrderInfo)> = HashMap::new();

        // ── Pass 1: placements ───────────────────────────────────────────
        for &(tx_hash, tx) in txs {
            let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
            for (index, output) in outputs.iter().enumerate() {
                let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
                if self.pool_dex_for_output(output, addr).is_some() {
                    continue;
                }
                let matched = payment_credential(addr)
                    .and_then(|cred| self.order_creds.get(&cred).copied())
                    .or_else(|| {
                        self.dyn_order_addrs
                            .read()
                            .unwrap()
                            .contains_key(addr)
                            .then_some(("VyFinance", Role::Swap))
                    });
                if let Some((dex, role)) = matched {
                    let info = self.order_info(dex, role, output, tx);
                    let outpoint = format!("{tx_hash}#{index}");
                    deferred.insert(outpoint, (tx_hash.to_string(), info));
                }
            }
        }

        let mut hits: Vec<(String, DexHit)> = Vec::new();

        // ── Pass 2: fills / cancels ──────────────────────────────────────
        for &(tx_hash, tx) in txs {
            let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
            let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
            let mut pools_touched: HashSet<&'static str> = HashSet::new();
            for output in outputs {
                let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
                if let Some(dex) = self.pool_dex_for_output(output, addr) {
                    pools_touched.insert(dex);
                }
            }

            // Same-block: spend an order that was placed earlier in this block.
            for input in inputs {
                let Some(outpoint) = input_outpoint(input) else { continue };
                let Some((_place_tx, mut info)) = deferred.remove(&outpoint) else {
                    continue;
                };
                self.finalize_want(&mut info, Some(tx));
                hits.push((
                    tx_hash.to_string(),
                    settle_or_cancel(&info, &pools_touched, true),
                ));
            }

            // Prior-block orders spent by this tx.
            for (_outpoint, mut info) in self.take_consumed(inputs) {
                self.finalize_want(&mut info, Some(tx));
                hits.push((
                    tx_hash.to_string(),
                    settle_or_cancel(&info, &pools_touched, false),
                ));
            }

            // Danogo CLMM: direct pool spends (no order UTxO). Diff successive
            // pool UTxO values keyed by the LP NFT under the pool policy.
            if pools_touched.contains("Dano Finance") || danogo_withdrawal(tx) {
                if let Some(hit) = self.detect_danogo_swap(tx) {
                    hits.push((tx_hash.to_string(), hit));
                }
            }
        }

        // Still-open placements → pending order cards + track for later fills.
        let mut to_remember = Vec::with_capacity(deferred.len());
        for (outpoint, (place_tx, mut info)) in deferred {
            self.finalize_want(&mut info, None);
            hits.push((place_tx, order_hit(&info)));
            to_remember.push((outpoint, info));
        }
        self.remember_orders(to_remember);
        hits
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
                let key = input_outpoint(i)?;
                t.map.remove(&key).map(|info| (key, info))
            })
            .collect()
    }

    /// Fetch VyFi's per-pool order addresses (mainnet only) and refresh
    /// them every 6 hours. Failures are non-fatal - VyFi detection is
    /// simply inactive until a fetch succeeds.
    pub async fn refresh_vyfi_loop(self: std::sync::Arc<Self>) {
        loop {
            match fetch_vyfi_order_pools().await {
                Ok(map) if !map.is_empty() => {
                    tracing::info!("vyfi: loaded {} order addresses", map.len());
                    *self.dyn_order_addrs.write().unwrap() = map;
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
        let mut want = if matches!(side, "buy" | "sell" | "swap") {
            self.order_want(dex, output, tx)
        } else {
            None
        };
        // Ask must not equal a deposited asset (that bug paints TOKEN → TOKEN).
        if let Some(w) = want.as_ref() {
            if want_matches_deposit(&assets, w) {
                want = None;
            }
        }
        // Minswap sell whose pool mapping collapsed to the offered token: still
        // show B→A min-receive as ADA (asset A is ADA on every ADA pool).
        if want.is_none() && side == "sell" && dex == "Minswap" {
            if let Some((a_to_b, qty, min, _, _)) = parse_minswap_v2_order_datum(output, tx) {
                if !a_to_b && qty > 0 {
                    want = Some(WantedOut::ada(qty, min));
                }
            }
        }
        OrderInfo {
            dex,
            side,
            ada,
            assets: crate::parse::asset_list(&refs),
            want,
            // Change output on the place tx — not the order/pool script.
            actor: crate::parse::actor_from_tx(tx),
        }
    }

    fn order_want(&self, dex: &'static str, output: &Value, tx: &Value) -> Option<WantedOut> {
        match dex {
            "Minswap" => self
                .minswap_want(output, tx)
                .or_else(|| parse_minswap_v1_want(output, tx)),
            "SundaeSwap" => parse_sundae_want(output, tx)
                .or_else(|| parse_offer_ask_want(output, tx)),
            "VyFinance" => self.vyfi_want(output, tx),
            "WingRiders" => parse_wingriders_want(output, tx),
            "Splash" => parse_splash_want(output, tx),
            "CSWAP" => parse_cswap_want(output, tx),
            "GeniusYield" => parse_genius_yield_want(output, tx),
            "ChadSwap" => parse_offer_ask_want(output, tx),
            _ => None,
        }
    }

    /// VyFi datum has min-receive qty + action tag; the out asset comes from the
    /// order-address → pool-pair map fetched from VyFi's API.
    fn vyfi_want(&self, output: &Value, tx: &Value) -> Option<WantedOut> {
        let (tag, qty) = parse_vyfi_qty(output, tx)?;
        if tag == 4 {
            return Some(WantedOut::ada(qty, true));
        }
        let addr = output.get("address").and_then(Value::as_str)?;
        let pair = self.dyn_order_addrs.read().unwrap().get(addr).cloned()?;
        let ask = wr_ask_from_pair(output, &pair.asset_a, &pair.asset_b)?;
        Some(WantedOut::from_ref(qty, true, ask))
    }

    /// Read Minswap V2 order datum for min-receive + resolve the out asset
    /// via the LP→pair cache (filled lazily from Minswap's API).
    fn minswap_want(&self, output: &Value, tx: &Value) -> Option<WantedOut> {
        let (a_to_b, qty, min, lp_policy, lp_name) = parse_minswap_v2_order_datum(output, tx)?;
        let lp_unit = format!("{lp_policy}{lp_name}");
        let mut want = if let Some(p) = self.lookup_minswap_pool(&lp_unit) {
            let out = if a_to_b { p.asset_b } else { p.asset_a };
            WantedOut::from_ref(qty, min, out)
        } else if !a_to_b {
            // Minswap sorts ADA first; B→A always wants ADA without a pool lookup.
            WantedOut::ada(qty, min)
        } else {
            // A→B still needs the pool to name token B.
            WantedOut {
                qty,
                min,
                policy: String::new(),
                name_hex: String::new(),
                resolved: false,
                lp_unit: None,
                a_to_b: None,
            }
        };
        want.lp_unit = Some(lp_unit);
        want.a_to_b = Some(a_to_b);
        Some(want)
    }

    /// Resolve deferred Minswap asks; on fills, also infer from user outputs.
    fn finalize_want(&self, info: &mut OrderInfo, fill_tx: Option<&Value>) {
        self.try_resolve_want(info);
        if let Some(tx) = fill_tx {
            self.enrich_want_from_fill_tx(info, tx);
        }
    }

    /// Retry Minswap A→B want resolution if the pool cache has warmed since place.
    fn try_resolve_want(&self, info: &mut OrderInfo) {
        let Some(w) = info.want.as_mut() else { return };
        if w.resolved {
            return;
        }
        let (Some(lp), Some(a_to_b)) = (w.lp_unit.as_deref(), w.a_to_b) else {
            return;
        };
        let Some(p) = self.lookup_minswap_pool(lp) else { return };
        let out = if a_to_b { p.asset_b } else { p.asset_a };
        w.policy = out.policy;
        w.name_hex = out.name_hex;
        w.resolved = true;
    }

    /// When the LP→pair cache missed, recover the ask from what the fill tx
    /// actually paid to a user (key-payment) address.
    fn enrich_want_from_fill_tx(&self, info: &mut OrderInfo, tx: &Value) {
        if matches!(info.side, "deposit" | "redeem") {
            return;
        }
        if info.want.as_ref().is_some_and(|w| w.resolved) {
            return;
        }
        let Some((policy, name_hex, qty)) = self.user_fill_asset(tx, info) else {
            return;
        };
        let prev = info.want.take();
        info.want = Some(WantedOut {
            qty: if qty > 0 {
                qty
            } else {
                prev.as_ref().map(|w| w.qty).unwrap_or(0)
            },
            min: false,
            policy,
            name_hex,
            resolved: true,
            lp_unit: prev.as_ref().and_then(|w| w.lp_unit.clone()),
            a_to_b: prev.as_ref().and_then(|w| w.a_to_b),
        });
    }

    /// Largest non-LP native asset sent to a key-payment (user) address in this tx.
    fn user_fill_asset(&self, tx: &Value, info: &OrderInfo) -> Option<(String, String, i128)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);

        // Deposited units to ignore on sells (change / leftover).
        let mut deposited: HashSet<(String, String)> = HashSet::new();
        if info.side == "sell" {
            if let Some(items) = info.assets.get("items").and_then(Value::as_array) {
                for a in items {
                    if let (Some(p), Some(n)) = (
                        a.get("policy").and_then(Value::as_str),
                        a.get("nameHex").and_then(Value::as_str),
                    ) {
                        deposited.insert((p.to_string(), n.to_string()));
                    }
                }
            }
        }

        let mut totals: HashMap<(String, String), i128> = HashMap::new();
        for output in outputs {
            let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
            // Skip script-payment outs (DEX pools/orders and other contracts).
            if address_has_script_payment(addr) {
                continue;
            }
            let mut assets = Vec::new();
            crate::parse::collect_assets(output.get("value"), &mut assets);
            for (p, n, q) in assets {
                if is_lp_like_asset(&n) {
                    continue;
                }
                let key = (p, n);
                if deposited.contains(&key) {
                    continue;
                }
                *totals.entry(key).or_insert(0) += q;
            }
        }
        totals
            .into_iter()
            .max_by_key(|(_, q)| *q)
            .map(|((p, n), q)| (p, n, q))
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

    /// Prefetch Minswap V2 pool pairs (LP → asset_a/asset_b) so buy orders can
    /// show `₳ → ≥TOKEN` without waiting on a per-order API race.
    pub async fn refresh_minswap_pools_loop(self: std::sync::Arc<Self>) {
        loop {
            match warm_minswap_pool_cache(&self.minswap_pools).await {
                Ok(n) => tracing::info!("minswap: cached {n} pool pairs"),
                Err(e) => tracing::debug!("minswap pool warm failed: {e:#}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    }

    /// Diff a Danogo pool rewrite against the last seen value for its LP NFT.
    fn detect_danogo_swap(&self, tx: &Value) -> Option<DexHit> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let mut pool_out: Option<&Value> = None;
        for output in outputs {
            let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
            if self.pool_dex_for_output(output, addr) == Some("Dano Finance") {
                pool_out = Some(output);
                break;
            }
        }
        let pool_out = pool_out?;
        let value = pool_out.get("value")?;
        let lp_unit = danogo_lp_unit(value)?;
        let prev = {
            let mut cache = self.danogo_pools.lock().unwrap();
            let prev = cache.get(&lp_unit).cloned();
            cache.insert(lp_unit.clone(), value.clone());
            // Soft cap: drop the whole map rather than O(n) random eviction.
            if cache.len() > 4_000 {
                cache.clear();
                cache.insert(lp_unit.clone(), value.clone());
            }
            prev
        };
        let Some(prev) = prev else {
            // First sighting of this pool — seed cache only.
            return None;
        };
        let (paid_ada, paid_assets, want_ada, want_assets) = pool_value_diff(&prev, value);
        // LP deposit: pool gained both sides / redeem: lost both. Swaps move
        // one side in and the other out.
        let paid_n = paid_assets.len() + usize::from(paid_ada > 0);
        let want_n = want_assets.len() + usize::from(want_ada > 0);
        if paid_n == 0 || want_n == 0 {
            // Likely deposit, redeem, or fee-only touch.
            return None;
        }
        let side = if paid_assets.is_empty() && paid_ada > 0 {
            "buy"
        } else if want_assets.is_empty() && want_ada > 0 {
            "sell"
        } else {
            "swap"
        };
        let refs: Vec<&(String, String, i128)> = paid_assets.iter().collect();
        let mut data = serde_json::Map::new();
        data.insert("dex".into(), json!("Dano Finance"));
        data.insert("side".into(), json!(side));
        data.insert("filled".into(), json!(true));
        if paid_ada > 0 {
            data.insert("ada".into(), json!(paid_ada));
        }
        if !refs.is_empty() {
            data.insert("assets".into(), crate::parse::asset_list(&refs));
        }
        let want = if want_ada > 0 {
            Some(WantedOut::ada(want_ada as i128, false))
        } else if let Some((p, n, q)) = want_assets.first() {
            Some(WantedOut::token(*q, false, p.clone(), n.clone()))
        } else {
            None
        };
        attach_want(&mut data, &want, true);
        crate::parse::attach_actor(&mut data, crate::parse::actor_from_tx(tx).as_deref());
        Some(DexHit {
            kind: "dex_fill",
            title: "Swap - Dano Finance".into(),
            data: Value::Object(data),
        })
    }
}

fn is_lp_side(side: &str) -> bool {
    matches!(side, "deposit" | "redeem")
}

/// Order-book DEXes settle peer-to-peer (no pool NFT touch).
fn is_orderbook_dex(dex: &str) -> bool {
    matches!(dex, "GeniusYield" | "ChadSwap")
}

fn settle_or_cancel(
    info: &OrderInfo,
    pools_touched: &HashSet<&'static str>,
    same_block: bool,
) -> DexHit {
    if pools_touched.contains(info.dex) {
        fill_hit(info, same_block)
    } else if is_orderbook_dex(info.dex) {
        // Peer fill (or cancel — we can't see the redeemer cheaply).
        fill_hit(info, false)
    } else {
        cancel_hit(info)
    }
}

fn danogo_withdrawal(tx: &Value) -> bool {
    tx.get("withdrawals")
        .and_then(Value::as_object)
        .map(|w| w.contains_key(DANOGO_REWARD_ACCOUNT))
        .unwrap_or(false)
}

fn danogo_lp_unit(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    let assets = obj.get(DANOGO_LP_POLICY)?.as_object()?;
    let (name, _) = assets.iter().next()?;
    Some(format!("{DANOGO_LP_POLICY}{name}"))
}

/// Assets the pool *gained* (user paid) and *lost* (user received).
fn pool_value_diff(
    prev: &Value,
    next: &Value,
) -> (u64, Vec<(String, String, i128)>, u64, Vec<(String, String, i128)>) {
    let prev_ada = output_lovelace_from_value(prev);
    let next_ada = output_lovelace_from_value(next);
    let mut prev_map: HashMap<(String, String), i128> = HashMap::new();
    let mut next_map: HashMap<(String, String), i128> = HashMap::new();
    let mut prev_assets = Vec::new();
    let mut next_assets = Vec::new();
    crate::parse::collect_assets(Some(prev), &mut prev_assets);
    crate::parse::collect_assets(Some(next), &mut next_assets);
    for (p, n, q) in prev_assets {
        if p == DANOGO_LP_POLICY {
            continue; // ignore LP NFT itself
        }
        prev_map.insert((p, n), q);
    }
    for (p, n, q) in next_assets {
        if p == DANOGO_LP_POLICY {
            continue;
        }
        next_map.insert((p, n), q);
    }
    let mut paid = Vec::new();
    let mut want = Vec::new();
    let mut keys: HashSet<(String, String)> = prev_map.keys().cloned().collect();
    keys.extend(next_map.keys().cloned());
    for k in keys {
        let a = prev_map.get(&k).copied().unwrap_or(0);
        let b = next_map.get(&k).copied().unwrap_or(0);
        if b > a {
            paid.push((k.0, k.1, b - a));
        } else if a > b {
            want.push((k.0, k.1, a - b));
        }
    }
    let paid_ada = next_ada.saturating_sub(prev_ada);
    let want_ada = prev_ada.saturating_sub(next_ada);
    (paid_ada, paid, want_ada, want)
}

fn output_lovelace_from_value(value: &Value) -> u64 {
    value
        .get("ada")
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
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
    let obj = data.as_object_mut().unwrap();
    attach_want(obj, &info.want, filled);
    crate::parse::attach_actor(obj, info.actor.as_deref());
    data
}

fn input_outpoint(input: &Value) -> Option<String> {
    let txid = input.get("transaction")?.get("id")?.as_str()?;
    let index = input.get("index")?.as_u64()?;
    Some(format!("{txid}#{index}"))
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

fn address_has_script_payment(addr: &str) -> bool {
    crate::parse::address_has_script_payment(addr)
}

async fn fetch_vyfi_order_pools() -> anyhow::Result<HashMap<String, PoolPair>> {
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
        let Some(pair) = parse_vyfi_units_pair(units) else {
            continue;
        };
        map.insert(addr, pair);
    }
    Ok(map)
}

fn parse_vyfi_units_pair(units: &str) -> Option<PoolPair> {
    let (a, b) = units.split_once('/')?;
    Some(PoolPair {
        asset_a: parse_vyfi_unit(a)?,
        asset_b: parse_vyfi_unit(b)?,
    })
}

fn parse_vyfi_unit(unit: &str) -> Option<AssetRef> {
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

async fn fetch_minswap_pool_pair(lp_unit: &str) -> anyhow::Result<PoolPair> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let url = format!("https://api-mainnet-prod.minswap.org/v1/pools/{lp_unit}/metrics");
    let v: Value = client.get(url).send().await?.error_for_status()?.json().await?;
    parse_minswap_pool_metrics(&v)
}

fn parse_minswap_pool_metrics(v: &Value) -> anyhow::Result<PoolPair> {
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
async fn warm_minswap_pool_cache(
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
                if let Ok(pair) = parse_minswap_pool_metrics(row) {
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
            lp_unit: None,
            a_to_b: None,
        });
    }
    None
}

/// VyFinance: `Constr 0 [ poolId(56), Constr tag [ minReceive ] ]`.
/// Tag 4 (observed) = sell token for ADA. Other tags need the order→pair map.
fn parse_vyfi_qty(output: &Value, tx: &Value) -> Option<(u64, i128)> {
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

/// Minswap V1 batch order: out asset + min receive are inline in the datum
/// (no LP→pair lookup needed). See CatspersCoffee BatchOrder Types.
fn parse_minswap_v1_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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
        lp_unit: None,
        a_to_b: None,
    })
}

/// CSWAP order: `Constr 0 [ addr, target_assets, input_assets, otype, … ]`
/// where each target is `[policy, name, minQty]` (ADA = empty bytes).
fn parse_cswap_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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

/// Genius Yield partial order: out asset + price × leftover offer ≈ min receive.
fn parse_genius_yield_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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

/// WingRiders V1/V2: datum carries the **pool pair** (A, B) + min receive —
/// not offer/ask. Ask = the pool side that is *not* sitting in the order UTxO.
/// V2 flat layout: `[deposit, dest…, expiration, APol, AName, BPol, BName, minConstr, …]`
/// V1 nested layout: `[… poolPair [A,B] …, minConstr]`.
fn parse_wingriders_want(output: &Value, tx: &Value) -> Option<WantedOut> {
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

/// True when the parsed ask is the same token sitting in the order UTxO (nonsense for a swap).
fn want_matches_deposit(assets: &[(String, String, i128)], want: &WantedOut) -> bool {
    if want.policy.is_empty() && want.name_hex.is_empty() {
        return false; // ADA out is fine even with ADA collateral in the UTxO
    }
    assets
        .iter()
        .any(|(pol, name, _)| pol == &want.policy && name == &want.name_hex)
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
                    lp_unit: None,
                    a_to_b: None,
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
        parent_id: None,
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
        let want = parse_wingriders_want(&output, &json!({})).expect("want");
        assert!(want.resolved);
        assert!(want.policy.is_empty() && want.name_hex.is_empty(), "should want ADA");
        assert_eq!(want.qty, 1_000_000_000);
    }

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
        let want = parse_minswap_v1_want(&output, &tx).expect("want");
        assert!(want.resolved);
        assert_eq!(
            want.policy,
            "279c909f348e533da5808898f87f9a14bb2c3dfbbacccd631d927a3f"
        );
        assert_eq!(want.name_hex, "534e454b"); // SNEK
        assert_eq!(want.qty, 0x47e85);
    }
}


