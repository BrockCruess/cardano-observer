//! Surf Lending (formerly Flow Lending) on-chain activity detection.
//!
//! Sources:
//! - https://docs.surflending.org/
//! - https://github.com/flow-lending/flow-lending-smart-contracts (`plutus.json`)
//!
//! Validators are parameterized per pool, so script hashes differ by market.
//! Detection keys off fixed asset *names* from the contracts:
//! - `POOL_NFT` / `POOL_INFO_NFT` — pool identity (create / apply / close)
//! - `VAULT_AT` — vault authorization token (borrow open / repay|liquidate)
//!
//! Amounts are always **net pool deltas** from a tracked prior `POOL_NFT` UTxO
//! (same lesson as Indigo SP deposits). Full pool balances and pool rewrites
//! with no user mint signal are never emitted.

use super::DappHit;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "Surf";

/// Pool identity NFT asset name (`POOL_NFT`).
const POOL_NFT: &str = "504f4f4c5f4e4654";
/// Pool-info identity NFT (`POOL_INFO_NFT`).
const POOL_INFO_NFT: &str = "504f4f4c5f494e464f5f4e4654";
/// Vault authorization token (`VAULT_AT`).
const VAULT_AT: &str = "5641554c545f4154";

const TRACKER_CAP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    CreatePool,
    ClosePool,
    Supply,
    Withdraw,
    Borrow,
    Repay,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::CreatePool => "create_pool",
            Self::ClosePool => "close_pool",
            Self::Supply => "supply",
            Self::Withdraw => "withdraw",
            Self::Borrow => "borrow",
            Self::Repay => "repay",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::CreatePool => "Create Lending Pool - Surf",
            Self::ClosePool => "Close Lending Pool - Surf",
            // LPs supply the pool’s lendable asset (mint fTokens); not stake pools.
            Self::Supply => "Supply Liquidity - Surf",
            Self::Withdraw => "Withdraw Liquidity - Surf",
            // Surf docs: borrow opens a loan / “Borrow Position”.
            Self::Borrow => "Open Loan - Surf",
            // Vault-auth burn covers full repay and liquidation.
            Self::Repay => "Repay Loan - Surf",
        }
    }
}

#[derive(Default, Clone, Debug)]
struct MintSummary {
    pool_nft: i64,
    pool_info_nft: i64,
    vault_at: i64,
    other_mints: Vec<(String, String, i128)>,
    other_burns: Vec<(String, String, i128)>,
}

/// ADA + native assets on a pool UTxO (auth NFTs stripped).
#[derive(Clone, Debug, Default)]
struct PoolValue {
    ada: u64,
    /// policy → name_hex → qty
    assets: HashMap<String, HashMap<String, u64>>,
}

impl PoolValue {
    fn from_value(value: Option<&Value>) -> Self {
        let mut pv = Self::default();
        let Some(obj) = value.and_then(Value::as_object) else {
            return pv;
        };
        if let Some(ada) = obj
            .get("ada")
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
        {
            pv.ada = ada;
        }
        for (policy, names) in obj {
            if policy == "ada" {
                continue;
            }
            let Some(names) = names.as_object() else { continue };
            for (name, qty) in names {
                if name == POOL_NFT || name == POOL_INFO_NFT || name == VAULT_AT {
                    continue;
                }
                let q = qty.as_u64().unwrap_or(0);
                if q == 0 {
                    continue;
                }
                pv.assets
                    .entry(policy.clone())
                    .or_default()
                    .insert(name.clone(), q);
            }
        }
        pv
    }

    fn merge_from(&mut self, other: &Self) {
        self.ada = self.ada.saturating_add(other.ada);
        for (policy, names) in &other.assets {
            let entry = self.assets.entry(policy.clone()).or_default();
            for (name, qty) in names {
                let e = entry.entry(name.clone()).or_insert(0);
                *e = e.saturating_add(*qty);
            }
        }
    }
}

