//! Shared DEX detection engine.
//!
//! Per-DEX contract constants and datum parsers live in sibling modules
//! (`minswap`, `sundaeswap`, `dano`, …); this module owns everything that is
//! common to all of them: the cross-block order tracker, pool-NFT matching,
//! hit/event construction, and a minimal Plutus-Data CBOR reader.
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
//!    can show both sides, e.g. `₳ 73 → ≥16,490 USDCx`. Same-block place+fill
//!    collapses to Swap. Liquidity contracts (and any UTxO carrying LP-like
//!    assets) emit a separate `dex_lp` event: deposit (ADA+tokens in) or
//!    redeem (LP shares in).
//! 2. **Fills** - batcher transactions recreate the pool UTxO. A tx that
//!    spends a tracked order *and* touches a pool emits one `dex_fill` (or
//!    filled `dex_lp`) per order. Order-book DEXes (Genius Yield, ChadSwap)
//!    settle peer-to-peer, so any spend of a tracked order counts as a fill.
//! 3. **Cancellations** - a tx that spends a tracked order *without* touching
//!    a pool is a cancel (batcher DEXes only).
//! 4. **Direct-pool swaps** - Dano Finance CLMM has no order UTxO; see `dano`.
//!
//! Contract constants were sourced from the open-source Iris DEX indexer
//! (IndigoProtocol/iris), Charli3 Dendrite, Danogo docs, and each DEX's own
//! published deployments.

use super::{
    chadswap, cswap, dano, geniusyield, minswap, muesliswap, splash, sundaeswap, vyfinance,
    wingriders,
};
use crate::model::ChainEvent;
use crate::parse::{address_has_script_payment, payment_credential};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// Every DEX module contributing order/pool constants to the registry.
/// Keep in sync with `DEX_VENUES` in `static/dex/mod.js`.
macro_rules! for_each_dex {
    ($mac:ident) => {
        $mac!(minswap);
        $mac!(sundaeswap);
        $mac!(wingriders);
        $mac!(muesliswap);
        $mac!(splash);
        $mac!(vyfinance);
        $mac!(cswap);
        $mac!(geniusyield);
        $mac!(chadswap);
        $mac!(dano);
    };
}

/// What a matched order/batch script is for.
#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    Swap,
    Liquidity,
}

const ORDER_TRACKER_CAP: usize = 30_000;

