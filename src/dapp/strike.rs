//! Strike Finance V2 Cardano custody detection (locker deposit / withdraw).
//!
//! Sources:
//! - https://docs.strikefinance.org/perpetuals/deposits-and-withdrawals
//! - https://docs.strikefinance.org/api/builder-codes/deposit
//!
//! V2 trading is off-chain on the Strike Node. Cardano L1 only handles
//! custody: users send assets to a protocol locker (often USDM after the
//! exchanger swaps ADA), and validators later release funds on withdraw.
//!
//! Mainnet locker payment credential resolved from observed deposit UTxOs
//! carrying CIP-30 COSE-signed quote datums (`address` / `hashed` fields)
//! under validator authority `e5efa8e8…` / `98d1fbc3…`.

use super::DappHit;
use crate::parse::payment_credential;
use crate::parse::{address_has_script_payment, asset_list, attach_actor, stake_from_address};
use bech32::{Bech32, Hrp};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "Strike";

/// V2 locker script payment credential (enterprise `addr1wx7j4…`).
const LOCKER_HASH: &str = "bd2adb685621e224aae7571cb6bd8f0beb0fdd31875eb3a27feee6c0";

/// Published bech32 form of [`LOCKER_HASH`] (no stake part).
const LOCKER_ADDR: &str = "addr1wx7j4kmg2cs7yf92uat3ed4a3u97kr7axxr4avaz0lhwdsq87ujx7";

/// Protocol validator / batcher payment credentials — never the user actor.
const PROTOCOL_AUTH_HASHES: &[&str] = &[
    "e5efa8e8ee2c02fdf1fec7df2bd5799919ac41f1986ed963637b3508",
    "98d1fbc32e7063cbc6283d178598818779f36c063b545ce4eab049d2",
];

const TRACKER_CAP: usize = 20_000;
/// Ignore dust ADA noise when classifying net direction (min-ADA reshuffles).
const ADA_DUST: i128 = 2_000_000;

/// ASCII `address` as hex inside COSE map keys in locker datums.
const COSE_ADDRESS_KEY: &str = "6761646472657373";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    Deposit,
    Withdraw,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Withdraw => "withdraw",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Deposit => "Strike Deposit",
            Self::Withdraw => "Strike Withdraw",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LockerValue {
    ada: u64,
    /// policy → name_hex → qty
    assets: HashMap<String, HashMap<String, u64>>,
    /// User payment address from the locker datum (deposit owner / withdrawer).
    owner: Option<String>,
}

impl LockerValue {
    fn from_output(output: &Value) -> Self {
        let mut lv = Self::from_value(output.get("value"));
        lv.owner = output
            .get("datum")
            .and_then(Value::as_str)
            .and_then(actor_from_locker_datum);
        lv
    }