/// Signed net change on the pool UTxO (`out − spent`). `None` if prior unknown.
#[derive(Clone, Debug, Default)]
struct PoolNet {
    known: bool,
    ada: i128,
    assets: Vec<(String, String, i128)>,
}

impl PoolNet {
    fn from_flows(spent: Option<PoolValue>, out: PoolValue) -> Self {
        let Some(spent) = spent else {
            return Self {
                known: false,
                ..Self::default()
            };
        };
        let ada = out.ada as i128 - spent.ada as i128;
        let mut keys: HashSet<(String, String)> = HashSet::new();
        for (p, names) in spent.assets.iter().chain(out.assets.iter()) {
            for n in names.keys() {
                keys.insert((p.clone(), n.clone()));
            }
        }
        let mut assets = Vec::new();
        for (p, n) in keys {
            let o = out
                .assets
                .get(&p)
                .and_then(|m| m.get(&n))
                .copied()
                .unwrap_or(0) as i128;
            let s = spent
                .assets
                .get(&p)
                .and_then(|m| m.get(&n))
                .copied()
                .unwrap_or(0) as i128;
            let d = o - s;
            if d != 0 {
                assets.push((p, n, d));
            }
        }
        assets.sort_by(|a, b| b.2.unsigned_abs().cmp(&a.2.unsigned_abs()));
        Self {
            known: true,
            ada,
            assets,
        }
    }

    /// Positive side for supply / repay (assets flowing into the pool).
    fn positive_side(&self) -> (u64, Vec<(String, String, i128)>) {
        let ada = if self.ada > 0 { self.ada as u64 } else { 0 };
        let assets = self
            .assets
            .iter()
            .filter(|(_, _, q)| *q > 0)
            .cloned()
            .collect();
        (ada, assets)
    }

    /// Absolute negative side for withdraw / borrow (assets leaving the pool).
    fn negative_side(&self) -> (u64, Vec<(String, String, i128)>) {
        let ada = if self.ada < 0 {
            (-self.ada) as u64
        } else {
            0
        };
        let assets = self
            .assets
            .iter()
            .filter(|(_, _, q)| *q < 0)
            .map(|(p, n, q)| (p.clone(), n.clone(), -q))
            .collect();
        (ada, assets)
    }
}

struct TrackedPools {
    map: HashMap<String, PoolValue>,
    order: VecDeque<String>,
}

impl TrackedPools {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, outpoint: String, value: PoolValue) {
        if self.map.insert(outpoint.clone(), value).is_none() {
            self.order.push_back(outpoint);
            while self.order.len() > TRACKER_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }

    fn take(&mut self, outpoint: &str) -> Option<PoolValue> {
        self.map.remove(outpoint)
    }
}