pub struct DexRegistry {
    /// payment credential (28-byte hex) -> (dex, role)
    order_creds: HashMap<String, (&'static str, Role)>,
    pool_creds: HashMap<String, &'static str>,
    pool_nft_policies: HashMap<&'static str, &'static str>,
    pool_nft_units: HashMap<&'static str, &'static str>,
    /// (policy, asset-name prefix) -> dex, for policies shared with LP tokens
    pool_nft_prefixes: Vec<(&'static str, &'static str, &'static str)>,
    /// VyFi per-pool order contracts, refreshed from their API.
    vyfinance: vyfinance::OrderAddrs,
    /// Minswap LP unit (policy||name) -> pool asset pair
    minswap: minswap::PoolCache,
    /// Dano Finance CLMM pool values, for direct-spend swap diffs
    dano: dano::PoolTracker,
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
pub struct AssetRef {
    pub policy: String,
    pub name_hex: String,
}

#[derive(Clone)]
pub struct PoolPair {
    pub asset_a: AssetRef,
    pub asset_b: AssetRef,
}

#[derive(Clone)]
pub struct WantedOut {
    pub qty: i128,
    /// True → display as ≥qty (limit / slippage floor).
    pub min: bool,
    pub policy: String,
    pub name_hex: String,
    /// False until the LP→pair cache resolved the out asset.
    pub resolved: bool,
    /// Minswap: re-resolve A→B wants once the pool cache warms.
    pub lp_unit: Option<String>,
    pub a_to_b: Option<bool>,
}

impl WantedOut {
    pub fn ada(qty: i128, min: bool) -> Self {
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

    pub fn token(qty: i128, min: bool, policy: String, name_hex: String) -> Self {
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

    pub fn from_ref(qty: i128, min: bool, asset: AssetRef) -> Self {
        Self::token(qty, min, asset.policy, asset.name_hex)
    }

    /// Quantity known, out asset not yet named (Minswap A→B before the
    /// LP→pair cache warms).
    pub fn unresolved(qty: i128, min: bool) -> Self {
        Self {
            qty,
            min,
            policy: String::new(),
            name_hex: String::new(),
            resolved: false,
            lp_unit: None,
            a_to_b: None,
        }
    }
}

pub(super) fn attach_want(obj: &mut serde_json::Map<String, Value>, want: &Option<WantedOut>, filled: bool) {
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
        let mut pool_creds = HashMap::new();
        let mut pool_nft_policies = HashMap::new();
        let mut pool_nft_units = HashMap::new();
        let mut pool_nft_prefixes = Vec::new();

        macro_rules! register {
            ($m:ident) => {
                for (addr, role) in $m::ORDER_ADDRESSES {
                    if let Some(cred) = payment_credential(addr) {
                        order_creds.insert(cred, ($m::NAME, *role));
                    }
                }
                for (hash, role) in $m::ORDER_SCRIPT_HASHES {
                    order_creds.insert((*hash).to_string(), ($m::NAME, *role));
                }
                for addr in $m::POOL_ADDRESSES {
                    if let Some(cred) = payment_credential(addr) {
                        pool_creds.insert(cred, $m::NAME);
                    }
                }
                for hash in $m::POOL_SCRIPT_HASHES {
                    pool_creds.insert((*hash).to_string(), $m::NAME);
                }
                for policy in $m::POOL_NFT_POLICIES {
                    pool_nft_policies.insert(*policy, $m::NAME);
                }
                for unit in $m::POOL_NFT_UNITS {
                    pool_nft_units.insert(*unit, $m::NAME);
                }
                for (policy, prefix) in $m::POOL_NFT_PREFIXES {
                    pool_nft_prefixes.push((*policy, *prefix, $m::NAME));
                }
            };
        }
        for_each_dex!(register);

        DexRegistry {
            order_creds,
            pool_creds,
            pool_nft_policies,
            pool_nft_units,
            pool_nft_prefixes,
            vyfinance: vyfinance::OrderAddrs::new(),
            minswap: minswap::PoolCache::new(),
            dano: dano::PoolTracker::new(),
            tracker: Mutex::new(OrderTracker {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    pub fn vyfinance(&self) -> &vyfinance::OrderAddrs {
        &self.vyfinance
    }

    pub fn minswap(&self) -> &minswap::PoolCache {
        &self.minswap
    }

    /// Like [`Self::new`], then rebuild the open-order tracker from restored
    /// `{ tx, block }` cache entries.
    ///
    /// The tracker is memory-only, so without this a restart drops every order
    /// placed before it: the later settlement spends an outpoint we no longer
    /// recognise and silently produces no `dex_fill`. Orders sit open for tens
    /// of blocks, so that lost window is real (batcher DEXes fill minutes after
    /// placement).
    pub fn with_restored_txs(entries: &[(String, Value)]) -> Self {
        let reg = Self::new();
        reg.warm_from_tx_entries(entries);
        reg
    }

    /// Re-track orders that are still open at the cache tip. Emits nothing:
    /// an order whose settlement is *also* in the cache was already reported
    /// when it happened, so replaying it would duplicate history.
    pub fn warm_from_tx_entries(&self, entries: &[(String, Value)]) {
        // `cached_tx_entries` iterates a HashMap, so impose block order —
        // the tracker evicts oldest-first once it hits ORDER_TRACKER_CAP.
        let mut ordered: Vec<&(String, Value)> = entries.iter().collect();
        ordered.sort_by_key(|(_, e)| {
            let b = e.get("block");
            (
                b.and_then(|b| b.get("height")).and_then(Value::as_u64),
                b.and_then(|b| b.get("slot")).and_then(Value::as_u64),
            )
        });

        let mut open: Vec<(String, OrderInfo)> = Vec::new();
        let mut spent: HashSet<String> = HashSet::new();
        for (tx_hash, entry) in ordered {
            let Some(tx) = entry.get("tx") else { continue };
            open.extend(self.placements_in_tx(tx_hash, tx));
            if let Some(inputs) = tx.get("inputs").and_then(Value::as_array) {
                spent.extend(inputs.iter().filter_map(input_outpoint));
            }
        }

        // Order-independent: subtracting every outpoint spent anywhere in the
        // cache leaves exactly those still unspent, whatever the replay order.
        let restored: Vec<(String, OrderInfo)> = open
            .into_iter()
            .filter(|(outpoint, _)| !spent.contains(outpoint))
            .map(|(outpoint, mut info)| {
                self.finalize_want(&mut info, None);
                (outpoint, info)
            })
            .collect();
        if !restored.is_empty() {
            tracing::info!("dex: restored {} open orders from tx cache", restored.len());
        }
        self.remember_orders(restored);
    }

    /// Outputs in this tx that pay a known order/batch script: `(outpoint, info)`.
    /// Shared by live scanning and cache warming so both agree on what counts
    /// as a placement.
    fn placements_in_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, OrderInfo)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let mut out = Vec::new();
        for (index, output) in outputs.iter().enumerate() {
            let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
            if self.pool_dex_for_output(output, addr).is_some() {
                continue;
            }
            let matched = payment_credential(addr)
                .and_then(|cred| self.order_creds.get(&cred).copied())
                .or_else(|| {
                    self.vyfinance
                        .contains(addr)
                        .then_some((vyfinance::NAME, Role::Swap))
                });
            if let Some((dex, role)) = matched {
                let info = self.order_info(dex, role, output, tx);
                out.push((format!("{tx_hash}#{index}"), info));
            }
        }
        out
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
            for (outpoint, info) in self.placements_in_tx(tx_hash, tx) {
                deferred.insert(outpoint, (tx_hash.to_string(), info));
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

            // Dano Finance CLMM: direct pool spends (no order UTxO). Diff
            // successive pool UTxO values keyed by the LP NFT.
            if pools_touched.contains(dano::NAME) || dano::PoolTracker::is_swap_tx(tx) {
                if let Some(pool_out) = outputs.iter().find(|o| {
                    let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                    self.pool_dex_for_output(o, addr) == Some(dano::NAME)
                }) {
                    if let Some(hit) = self.dano.detect_swap(tx, pool_out) {
                        hits.push((tx_hash.to_string(), hit));
                    }
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
                for (p, prefix, dex) in &self.pool_nft_prefixes {
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
        // UTxOs often look like a one-token "sell" (e.g. ADA+USDCx deposit).
        let minswap_lp = (dex == minswap::NAME)
            .then(|| minswap::lp_side(output, tx))
            .flatten();

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
        if want.is_none() && side == "sell" && dex == minswap::NAME {
            if let Some((a_to_b, qty, min, _, _)) = minswap::parse_v2_order_datum(output, tx) {
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
            minswap::NAME => self
                .minswap
                .want(output, tx)
                .or_else(|| minswap::parse_v1_want(output, tx)),
            sundaeswap::NAME => sundaeswap::want(output, tx),
            vyfinance::NAME => self.vyfinance.want(output, tx),
            wingriders::NAME => wingriders::want(output, tx),
            splash::NAME => splash::want(output, tx),
            cswap::NAME => cswap::want(output, tx),
            geniusyield::NAME => geniusyield::want(output, tx),
            chadswap::NAME => chadswap::want(output, tx),
            muesliswap::NAME => muesliswap::want(output, tx),
            dano::NAME => dano::want(output, tx),
            _ => None,
        }
    }

    /// Resolve deferred Minswap asks; on fills, also infer from user outputs.
    fn finalize_want(&self, info: &mut OrderInfo, fill_tx: Option<&Value>) {
        self.try_resolve_want(info);
        if let Some(tx) = fill_tx {
            self.enrich_want_from_fill_tx(info, tx);
            // Place tx sometimes has no key-payment change (script wallets /
            // batchers). Recover the user from the fill/settlement tx.
            if info.actor.is_none() {
                info.actor = crate::parse::actor_from_tx(tx);
            }
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
        let Some(p) = self.minswap.lookup(lp) else { return };
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

}

fn is_lp_side(side: &str) -> bool {
    matches!(side, "deposit" | "redeem")
}

/// Order-book DEXes settle peer-to-peer (no pool NFT touch).
fn is_orderbook_dex(dex: &str) -> bool {
    macro_rules! check {
        ($m:ident) => {
            if $m::ORDERBOOK && dex == $m::NAME {
                return true;
            }
        };
    }
    for_each_dex!(check);
    false
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



/// True when the parsed ask is the same token sitting in the order UTxO (nonsense for a swap).
fn want_matches_deposit(assets: &[(String, String, i128)], want: &WantedOut) -> bool {
    if want.policy.is_empty() && want.name_hex.is_empty() {
        return false; // ADA out is fine even with ADA collateral in the UTxO
    }
    assets
        .iter()
        .any(|(pol, name, _)| pol == &want.policy && name == &want.name_hex)
}

/// Scoop / Sundae V3 style: a constr holding two `[policy, name, qty]` arrays
/// (offer, ask). The ask is what the order wants to receive.
pub(super) fn parse_offer_ask_want(output: &Value, tx: &Value) -> Option<WantedOut> {
    let bytes = output_datum_cbor(output, Some(tx))?;
    let mut r = CborReader { bytes: &bytes, i: 0 };
    let root = r.decode()?;
    offer_ask_from_val(&root)
}

pub(super) fn offer_ask_from_val(v: &CborVal) -> Option<WantedOut> {
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

pub(super) fn option_int(v: &CborVal) -> Option<i128> {
    // Aiken Option: Some = Constr 0 [x], None = Constr 1 []
    match v.as_constr() {
        Some((0, fields)) => fields.first()?.as_int(),
        Some((1, _)) => None,
        _ => v.as_int(), // bare int fallback
    }
}

pub(super) fn output_datum_cbor(output: &Value, tx: Option<&Value>) -> Option<Vec<u8>> {
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
pub(super) struct CborReader<'a> {
    pub bytes: &'a [u8],
    pub i: usize,
}

#[derive(Debug, Clone)]
pub(super) enum CborVal {
    Int(i128),
    Bytes(Vec<u8>),
    Array(Vec<CborVal>),
    Constr { tag: u64, fields: Vec<CborVal> },
    Bool(bool),
    Other,
}

impl CborVal {
    pub fn as_constr(&self) -> Option<(u64, &Vec<CborVal>)> {
        match self {
            CborVal::Constr { tag, fields } => Some((*tag, fields)),
            _ => None,
        }
    }
    pub fn constr_fields(&self, expect_tag: u64) -> Option<&Vec<CborVal>> {
        let (tag, fields) = self.as_constr()?;
        (tag == expect_tag).then_some(fields)
    }
    pub fn as_array(&self) -> Option<&Vec<CborVal>> {
        match self {
            CborVal::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i128> {
        match self {
            CborVal::Int(n) => Some(*n),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            CborVal::Bytes(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
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

    pub fn decode(&mut self) -> Option<CborVal> {
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
        category: "finance".into(),
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

/// Drive the background caches the DEX modules depend on (mainnet only).
/// Both are best-effort: a failing fetch just leaves that DEX less detailed.
pub async fn refresh_dex_caches(reg: std::sync::Arc<DexRegistry>) {
    tokio::spawn(minswap::PoolCache::refresh_loop(reg.minswap().pools()));
    vyfinance::refresh_loop(reg).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CSWAP_ORDER: &str = "addr1z8d9k3aw6w24eyfjacy809h68dv2rwnpw0arrfau98jk6nhv88awp8sgxk65d6kry0mar3rd0dlkfljz7dv64eu39vfs38yd9p";
    const CSWAP_POOL: &str = "addr1z8ke0c9p89rjfwmuh98jpt8ky74uy5mffjft3zlcld9h7ml3lmln3mwk0y3zsh3gs3dzqlwa9rjzrxawkwm4udw9axhs6fuu6e";

    /// A cached `{ block, tx }` entry placing an order at `outpoint tx_id#0`.
    fn placement(tx_id: &str, height: u64) -> (String, Value) {
        (
            tx_id.to_string(),
            json!({
                "block": { "height": height, "slot": height },
                "tx": {
                    "id": tx_id,
                    "inputs": [],
                    "outputs": [{
                        "address": CSWAP_ORDER,
                        "value": { "ada": { "lovelace": 100_000_000u64 } },
                    }],
                },
            }),
        )
    }

    /// Settlement: spends `tx_id#0` and rebuilds the pool UTxO.
    fn settlement(tx_id: &str) -> Value {
        json!({
            "inputs": [{ "transaction": { "id": tx_id }, "index": 0 }],
            "outputs": [{
                "address": CSWAP_POOL,
                "value": { "ada": { "lovelace": 5_000_000_000u64 } },
            }],
        })
    }

    fn fills(reg: &DexRegistry, tx: &Value) -> Vec<String> {
        reg.scan_block(&[("settle_tx", tx)])
            .into_iter()
            .filter(|(_, h)| h.kind == "dex_fill")
            .map(|(_, h)| h.title)
            .collect()
    }

    /// The tracker is memory-only, so a restart used to drop every order placed
    /// before it and silently emit no fill when it later settled.
    #[test]
    fn cold_registry_cannot_attribute_a_prior_block_fill() {
        let reg = DexRegistry::new();
        assert!(fills(&reg, &settlement("order_tx")).is_empty());
    }

    #[test]
    fn warming_from_cache_restores_prior_block_fill_attribution() {
        let reg = DexRegistry::with_restored_txs(&[placement("order_tx", 100)]);
        assert_eq!(fills(&reg, &settlement("order_tx")), ["Order Fill - CSWAP"]);
    }

    /// An order already settled inside the cache window must not be re-tracked,
    /// or its fill would be reported a second time after every restart.
    #[test]
    fn warming_skips_orders_already_spent_within_the_cache() {
        let mut spent = placement("order_tx", 100);
        let settle = (
            "settle_in_cache".to_string(),
            json!({
                "block": { "height": 101, "slot": 101 },
                "tx": settlement("order_tx"),
            }),
        );
        spent.1["block"]["height"] = json!(100);
        let reg = DexRegistry::with_restored_txs(&[spent, settle]);
        assert!(fills(&reg, &settlement("order_tx")).is_empty());
    }
}