    fn from_value(value: Option<&Value>) -> Self {
        let mut lv = Self::default();
        let Some(obj) = value.and_then(Value::as_object) else {
            return lv;
        };
        if let Some(ada) = obj
            .get("ada")
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
        {
            lv.ada = ada;
        } else if let Some(ada) = obj
            .get("ada")
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_i64)
        {
            lv.ada = ada.unsigned_abs();
        }
        for (policy, names) in obj {
            if policy == "ada" {
                continue;
            }
            let Some(names) = names.as_object() else { continue };
            for (name, qty) in names {
                let q = qty.as_i64().unwrap_or(0).unsigned_abs();
                if q == 0 {
                    continue;
                }
                lv.assets
                    .entry(policy.clone())
                    .or_default()
                    .insert(name.clone(), q);
            }
        }
        lv
    }

    fn merge(&mut self, other: &Self) {
        self.ada = self.ada.saturating_add(other.ada);
        for (policy, names) in &other.assets {
            let entry = self.assets.entry(policy.clone()).or_default();
            for (name, q) in names {
                *entry.entry(name.clone()).or_default() =
                    entry.get(name).copied().unwrap_or(0).saturating_add(*q);
            }
        }
        if self.owner.is_none() {
            self.owner.clone_from(&other.owner);
        }
    }

    fn is_empty(&self) -> bool {
        self.ada == 0 && self.assets.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
struct LockerNet {
    known: bool,
    ada: i128,
    assets: Vec<(String, String, i128)>,
}

impl LockerNet {
    fn from_flows(spent: Option<LockerValue>, out: LockerValue) -> Self {
        let Some(spent) = spent else {
            // No prior spends: treat full locker outputs as a net inflow when present.
            if out.is_empty() {
                return Self::default();
            }
            let mut assets = Vec::new();
            for (p, names) in &out.assets {
                for (n, q) in names {
                    assets.push((p.clone(), n.clone(), *q as i128));
                }
            }
            assets.sort_by(|a, b| b.2.unsigned_abs().cmp(&a.2.unsigned_abs()));
            return Self {
                known: true,
                ada: out.ada as i128,
                assets,
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

    fn has_meaningful_inflow(&self) -> bool {
        self.ada > ADA_DUST || self.assets.iter().any(|(_, _, q)| *q > 0)
    }

    fn has_meaningful_outflow(&self) -> bool {
        self.ada < -ADA_DUST || self.assets.iter().any(|(_, _, q)| *q < 0)
    }

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

struct TrackedSet {
    map: HashMap<String, LockerValue>,
    order: VecDeque<String>,
}

impl TrackedSet {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, outpoint: String, value: LockerValue) {
        if self.map.insert(outpoint.clone(), value).is_none() {
            self.order.push_back(outpoint);
            while self.order.len() > TRACKER_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }

    fn take(&mut self, outpoint: &str) -> Option<LockerValue> {
        self.map.remove(outpoint)
    }
}

/// Stateful Strike V2 locker scanner.
pub struct Scanner {
    roles: HashMap<String, ()>,
    tracked: Mutex<TrackedSet>,
    graph_hubs: Mutex<HashSet<String>>,
}

impl Scanner {
    pub fn new() -> Self {
        let mut roles = HashMap::new();
        roles.insert(LOCKER_HASH.to_string(), ());
        if let Some(cred) = payment_credential(LOCKER_ADDR) {
            roles.insert(cred, ());
        }
        Self {
            roles,
            tracked: Mutex::new(TrackedSet::new()),
            graph_hubs: Mutex::new(HashSet::new()),
        }
    }

    fn is_locker_addr(&self, addr: &str) -> bool {
        if self.roles.contains_key(addr) {
            return true;
        }
        payment_credential(addr).is_some_and(|c| self.roles.contains_key(&c))
    }

    pub fn is_spend_graph_hub(&self, outpoint: &str) -> bool {
        self.graph_hubs
            .lock()
            .map(|h| h.contains(outpoint))
            .unwrap_or(false)
    }

    pub fn is_spend_graph_hub_address(&self, addr: &str) -> bool {
        self.is_locker_addr(addr)
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
            let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
            if self.is_locker_addr(addr) {
                hubs.insert(format!("{tx_hash}#{index}"));
            }
        }
        while hubs.len() > TRACKER_CAP {
            if let Some(old) = hubs.iter().next().cloned() {
                hubs.remove(&old);
            }
        }
    }

    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        let mut hits = Vec::new();
        for &(tx_hash, tx) in txs {
            hits.extend(self.scan_tx(tx_hash, tx));
        }
        hits
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
            self.note_spend_graph_hubs(hash, tx);
        }
        tracing::info!("strike: warmed locker UTxO tracker from {n} cached txs");
    }

    fn scan_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, DappHit)> {
        let hits = self.scan_tx_inner(tx_hash, tx, true);
        self.note_spend_graph_hubs(tx_hash, tx);
        hits
    }

    fn scan_tx_inner(&self, tx_hash: &str, tx: &Value, emit: bool) -> Vec<(String, DappHit)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);

        let mut out = LockerValue::default();
        let mut locker_outputs: Vec<&Value> = Vec::new();
        let mut locker_out_indexes: Vec<usize> = Vec::new();
        for (index, o) in outputs.iter().enumerate() {
            let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
            if !self.is_locker_addr(addr) {
                continue;
            }
            let lv = LockerValue::from_output(o);
            out.merge(&lv);
            locker_outputs.push(o);
            locker_out_indexes.push(index);
        }

        let mut spent = LockerValue::default();
        let mut spent_any = false;
        let locker_script_spent = tx
            .get("scripts")
            .and_then(Value::as_object)
            .is_some_and(|s| s.contains_key(LOCKER_HASH));

        {
            let mut tracked = self.tracked.lock().unwrap();
            for input in inputs {
                let Some(op) = input_outpoint(input) else { continue };
                if let Some(v) = tracked.take(&op) {
                    spent.merge(&v);
                    spent_any = true;
                }
            }
        }

        for input in inputs {
            let addr = input.get("address").and_then(Value::as_str).unwrap_or("");
            if self.is_locker_addr(addr) {
                spent_any = true;
            }
        }

        let touches = !out.is_empty() || spent_any || locker_script_spent;
        let hits = if emit && touches {
            let actor_hint = resolve_actor(tx, &locker_outputs, spent.owner.as_deref());
            let spent_opt = if spent_any || locker_script_spent {
                Some(spent.clone())
            } else {
                None
            };
            // When we spend the locker but have no tracked priors, classify
            // withdraw from non-locker receipts.
            if spent_any && spent.is_empty() && locker_script_spent {
                classify_untracked_withdraw(tx, self, actor_hint.as_deref())
            } else {
                let net = LockerNet::from_flows(spent_opt, out.clone());
                classify_net(tx, &net, actor_hint.as_deref())
            }
        } else {
            Vec::new()
        };

        {
            let mut tracked = self.tracked.lock().unwrap();
            for &index in &locker_out_indexes {
                let o = &outputs[index];
                let value = LockerValue::from_output(o);
                if value.is_empty() {
                    continue;
                }
                tracked.insert(format!("{tx_hash}#{index}"), value);
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

fn classify_net(tx: &Value, net: &LockerNet, actor: Option<&str>) -> Vec<DappHit> {
    if !net.known {
        return Vec::new();
    }
    if net.has_meaningful_inflow() && !net.has_meaningful_outflow() {
        return hit_from_side(EventType::Deposit, tx, net.positive_side(), actor)
            .into_iter()
            .collect();
    }
    if net.has_meaningful_outflow() && !net.has_meaningful_inflow() {
        return hit_from_side(EventType::Withdraw, tx, net.negative_side(), actor)
            .into_iter()
            .collect();
    }
    // Mixed reshuffle with both sides — prefer the dominant native-asset side.
    let in_native: i128 = net
        .assets
        .iter()
        .filter(|(_, _, q)| *q > 0)
        .map(|(_, _, q)| *q)
        .sum();
    let out_native: i128 = net
        .assets
        .iter()
        .filter(|(_, _, q)| *q < 0)
        .map(|(_, _, q)| -*q)
        .sum();
    if in_native > out_native && in_native > 0 {
        return hit_from_side(EventType::Deposit, tx, net.positive_side(), actor)
            .into_iter()
            .collect();
    }
    if out_native > 0 || net.ada < -ADA_DUST {
        return hit_from_side(EventType::Withdraw, tx, net.negative_side(), actor)
            .into_iter()
            .collect();
    }
    Vec::new()
}

/// Prefer native-asset amounts; only surface ADA when it is the primary transfer
/// (volatile ADA deposits / ADA withdrawals), not locker min-ADA scaffolding.
fn hit_from_side(
    et: EventType,
    tx: &Value,
    side: (u64, Vec<(String, String, i128)>),
    actor: Option<&str>,
) -> Option<DappHit> {
    let (ada, assets) = side;
    let show_ada = if assets.is_empty() { ada } else { 0 };
    if show_ada == 0 && assets.is_empty() {
        return None;
    }
    Some(hit_for(et, tx, show_ada, &assets, actor))
}

/// When locker script is spent but priors were not tracked, attribute USDM/ADA
/// paid to non-script addresses as a withdraw.
fn classify_untracked_withdraw(
    tx: &Value,
    scanner: &Scanner,
    actor: Option<&str>,
) -> Vec<DappHit> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut ada = 0u64;
    let mut assets: HashMap<(String, String), i128> = HashMap::new();
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if scanner.is_locker_addr(addr)
            || address_has_script_payment(addr)
            || is_protocol_auth_addr(addr)
        {
            continue;
        }
        let v = LockerValue::from_value(o.get("value"));
        ada = ada.saturating_add(v.ada);
        for (p, names) in v.assets {
            for (n, q) in names {
                *assets.entry((p.clone(), n)).or_default() += q as i128;
            }
        }
    }
    if ada <= ADA_DUST as u64 && assets.is_empty() {
        return Vec::new();
    }
    let mut asset_vec: Vec<(String, String, i128)> = assets
        .into_iter()
        .map(|((p, n), q)| (p, n, q))
        .collect();
    asset_vec.sort_by(|a, b| b.2.unsigned_abs().cmp(&a.2.unsigned_abs()));
    let show_ada = if asset_vec.is_empty() { ada } else { 0 };
    hit_from_side(EventType::Withdraw, tx, (show_ada, asset_vec), actor)
        .into_iter()
        .collect()
}

fn hit_for(
    et: EventType,
    tx: &Value,
    ada: u64,
    assets: &[(String, String, i128)],
    actor: Option<&str>,
) -> DappHit {
    let mut data = json!({
        "dapp": DAPP,
        "eventType": et.as_str(),
    });
    let obj = data.as_object_mut().unwrap();
    if ada > 0 {
        obj.insert("ada".into(), json!(ada));
    }
    if !assets.is_empty() {
        let ptrs: Vec<&(String, String, i128)> = assets.iter().collect();
        obj.insert("assets".into(), asset_list(&ptrs));
    }
    let resolved = actor
        .map(str::to_string)
        .or_else(|| fallback_actor(et, tx));
    attach_actor(obj, resolved.as_deref());
    DappHit {
        kind: "dapp_activity",
        title: et.title().to_string(),
        data,
    }
}

fn resolve_actor(tx: &Value, locker_outputs: &[&Value], spent_owner: Option<&str>) -> Option<String> {
    // 1) User address embedded in locker deposit/withdraw datums (authoritative).
    for o in locker_outputs {
        if let Some(actor) = o
            .get("datum")
            .and_then(Value::as_str)
            .and_then(actor_from_locker_datum)
        {
            return Some(prefer_stake(&actor));
        }
    }
    // 2) Owner remembered from the spent locker UTxO.
    if let Some(owner) = spent_owner {
        return Some(prefer_stake(owner));
    }
    // 3) Withdraw: recipient whose payment cred is in requiredExtraSignatories.
    if let Some(actor) = actor_from_required_extra(tx) {
        return Some(actor);
    }
    // 4) Largest non-protocol, non-locker key recipient / change.
    fallback_actor_from_outputs(tx)
}

fn fallback_actor(et: EventType, tx: &Value) -> Option<String> {
    match et {
        EventType::Deposit => {
            // Datum should already have covered deposits; last resort = non-auth change.
            fallback_actor_from_outputs(tx)
        }
        EventType::Withdraw => actor_from_required_extra(tx).or_else(|| fallback_actor_from_outputs(tx)),
    }
}

fn fallback_actor_from_outputs(tx: &Value) -> Option<String> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut best: Option<(i128, String)> = None;
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if is_locker_addr_str(addr) || address_has_script_payment(addr) || is_protocol_auth_addr(addr)
        {
            continue;
        }
        let v = LockerValue::from_value(o.get("value"));
        let native: i128 = v
            .assets
            .values()
            .flat_map(|m| m.values())
            .map(|q| *q as i128)
            .sum();
        let score = if native > 0 { native } else { v.ada as i128 };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, prefer_stake(addr)));
        }
    }
    best.map(|(_, a)| a)
}