/// Stateful Surf scanner (tracks pool UTxOs for net flows + spend-graph hubs).
pub struct Scanner {
    /// Outpoints carrying `POOL_NFT` — shared pool state, omitted from light-cone.
    graph_hubs: Mutex<HashSet<String>>,
    /// Prior pool balances for net-delta amounts.
    pools: Mutex<TrackedPools>,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            graph_hubs: Mutex::new(HashSet::new()),
            pools: Mutex::new(TrackedPools::new()),
        }
    }

    pub fn is_spend_graph_hub(&self, outpoint: &str) -> bool {
        self.graph_hubs
            .lock()
            .map(|h| h.contains(outpoint))
            .unwrap_or(false)
    }

    pub fn is_spend_graph_hub_address(&self, _addr: &str) -> bool {
        // Hubs are outpoint-based (POOL_NFT); address alone is not enough
        // because each pool uses a distinct parameterized script.
        false
    }

    pub fn note_spend_graph_hubs(&self, tx_hash: &str, tx: &Value) {
        let empty = Vec::new();
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let Ok(mut hubs) = self.graph_hubs.lock() else {
            return;
        };
        for input in inputs {
            if let Some(op) = input_outpoint(input) {
                hubs.remove(&op);
            }
        }
        for (index, o) in outputs.iter().enumerate() {
            if value_has_named_asset(o.get("value"), POOL_NFT) {
                hubs.insert(format!("{tx_hash}#{index}"));
            }
        }
        while hubs.len() > TRACKER_CAP {
            if let Some(old) = hubs.iter().next().cloned() {
                hubs.remove(&old);
            } else {
                break;
            }
        }
    }

    pub fn warm_from_tx_entries(&self, entries: &[(String, Value)]) {
        if entries.is_empty() {
            return;
        }
        let mut ordered: Vec<(u64, &str, &Value)> = entries
            .iter()
            .filter_map(|(hash, entry)| {
                let tx = entry.get("tx")?;
                let slot = entry
                    .get("block")
                    .and_then(|b| b.get("slot"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                Some((slot, hash.as_str(), tx))
            })
            .collect();
        ordered.sort_by_key(|(slot, hash, _)| (*slot, *hash));
        let n = ordered.len();
        for (_, hash, tx) in ordered {
            let _ = self.scan_tx_inner(hash, tx, false);
        }
        tracing::info!("surf: warmed pool UTxO tracker from {n} cached txs");
    }

    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        let mut hits = Vec::new();
        for &(tx_hash, tx) in txs {
            hits.extend(self.scan_tx(tx_hash, tx));
        }
        hits
    }

    fn scan_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, DappHit)> {
        self.scan_tx_inner(tx_hash, tx, true)
    }

    fn scan_tx_inner(&self, tx_hash: &str, tx: &Value, emit: bool) -> Vec<(String, DappHit)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
        let mint = summarize_mint(tx.get("mint"));

        let mut pool_out = PoolValue::default();
        let mut has_pool_out = false;
        for o in outputs {
            if value_has_named_asset(o.get("value"), POOL_NFT) {
                has_pool_out = true;
                pool_out.merge_from(&PoolValue::from_value(o.get("value")));
            }
        }

        let spent_pool = {
            let mut pools = self.pools.lock().unwrap();
            let mut spent: Option<PoolValue> = None;
            for input in inputs {
                let Some(op) = input_outpoint(input) else { continue };
                if let Some(v) = pools.take(&op) {
                    match &mut spent {
                        Some(acc) => acc.merge_from(&v),
                        None => spent = Some(v),
                    }
                }
            }
            spent
        };
        let pool_net = PoolNet::from_flows(spent_pool, pool_out);

        let hits = if emit {
            classify(tx, &mint, has_pool_out, &pool_net)
        } else {
            Vec::new()
        };

        // Update trackers after classify (same order as Indigo).
        self.note_spend_graph_hubs(tx_hash, tx);
        {
            let mut pools = self.pools.lock().unwrap();
            for (index, o) in outputs.iter().enumerate() {
                if value_has_named_asset(o.get("value"), POOL_NFT) {
                    pools.insert(
                        format!("{tx_hash}#{index}"),
                        PoolValue::from_value(o.get("value")),
                    );
                }
            }
        }

        hits.into_iter()
            .map(|hit| (tx_hash.to_string(), hit))
            .collect()
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

fn classify(
    tx: &Value,
    mint: &MintSummary,
    has_pool_out: bool,
    pool_net: &PoolNet,
) -> Vec<DappHit> {
    // Require a pool-identity signal so random VAULT_AT / CIP-68 mints elsewhere
    // don't get attributed to Surf.
    let pool_signal = has_pool_out || mint.pool_nft != 0 || mint.pool_info_nft != 0;
    if !pool_signal {
        return Vec::new();
    }

    // Genuine user actions only — mint/burn of protocol tokens. Pool rewrites
    // with no mint signal (interest / fee touch-ups) are dropped.
    let et = if mint.pool_nft > 0 {
        EventType::CreatePool
    } else if mint.pool_nft < 0 {
        EventType::ClosePool
    } else if mint.vault_at > 0 {
        EventType::Borrow
    } else if mint.vault_at < 0 {
        EventType::Repay
    } else if !mint.other_mints.is_empty() {
        EventType::Supply
    } else if !mint.other_burns.is_empty() {
        EventType::Withdraw
    } else {
        return Vec::new();
    };

    vec![hit_for(et, tx, mint, pool_net)]
}