fn actor_from_required_extra(tx: &Value) -> Option<String> {
    let empty = Vec::new();
    let extras: HashSet<&str> = tx
        .get("requiredExtraSignatories")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if extras.is_empty() {
        return None;
    }
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut best: Option<(i128, String)> = None;
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        let Some(cred) = payment_credential(addr) else {
            continue;
        };
        if !extras.contains(cred.as_str()) {
            continue;
        }
        if is_protocol_auth_addr(addr) {
            continue;
        }
        let v = LockerValue::from_value(o.get("value"));
        let native: i128 = v
            .assets
            .values()
            .flat_map(|m| m.values())
            .map(|q| *q as i128)
            .sum();
        let score = if native > 0 { native } else { v.ada as i128 };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, prefer_stake(addr)));
        }
    }
    best.map(|(_, a)| a)
}

fn is_locker_addr_str(addr: &str) -> bool {
    addr == LOCKER_ADDR || payment_credential(addr).as_deref() == Some(LOCKER_HASH)
}

fn is_protocol_auth_addr(addr: &str) -> bool {
    payment_credential(addr).is_some_and(|c| PROTOCOL_AUTH_HASHES.contains(&c.as_str()))
}

fn prefer_stake(addr: &str) -> String {
    stake_from_address(addr).unwrap_or_else(|| addr.to_string())
}