fn summarize_mint(mint: Option<&Value>) -> MintSummary {
    let mut s = MintSummary::default();
    let Some(obj) = mint.and_then(Value::as_object) else {
        return s;
    };
    for (policy, names) in obj {
        if policy == "ada" {
            continue;
        }
        let Some(names) = names.as_object() else { continue };
        for (name, qty) in names {
            let signed = i128::from(qty.as_i64().unwrap_or(0));
            if signed == 0 {
                continue;
            }
            if name == POOL_NFT {
                s.pool_nft = s.pool_nft.saturating_add(signed as i64);
            } else if name == POOL_INFO_NFT {
                s.pool_info_nft = s.pool_info_nft.saturating_add(signed as i64);
            } else if name == VAULT_AT {
                s.vault_at = s.vault_at.saturating_add(signed as i64);
            } else if signed > 0 {
                s.other_mints
                    .push((policy.clone(), name.clone(), signed));
            } else {
                s.other_burns
                    .push((policy.clone(), name.clone(), signed));
            }
        }
    }
    s
}

fn value_has_named_asset(value: Option<&Value>, name_hex: &str) -> bool {
    let Some(obj) = value.and_then(Value::as_object) else {
        return false;
    };
    for (policy, names) in obj {
        if policy == "ada" {
            continue;
        }
        let Some(names) = names.as_object() else { continue };
        if names.contains_key(name_hex) {
            return true;
        }
    }
    false
}

fn input_outpoint(input: &Value) -> Option<String> {
    let tx = input
        .get("transaction")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .or_else(|| input.get("txId").and_then(Value::as_str))
        .or_else(|| input.get("transactionId").and_then(Value::as_str))?;
    let index = input
        .get("index")
        .and_then(Value::as_u64)
        .or_else(|| input.get("outputIndex").and_then(Value::as_u64))?;
    Some(format!("{tx}#{index}"))
}