/// Extract the user Shelley address from a Strike locker datum (COSE `address`).
fn actor_from_locker_datum(datum_hex: &str) -> Option<String> {
    let key_idx = datum_hex.find(COSE_ADDRESS_KEY)?;
    let after = &datum_hex[key_idx + COSE_ADDRESS_KEY.len()..];
    // Major type 2 (bstr): 58 xx <len bytes> or 59 xxxx …
    let bytes = if let Some(rest) = after.strip_prefix("58") {
        let len = usize::from_str_radix(rest.get(0..2)?, 16).ok()?;
        let hex = rest.get(2..2 + len * 2)?;
        hex::decode(hex).ok()?
    } else if let Some(rest) = after.strip_prefix("59") {
        let len = usize::from_str_radix(rest.get(0..4)?, 16).ok()?;
        let hex = rest.get(4..4 + len * 2)?;
        hex::decode(hex).ok()?
    } else {
        return None;
    };
    // Shelley payment addresses are 29 (enterprise) or 57 (base) bytes.
    if bytes.len() != 29 && bytes.len() != 57 {
        return None;
    }
    encode_address_bytes(&bytes)
}

fn encode_address_bytes(bytes: &[u8]) -> Option<String> {
    let hrp = Hrp::parse("addr").ok()?;
    bech32::encode::<Bech32>(hrp, bytes).ok()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const USDM_POL: &str = "c48cbb3d5e57ed56e276bc45f99ab39abe94e6cd7ac39fb402da47ad";
    const USDM_NAME: &str = "0014df105553444d";
    /// User payment + stake (key/key base addr) used in COSE `address` field.
    const USER_PAY: &str = "becbe7b33417aed7392398e447b0c25604b84b51043ef5c43260be82";
    const USER_STAKE: &str = "458c36ea901a31748844944bacec7d69b646d38555d56b08520b6546";
    const AUTH_ADDR: &str =
        "addr1q8j7l28gackq9l03lmra72740xv3ntzp7xvxaktrvdan2zrwszqnu4z8s4m53ewq7kp745gx9yyecs80688vrdm9v58qx9nuzn";

    fn user_addr() -> String {
        let mut addr_bytes = vec![0x01u8];
        addr_bytes.extend(hex::decode(USER_PAY).unwrap());
        addr_bytes.extend(hex::decode(USER_STAKE).unwrap());
        encode_address_bytes(&addr_bytes).expect("user addr")
    }

    fn user_cose_datum() -> String {
        // Minimal suffix: … "address" <bstr 57-byte shelley addr>
        let mut addr_bytes = vec![0x01u8];
        addr_bytes.extend(hex::decode(USER_PAY).unwrap());
        addr_bytes.extend(hex::decode(USER_STAKE).unwrap());
        format!(
            "d87980{}58{:02x}{}",
            COSE_ADDRESS_KEY,
            addr_bytes.len(),
            hex::encode(addr_bytes)
        )
    }

    fn deposit_tx() -> Value {
        json!({
            "inputs": [{"transaction": {"id": "aaaa"}, "index": 0}],
            "outputs": [
                {
                    "address": LOCKER_ADDR,
                    "datum": user_cose_datum(),
                    "value": {
                        "ada": {"lovelace": 7301140},
                        USDM_POL: { USDM_NAME: 2_000_000 }
                    }
                },
                {
                    // Protocol auth change — must NOT be chosen as actor.
                    "address": AUTH_ADDR,
                    "value": {"ada": {"lovelace": 5_000_000}}
                }
            ]
        })
    }

    #[test]
    fn simple_usdm_send_is_deposit() {
        let s = Scanner::new();
        let tx = deposit_tx();
        let hits = s.scan_tx("dep1", &tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.data["dapp"], "Strike");
        assert_eq!(hits[0].1.data["eventType"], "deposit");
        assert_eq!(hits[0].1.title, "Strike Deposit");
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "2000000");
        // Stake preferred for handle resolution.
        let stake = stake_from_address(&user_addr()).expect("stake");
        assert_eq!(hits[0].1.data["stake"], stake);
        assert!(hits[0].1.data.get("address").is_none());
        assert!(s.is_spend_graph_hub("dep1#0"));
    }

    #[test]
    fn locker_spend_net_out_is_withdraw() {
        let s = Scanner::new();
        let dep = deposit_tx();
        let _ = s.scan_tx("dep1", &dep);
        let user = user_addr();

        let withdraw = json!({
            "scripts": { LOCKER_HASH: {"language": "plutus:v3"} },
            "requiredExtraSignatories": [USER_PAY],
            "inputs": [{"transaction": {"id": "dep1"}, "index": 0}],
            "outputs": [
                {
                    "address": user,
                    "value": {
                        "ada": {"lovelace": 2_000_000},
                        USDM_POL: { USDM_NAME: 2_000_000 }
                    }
                }
            ],
            "redeemers": [{"validator": {"purpose": "spend", "index": 0}, "redeemer": "d87e80"}]
        });
        let hits = s.scan_tx("wd1", &withdraw);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.data["eventType"], "withdraw");
        assert_eq!(hits[0].1.title, "Strike Withdraw");
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "2000000");
        let stake = stake_from_address(&user_addr()).expect("stake");
        assert_eq!(hits[0].1.data["stake"], stake);
    }

    #[test]
    fn unrelated_tx_ignored() {
        let s = Scanner::new();
        let tx = json!({
            "inputs": [{"transaction": {"id": "x"}, "index": 0}],
            "outputs": [{
                "address": AUTH_ADDR,
                "value": {"ada": {"lovelace": 1_000_000}}
            }]
        });
        assert!(s.scan_tx("noop", &tx).is_empty());
    }

    #[test]
    fn datum_address_roundtrip() {
        let datum = user_cose_datum();
        let got = actor_from_locker_datum(&datum).expect("addr");
        assert_eq!(got, user_addr());
    }
}