fn hit_for(et: EventType, tx: &Value, mint: &MintSummary, pool_net: &PoolNet) -> DappHit {
    let mut data = json!({
        "dapp": DAPP,
        "eventType": et.as_str(),
    });
    let obj = data.as_object_mut().unwrap();
    let actor = surf_actor(et, tx, mint);

    match et {
        EventType::Supply => {
            // Only net pool inflows — never the full pool balance / cToken alone.
            if pool_net.known {
                let (ada, assets) = pool_net.positive_side();
                attach_amounts(obj, ada, &assets);
            }
        }
        EventType::Withdraw => {
            if pool_net.known {
                let (ada, assets) = pool_net.negative_side();
                attach_amounts(obj, ada, &assets);
            }
        }
        EventType::Borrow => {
            // Borrowed principal = assets leaving the pool (net).
            if pool_net.known {
                let (ada, assets) = pool_net.negative_side();
                attach_amounts(obj, ada, &assets);
            }
            // New vault collateral is itself a net deposit (vault did not exist).
            let collateral = vault_collateral_out(tx);
            if !collateral.is_empty() {
                let ptrs: Vec<&(String, String, i128)> = collateral.iter().collect();
                obj.insert("collateral".into(), crate::parse::asset_list(&ptrs));
            }
        }
        EventType::Repay => {
            // Card shows net repaid into the pool only — not batcher fee scraps
            // (e.g. fixed ~528.94 NIGHT + dust on the batcher address).
            let repaid_keys = if pool_net.known {
                let (ada, assets) = pool_net.positive_side();
                attach_amounts(obj, ada, &assets);
                assets
                    .iter()
                    .map(|(p, n, _)| (p.clone(), n.clone()))
                    .collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
            // Real vault collateral returned to the *user* (e.g. FLOW on an ADA loan).
            let collateral =
                returned_collateral_to_user(tx, actor.as_deref(), &repaid_keys);
            if !collateral.is_empty() {
                let ptrs: Vec<&(String, String, i128)> = collateral.iter().collect();
                obj.insert("collateral".into(), crate::parse::asset_list(&ptrs));
            }
        }
        EventType::CreatePool | EventType::ClosePool => {}
    }

    // User (key payment), never pool/vault scripts — see `surf_actor`.
    crate::parse::attach_actor(obj, actor.as_deref());

    DappHit {
        kind: "dapp_activity",
        title: et.title().to_string(),
        data,
    }
}

/// Resolve the **user** for a Surf action (not the batcher / pool script).
///
/// Plain `actor_from_tx` picks the largest key-ADA output, which on supply is
/// usually the batcher’s change — the supplier only receives the cToken.
fn surf_actor(et: EventType, tx: &Value, mint: &MintSummary) -> Option<String> {
    match et {
        EventType::Supply => {
            for (policy, name, qty) in &mint.other_mints {
                let q = qty.unsigned_abs() as u64;
                if q == 0 {
                    continue;
                }
                if let Some(a) = crate::parse::actor_receiving_asset(tx, policy, name, q) {
                    return Some(a);
                }
            }
            crate::parse::actor_from_tx(tx)
        }
        EventType::Withdraw => crate::parse::actor_receiving_ada(tx, 5_000_000)
            .or_else(|| crate::parse::actor_from_tx(tx)),
        // Repay: prefer largest key-ADA out (user change). Do not prefer the
        // batcher’s fee CNT output (~528 NIGHT) as the actor.
        EventType::Borrow | EventType::Repay => crate::parse::actor_from_tx(tx),
        EventType::CreatePool | EventType::ClosePool => crate::parse::actor_from_tx(tx),
    }
}

fn attach_amounts(
    obj: &mut serde_json::Map<String, Value>,
    ada: u64,
    assets: &[(String, String, i128)],
) {
    if ada > 0 {
        obj.insert("ada".into(), json!(ada));
    }
    if !assets.is_empty() {
        let ptrs: Vec<&(String, String, i128)> = assets.iter().collect();
        obj.insert("assets".into(), crate::parse::asset_list(&ptrs));
    }
}

/// Native assets on newly created vault outputs (excludes auth / pool NFTs).
fn vault_collateral_out(tx: &Value) -> Vec<(String, String, i128)> {
    collect_non_pool_native(tx, |o| value_has_named_asset(o.get("value"), VAULT_AT), false, None, None)
}

/// Collateral returned to the loan user — never batcher fee CNTs.
///
/// Skips assets that were the repaid principal (same unit as pool net inflow).
fn returned_collateral_to_user(
    tx: &Value,
    actor: Option<&str>,
    repaid_keys: &HashSet<(String, String)>,
) -> Vec<(String, String, i128)> {
    collect_non_pool_native(tx, |_| true, true, actor, Some(repaid_keys))
}

fn output_matches_actor(addr: &str, actor: &str) -> bool {
    if addr == actor {
        return true;
    }
    if actor.starts_with("stake") {
        return crate::parse::stake_from_address(addr).as_deref() == Some(actor);
    }
    false
}

fn collect_non_pool_native(
    tx: &Value,
    output_ok: impl Fn(&Value) -> bool,
    skip_unit_qty: bool,
    actor: Option<&str>,
    exclude_keys: Option<&HashSet<(String, String)>>,
) -> Vec<(String, String, i128)> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut out = Vec::new();
    for o in outputs {
        if value_has_named_asset(o.get("value"), POOL_NFT) {
            continue;
        }
        if !output_ok(o) {
            continue;
        }
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if let Some(actor) = actor {
            if !output_matches_actor(addr, actor) {
                continue;
            }
        }
        let Some(obj) = o.get("value").and_then(Value::as_object) else {
            continue;
        };
        for (policy, names) in obj {
            if policy == "ada" {
                continue;
            }
            let Some(names) = names.as_object() else { continue };
            for (name, qty) in names {
                if name == VAULT_AT || name == POOL_INFO_NFT || name == POOL_NFT {
                    continue;
                }
                if exclude_keys.is_some_and(|ex| ex.contains(&(policy.clone(), name.clone()))) {
                    continue;
                }
                let q = i128::from(qty.as_i64().unwrap_or(0).unsigned_abs() as i64);
                if q == 0 || (skip_unit_qty && q == 1) {
                    continue;
                }
                out.push((policy.clone(), name.clone(), q));
            }
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out.truncate(8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const POL_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const POL_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const POL_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const POL_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const POL_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn types(hits: &[(String, DappHit)]) -> Vec<&str> {
        hits.iter()
            .map(|(_, h)| h.data["eventType"].as_str().unwrap())
            .collect()
    }

    fn seed_pool(s: &Scanner, hash: &str, ada: u64, extras: Value) {
        let mut value = json!({
            "ada": { "lovelace": ada },
            POL_B: { POOL_NFT: 1 }
        });
        if let Some(obj) = extras.as_object() {
            for (k, v) in obj {
                value
                    .as_object_mut()
                    .unwrap()
                    .insert(k.clone(), v.clone());
            }
        }
        let tx = json!({
            "outputs": [{ "address": "addr1xpool", "value": value }]
        });
        let _ = s.scan_tx_inner(hash, &tx, false);
    }

    #[test]
    fn detects_borrow_with_pool_net_not_full_balance() {
        let s = Scanner::new();
        seed_pool(&s, "prior", 100_000_000_000, json!({}));
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_A: { VAULT_AT: 1 } },
            "outputs": [
                {
                    "address": "addr1xpool",
                    "value": {
                        "ada": { "lovelace": 90_000_000_000u64 },
                        POL_B: { POOL_NFT: 1 }
                    }
                },
                {
                    "address": "addr1xvault",
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        POL_A: { VAULT_AT: 1 },
                        POL_C: { "4e49474854": 1_000_000u64 }
                    }
                },
                {
                    "address": "addr1quser",
                    "value": { "ada": { "lovelace": 10_000_000_000u64 } }
                }
            ]
        });
        let hits = s.scan_block(&[("b", &tx)]);
        assert_eq!(types(&hits), vec!["borrow"]);
        // Net borrowed = 10k ADA, not the remaining 90k pool balance.
        assert_eq!(hits[0].1.data["ada"], 10_000_000_000u64);
        assert_eq!(hits[0].1.title, "Open Loan - Surf");
        assert!(hits[0].1.data.get("collateral").is_some());
    }

    #[test]
    fn borrow_omits_ada_when_prior_pool_unknown() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { POL_A: { VAULT_AT: 1 } },
            "outputs": [
                {
                    "address": "addr1xpool",
                    "value": {
                        "ada": { "lovelace": 90_000_000_000u64 },
                        POL_B: { POOL_NFT: 1 }
                    }
                },
                {
                    "address": "addr1xvault",
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        POL_A: { VAULT_AT: 1 },
                        POL_C: { "434f434b": 1u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("b", &tx)]);
        assert_eq!(types(&hits), vec!["borrow"]);
        // Must not treat the full pool UTxO as the borrow amount.
        assert!(hits[0].1.data.get("ada").is_none());
    }

    #[test]
    fn supply_uses_pool_net_ada() {
        let s = Scanner::new();
        seed_pool(&s, "prior", 1_000_000_000_000, json!({}));
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_D: { "0014df1066414441": 15_000_000_000i64 } },
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 1_015_000_000_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("s", &tx)]);
        assert_eq!(types(&hits), vec!["supply"]);
        assert_eq!(hits[0].1.data["ada"], 15_000_000_000u64);
        // Do not surface cToken mint as the primary amount.
        assert!(hits[0].1.data.get("assets").is_none());
    }

    #[test]
    fn supply_actor_is_ctoken_recipient_not_batcher_change() {
        // Mirrors 65083c1e…: batcher holds ~₳580 change; supplier receives cToken at ₳2.
        let s = Scanner::new();
        seed_pool(&s, "prior", 1_000_000_000_000, json!({}));
        let user = "addr1q9gqsphqrze8jvgjg84decguy6uwr3se5eqkdqkhp9swz5w482ekxnn442wzke60qe8q242tuyyd4qe40hvyvkkfv0cqeczymg";
        let batcher = "addr1q8tg989f3p9wc4fum2pjhr2rm0vefjy5rqkadrqvq9rz6p8rn3drfgphm5xvuz9u5kssgf2jxvtgyhfxmef49e8xq07qdufat7";
        let pool = "addr1x9dl26l64ggw7n8cv50gmc6ekfauw77pdfag6yvq9vxwxh3zdzu598pz3t7jwydjguxcafv8vfrd4zufq8d2kexx82asav5fly";
        let expected_stake = crate::parse::stake_from_address(user).expect("user stake");
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_D: { "0014df1066414441": 14_210_746_109i64 } },
            "outputs": [
                {
                    "address": pool,
                    "value": {
                        "ada": { "lovelace": 2_504_183_897_937u64 },
                        POL_B: { POOL_NFT: 1 }
                    }
                },
                {
                    "address": batcher,
                    "value": { "ada": { "lovelace": 579_584_951u64 } }
                },
                {
                    "address": user,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        POL_D: { "0014df1066414441": 14_210_746_109u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("s", &tx)]);
        assert_eq!(types(&hits), vec!["supply"]);
        assert_eq!(hits[0].1.data["stake"], expected_stake);
        let batcher_stake = crate::parse::stake_from_address(batcher);
        assert_ne!(hits[0].1.data.get("stake"), batcher_stake.as_ref().map(|s| json!(s)).as_ref());
    }

    #[test]
    fn supply_omits_amount_when_prior_unknown() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { POL_D: { "0014df1066414441": 15_000_000_000i64 } },
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 1_015_000_000_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("s", &tx)]);
        assert_eq!(types(&hits), vec!["supply"]);
        assert!(hits[0].1.data.get("ada").is_none());
        assert!(hits[0].1.data.get("assets").is_none());
    }

    #[test]
    fn withdraw_uses_pool_net() {
        let s = Scanner::new();
        seed_pool(&s, "prior", 100_000_000_000, json!({}));
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_D: { "0014df1066414441": -5_000_000_000i64 } },
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 95_000_000_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("w", &tx)]);
        assert_eq!(types(&hits), vec!["withdraw"]);
        assert_eq!(hits[0].1.data["ada"], 5_000_000_000u64);
    }

    #[test]
    fn repay_uses_pool_net_and_user_collateral_not_batcher_fees() {
        let s = Scanner::new();
        seed_pool(&s, "prior", 80_000_000_000, json!({}));
        // FLOW repay recipient from f55e00ab…
        let user = "addr1q853wsrygpu7gf8g00w69ahw6eawfyl3j5nxr5lr8zc2je94q0a7hc0mg8vha6gu3f7nctkf7470zugh0sd7yvp0n96qnfzw82";
        let batcher = "addr1qypvquktryfew2pjqgqmygwekjek0wx5qcmf84kr39f75twz6pjksw7xjma8my77l9v8ld8cgsz6nxs8canw5avmdqvq54rl6f";
        let pool = "addr1x9wrqt696lm407lc9rpe7gl7tjm06gpeh3km36hs7gdg6kydct6jduhwdtq9jtgqrgsusegk4h4jg350dm5fwdv6j3csfd77c9";
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_A: { VAULT_AT: -1 } },
            "outputs": [
                {
                    "address": pool,
                    "value": {
                        "ada": { "lovelace": 82_000_000_000u64 },
                        POL_B: { POOL_NFT: 1 }
                    }
                },
                {
                    "address": user,
                    "value": {
                        "ada": { "lovelace": 107_624_265u64 },
                        POL_C: { "464c4f57": 6_399_979_844u64 }
                    }
                },
                // Batcher fee scraps must not appear as collateral.
                {
                    "address": batcher,
                    "value": {
                        "ada": { "lovelace": 6_249_763u64 },
                        POL_C: { "4e49474854": 528_940_000u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("r", &tx)]);
        assert_eq!(types(&hits), vec!["repay"]);
        assert_eq!(hits[0].1.data["ada"], 2_000_000_000u64);
        assert_eq!(hits[0].1.title, "Repay Loan - Surf");
        let coll = &hits[0].1.data["collateral"]["items"];
        assert_eq!(coll.as_array().unwrap().len(), 1);
        assert_eq!(coll[0]["name"], "FLOW");
        assert_eq!(coll[0]["qty"], "6399979844");
    }

    #[test]
    fn repay_night_loan_shows_only_pool_net_not_fee_chips() {
        // Mirrors a9a72197…: pool +16.6k NIGHT; batcher takes 528.94 + 1.66 NIGHT.
        let s = Scanner::new();
        seed_pool(
            &s,
            "prior",
            2_000_000,
            json!({ POL_C: { "4e49474854": 1_583_631_887_989u64 } }),
        );
        let user = "addr1qywlxjvvd2953sg3z85za2ug28pumf40w5shf3wjtwtff07s94np5jegg4cxlef0wvk9nmzcjqh970c82w3au5usyyhq7gmay7";
        let batcher = "addr1qypvquktryfew2pjqgqmygwekjek0wx5qcmf84kr39f75twz6pjksw7xjma8my77l9v8ld8cgsz6nxs8canw5avmdqvq54rl6f";
        let pool = "addr1x9wrqt696lm407lc9rpe7gl7tjm06gpeh3km36hs7gdg6kydct6jduhwdtq9jtgqrgsusegk4h4jg350dm5fwdv6j3csfd77c9";
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "mint": { POL_A: { VAULT_AT: -1 } },
            "outputs": [
                {
                    "address": user,
                    "value": { "ada": { "lovelace": 5_229_537_659u64 } }
                },
                {
                    "address": pool,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        POL_B: { POOL_NFT: 1 },
                        POL_C: { "4e49474854": 1_600_236_482_532u64 }
                    }
                },
                {
                    "address": batcher,
                    "value": {
                        "ada": { "lovelace": 6_249_763u64 },
                        POL_C: { "4e49474854": 528_940_000u64 }
                    }
                },
                {
                    "address": batcher,
                    "value": {
                        "ada": { "lovelace": 6_178_200u64 },
                        POL_C: { "4e49474854": 1_660_293u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("rn", &tx)]);
        assert_eq!(types(&hits), vec!["repay"]);
        // Net repaid = 1600236482532 - 1583631887989 = 16604594543
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "16604594543");
        assert_eq!(hits[0].1.data["assets"]["items"][0]["name"], "NIGHT");
        assert!(hits[0].1.data.get("collateral").is_none());
    }

    #[test]
    fn ignores_pool_rewrite_without_mint() {
        let s = Scanner::new();
        seed_pool(&s, "prior", 50_000_000_000, json!({}));
        let tx = json!({
            "inputs": [{ "transaction": { "id": "prior" }, "index": 0 }],
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 50_000_100_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        assert!(s.scan_block(&[("touch", &tx)]).is_empty());
    }

    #[test]
    fn detects_create_pool() {
        let s = Scanner::new();
        let tx = json!({
            "mint": {
                POL_B: { POOL_NFT: 1, POOL_INFO_NFT: 1 }
            },
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 5_000_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("c", &tx)]);
        assert_eq!(types(&hits), vec!["create_pool"]);
    }

    #[test]
    fn ignores_unrelated_mint() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { POL_E: { "4e49474854": 1 } },
            "outputs": [{
                "address": "addr1quser",
                "value": { "ada": { "lovelace": 2_000_000u64 } }
            }]
        });
        assert!(s.scan_block(&[("x", &tx)]).is_empty());
    }

    #[test]
    fn pool_nft_outputs_are_spend_graph_hubs() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": "addr1xpool",
                "value": {
                    "ada": { "lovelace": 10_000_000u64 },
                    POL_B: { POOL_NFT: 1 }
                }
            }]
        });
        s.note_spend_graph_hubs("p", &tx);
        assert!(s.is_spend_graph_hub("p#0"));
    }
}
