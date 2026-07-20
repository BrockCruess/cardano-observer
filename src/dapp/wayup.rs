//! Wayup NFT marketplace activity detection.
//!
//! Wayup (Anvil / Cardano-Forge) has no public validator docs. Identifiers were
//! mined from Anvil API examples and mainnet txs carrying CIP-20 metadata
//! `msg: "Wayup Transaction"`:
//! - Ask (listings) — NFT locked for ADA
//! - Bid (offers) — ADA locked for an NFT
//!
//! Redeemers match the jpg.store Ask schema (`Buy` / `WithdrawOrUpdate`).
//! Aggregated checkouts of foreign listings still carry Wayup metadata.

use super::DappHit;
use crate::parse::payment_credential;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "Wayup";

/// Ask / listing script payment credential (NFT escrow).
const ASK_HASH: &str = "a76f0fb801a29f591e9871576508d85b0b5f3c38774f65032f58fdad";
/// Bid / offer script payment credential (ADA escrow).
const BID_HASH: &str = "27d46ecbec94b052d8f875cf3beafd0e8ca40e8ad069f677e0a128ea";

const TRACKER_CAP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    Ask,
    Bid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    List,
    Unlist,
    UpdateListing,
    Sale,
    MakeOffer,
    CancelOffer,
    UpdateOffer,
    AcceptOffer,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Unlist => "unlist",
            Self::UpdateListing => "update_listing",
            Self::Sale => "sale",
            Self::MakeOffer => "make_offer",
            Self::CancelOffer => "cancel_offer",
            Self::UpdateOffer => "update_offer",
            Self::AcceptOffer => "accept_offer",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::List => "List NFT - Wayup",
            Self::Unlist => "Unlist NFT - Wayup",
            Self::UpdateListing => "Update Listing - Wayup",
            Self::Sale => "Sale - Wayup",
            Self::MakeOffer => "Make Offer - Wayup",
            Self::CancelOffer => "Cancel Offer - Wayup",
            Self::UpdateOffer => "Update Offer - Wayup",
            Self::AcceptOffer => "Accept Offer - Wayup",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct NativeAssets {
    ada: u64,
    /// Native assets on the UTxO value (Ask listings carry the NFT here).
    assets: Vec<(String, String, i128)>,
    /// Target NFT from Bid/Ask datum when the UTxO itself is ADA-only (offers).
    wanted: Vec<(String, String, i128)>,
}

impl NativeAssets {
    fn from_value(value: Option<&Value>) -> Self {
        let mut out = Self::default();
        let Some(obj) = value.and_then(Value::as_object) else {
            return out;
        };
        if let Some(ada) = obj
            .get("ada")
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
        {
            out.ada = ada;
        }
        for (policy, names) in obj {
            if policy == "ada" {
                continue;
            }
            let Some(names) = names.as_object() else { continue };
            for (name, qty) in names {
                let q = qty.as_i64().map(|v| v as i128).or_else(|| {
                    qty.as_u64().map(|v| v as i128)
                }).unwrap_or(0);
                if q == 0 {
                    continue;
                }
                out.assets.push((policy.clone(), name.clone(), q.abs()));
            }
        }
        out
    }

    fn has_nft(&self) -> bool {
        self.assets.iter().any(|(_, _, q)| *q > 0)
    }

    /// NFT chips: prefer value assets, else datum-wanted targets.
    fn nfts(&self) -> &[(String, String, i128)] {
        if !self.assets.is_empty() {
            &self.assets
        } else {
            &self.wanted
        }
    }

    fn with_wanted(mut self, wanted: Vec<(String, String, i128)>) -> Self {
        self.wanted = wanted;
        self
    }
}

#[derive(Clone, Debug)]
struct TrackedUtxo {
    role: Role,
    assets: NativeAssets,
}

struct TrackedSet {
    map: HashMap<String, TrackedUtxo>,
    order: VecDeque<String>,
}

impl TrackedSet {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, outpoint: String, utxo: TrackedUtxo) {
        if self.map.insert(outpoint.clone(), utxo).is_none() {
            self.order.push_back(outpoint);
            while self.order.len() > TRACKER_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }

    fn take(&mut self, outpoint: &str) -> Option<TrackedUtxo> {
        if let Some(v) = self.map.remove(outpoint) {
            self.order.retain(|x| x != outpoint);
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpendAction {
    Buy,
    WithdrawOrUpdate,
}

/// Stateful Wayup scanner (tracks Ask/Bid UTxOs + spend-graph hubs).
pub struct Scanner {
    roles: HashMap<String, Role>,
    tracked: Mutex<TrackedSet>,
    graph_hubs: Mutex<HashSet<String>>,
}

impl Scanner {
    pub fn new() -> Self {
        let mut roles = HashMap::new();
        roles.insert(ASK_HASH.to_string(), Role::Ask);
        roles.insert(BID_HASH.to_string(), Role::Bid);
        Self {
            roles,
            tracked: Mutex::new(TrackedSet::new()),
            graph_hubs: Mutex::new(HashSet::new()),
        }
    }

    pub fn is_spend_graph_hub(&self, outpoint: &str) -> bool {
        self.graph_hubs
            .lock()
            .map(|h| h.contains(outpoint))
            .unwrap_or(false)
    }

    pub fn is_spend_graph_hub_address(&self, addr: &str) -> bool {
        self.role_for_addr(addr).is_some()
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
            if self.role_for_addr(addr).is_some() {
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

    fn role_for_addr(&self, addr: &str) -> Option<Role> {
        if addr.is_empty() {
            return None;
        }
        self.roles
            .get(addr)
            .copied()
            .or_else(|| payment_credential(addr).and_then(|c| self.roles.get(&c).copied()))
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
            self.note_spend_graph_hubs(hash, tx);
            let _ = self.scan_tx_inner(hash, tx, false);
        }
        tracing::info!("wayup: warmed Ask/Bid UTxO tracker from {n} cached txs");
    }

    fn scan_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, DappHit)> {
        self.note_spend_graph_hubs(tx_hash, tx);
        self.scan_tx_inner(tx_hash, tx, true)
    }

    fn scan_tx_inner(&self, tx_hash: &str, tx: &Value, emit: bool) -> Vec<(String, DappHit)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);

        let mut created_ask: Vec<NativeAssets> = Vec::new();
        let mut created_bid: Vec<NativeAssets> = Vec::new();
        let mut spent_ask: Vec<NativeAssets> = Vec::new();
        let mut spent_bid: Vec<NativeAssets> = Vec::new();
        let mut touched_script = false;

        // Spends first (Ogmios often omits input addresses — use the tracker).
        {
            let mut tracked = self.tracked.lock().unwrap();
            for input in inputs {
                let Some(op) = input_outpoint(input) else {
                    let addr = input.get("address").and_then(Value::as_str).unwrap_or("");
                    if let Some(role) = self.role_for_addr(addr) {
                        touched_script = true;
                        let assets = NativeAssets::from_value(input.get("value"));
                        match role {
                            Role::Ask => spent_ask.push(assets),
                            Role::Bid => spent_bid.push(assets),
                        }
                    }
                    continue;
                };
                if let Some(utxo) = tracked.take(&op) {
                    touched_script = true;
                    match utxo.role {
                        Role::Ask => spent_ask.push(utxo.assets),
                        Role::Bid => spent_bid.push(utxo.assets),
                    }
                    continue;
                }
                let addr = input.get("address").and_then(Value::as_str).unwrap_or("");
                if let Some(role) = self.role_for_addr(addr) {
                    touched_script = true;
                    let assets = NativeAssets::from_value(input.get("value"));
                    match role {
                        Role::Ask => spent_ask.push(assets),
                        Role::Bid => spent_bid.push(assets),
                    }
                }
            }

            for (index, o) in outputs.iter().enumerate() {
                let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                let Some(role) = self.role_for_addr(addr) else {
                    continue;
                };
                touched_script = true;
                let wanted = nfts_from_output_datum(o, tx);
                let assets = NativeAssets::from_value(o.get("value")).with_wanted(wanted);
                match role {
                    Role::Ask if assets.has_nft() => created_ask.push(assets.clone()),
                    Role::Bid if !assets.has_nft() && assets.ada > 0 => {
                        created_bid.push(assets.clone())
                    }
                    Role::Ask | Role::Bid => {}
                }
                tracked.insert(
                    format!("{tx_hash}#{index}"),
                    TrackedUtxo { role, assets },
                );
            }
        }

        let wayup_meta = has_wayup_metadata(tx);
        let actions = spend_actions(tx);
        let has_buy = actions.contains(&SpendAction::Buy);
        let has_wd = actions.contains(&SpendAction::WithdrawOrUpdate);

        if !emit {
            return Vec::new();
        }
        if !touched_script && !wayup_meta {
            return Vec::new();
        }

        let created_ask_nft = !created_ask.is_empty();
        let created_bid_ada = !created_bid.is_empty();
        let spent_ask_any = !spent_ask.is_empty();
        let spent_bid_any = !spent_bid.is_empty();

        let et = if created_ask_nft && !has_buy && !has_wd {
            Some(EventType::List)
        } else if created_bid_ada && !has_buy && !has_wd {
            Some(EventType::MakeOffer)
        } else if created_ask_nft && has_wd {
            Some(EventType::UpdateListing)
        } else if created_bid_ada && has_wd {
            Some(EventType::UpdateOffer)
        } else if has_buy && spent_bid_any && !spent_ask_any {
            Some(EventType::AcceptOffer)
        } else if has_buy && (spent_ask_any || wayup_meta) {
            Some(EventType::Sale)
        } else if has_wd && spent_ask_any && !created_ask_nft {
            Some(EventType::Unlist)
        } else if has_wd && spent_bid_any && !created_bid_ada {
            Some(EventType::CancelOffer)
        } else if wayup_meta && has_buy {
            Some(EventType::Sale)
        } else if wayup_meta && has_wd && !created_ask_nft && !created_bid_ada {
            // Cancel without tracked parent — prefer unlist when NFT returns to a key wallet.
            if external_nft_outs(tx) {
                Some(EventType::Unlist)
            } else {
                Some(EventType::CancelOffer)
            }
        } else if touched_script || wayup_meta {
            // Touched marketplace but no clear cue — skip rather than spam.
            None
        } else {
            None
        };

        let Some(et) = et else {
            return Vec::new();
        };

        vec![(
            tx_hash.to_string(),
            hit_for(et, tx, &created_ask, &created_bid, &spent_ask, &spent_bid),
        )]
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

fn hit_for(
    et: EventType,
    tx: &Value,
    created_ask: &[NativeAssets],
    created_bid: &[NativeAssets],
    spent_ask: &[NativeAssets],
    spent_bid: &[NativeAssets],
) -> DappHit {
    let mut data = json!({
        "dapp": DAPP,
        "eventType": et.as_str(),
    });
    let obj = data.as_object_mut().unwrap();

    let mut assets: Vec<(String, String, i128)> = Vec::new();
    let mut ada: u64 = 0;

    match et {
        EventType::List | EventType::UpdateListing => {
            for a in created_ask {
                assets.extend(a.nfts().iter().cloned());
            }
        }
        EventType::MakeOffer | EventType::UpdateOffer => {
            for a in created_bid {
                ada = ada.saturating_add(a.ada);
                assets.extend(a.nfts().iter().cloned());
            }
        }
        EventType::Sale => {
            for a in spent_ask {
                assets.extend(a.nfts().iter().cloned());
            }
            if assets.is_empty() {
                collect_external_nfts(tx, &mut assets);
            }
            ada = payout_ada_sum(tx);
        }
        EventType::AcceptOffer => {
            for a in spent_bid {
                assets.extend(a.nfts().iter().cloned());
            }
            if assets.is_empty() {
                collect_external_nfts(tx, &mut assets);
            }
            ada = payout_ada_sum(tx);
        }
        EventType::Unlist => {
            for a in spent_ask {
                assets.extend(a.nfts().iter().cloned());
            }
            if assets.is_empty() {
                collect_external_nfts(tx, &mut assets);
            }
        }
        EventType::CancelOffer => {
            for a in spent_bid {
                ada = ada.saturating_add(a.ada);
                assets.extend(a.nfts().iter().cloned());
            }
            for a in created_bid {
                if ada == 0 {
                    ada = ada.saturating_add(a.ada);
                }
                if assets.is_empty() {
                    assets.extend(a.nfts().iter().cloned());
                }
            }
            if ada == 0 {
                ada = payout_ada_sum(tx);
            }
        }
    }

    // Offers / aggregated buys often only carry the NFT in the script datum.
    if assets.is_empty() {
        assets = nfts_from_tx_datums(tx);
    }

    let count = assets.len().max(if matches!(
        et,
        EventType::MakeOffer | EventType::UpdateOffer | EventType::CancelOffer
    ) {
        created_bid.len().max(spent_bid.len())
    } else {
        0
    });

    if ada > 0 {
        obj.insert("ada".into(), json!(ada));
    }
    if !assets.is_empty() {
        let ptrs: Vec<&(String, String, i128)> = assets.iter().collect();
        obj.insert("assets".into(), crate::parse::asset_list(&ptrs));
    }
    if count > 1 {
        obj.insert("count".into(), json!(count));
    }

    let actor = match et {
        EventType::Sale | EventType::AcceptOffer => {
            if let Some((p, n, q)) = assets.first() {
                if !n.is_empty() {
                    crate::parse::actor_receiving_asset(tx, p, n, *q as u64)
                        .or_else(|| crate::parse::actor_from_tx(tx))
                } else {
                    crate::parse::actor_from_tx(tx)
                }
            } else {
                crate::parse::actor_from_tx(tx)
            }
        }
        EventType::MakeOffer | EventType::UpdateOffer | EventType::CancelOffer => {
            crate::parse::actor_from_tx(tx)
        }
        EventType::List | EventType::UpdateListing | EventType::Unlist => {
            crate::parse::actor_from_tx(tx)
        }
    };
    crate::parse::attach_actor(obj, actor.as_deref());

    DappHit {
        kind: "dapp_activity",
        title: et.title().to_string(),
        data,
    }
}

fn has_wayup_metadata(tx: &Value) -> bool {
    let Some(labels) = tx
        .get("metadata")
        .and_then(|m| m.get("labels"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    let Some(msg) = labels.get("674").and_then(|l| l.get("json")).and_then(|j| j.get("msg"))
    else {
        // Fall back to raw CBOR / string search in the label blob.
        return labels
            .get("674")
            .map(|l| l.to_string().contains("Wayup Transaction"))
            .unwrap_or(false);
    };
    match msg {
        Value::String(s) => s.contains("Wayup Transaction"),
        Value::Array(lines) => lines
            .iter()
            .filter_map(Value::as_str)
            .any(|s| s.contains("Wayup Transaction")),
        _ => false,
    }
}

fn spend_actions(tx: &Value) -> Vec<SpendAction> {
    let mut out = Vec::new();
    let Some(redeemers) = tx.get("redeemers") else {
        return out;
    };
    let push_code = |code: &str, out: &mut Vec<SpendAction>| {
        let c = code.trim();
        if c.is_empty() {
            return;
        }
        // Constr 0 … = Buy; Constr 1 [] = WithdrawOrUpdate (`d87a80`).
        if c.starts_with("d879") {
            out.push(SpendAction::Buy);
        } else if c == "d87a80" || c.starts_with("d87a") {
            out.push(SpendAction::WithdrawOrUpdate);
        }
    };
    match redeemers {
        Value::Array(arr) => {
            for r in arr {
                let purpose = r
                    .pointer("/validator/purpose")
                    .and_then(Value::as_str)
                    .unwrap_or("spend");
                if purpose != "spend" {
                    continue;
                }
                if let Some(code) = r.get("redeemer").and_then(Value::as_str) {
                    push_code(code, &mut out);
                }
            }
        }
        Value::Object(map) => {
            for (_k, r) in map {
                if let Some(code) = r.get("redeemer").and_then(Value::as_str) {
                    push_code(code, &mut out);
                } else if let Some(code) = r.as_str() {
                    push_code(code, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

/// ADA paid to key wallets that do not carry native assets (seller + royalties).
fn payout_ada_sum(tx: &Value) -> u64 {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut sum = 0u64;
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if addr.is_empty() || crate::parse::address_has_script_payment(addr) {
            continue;
        }
        let assets = NativeAssets::from_value(o.get("value"));
        if assets.has_nft() {
            continue;
        }
        sum = sum.saturating_add(assets.ada);
    }
    sum
}

fn collect_external_nfts(tx: &Value, assets: &mut Vec<(String, String, i128)>) {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if addr.is_empty() || crate::parse::address_has_script_payment(addr) {
            continue;
        }
        let na = NativeAssets::from_value(o.get("value"));
        assets.extend(na.assets);
    }
}

/// Resolve inline / hashed datum bytes for an output from `tx.datums`.
fn nfts_from_output_datum(output: &Value, tx: &Value) -> Vec<(String, String, i128)> {
    if let Some(inline) = output.get("datum").and_then(Value::as_str) {
        let nfts = nfts_from_datum_hex(inline);
        if !nfts.is_empty() {
            return nfts;
        }
    }
    let Some(hash) = output.get("datumHash").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(hex) = tx
        .get("datums")
        .and_then(Value::as_object)
        .and_then(|m| m.get(hash))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    nfts_from_datum_hex(hex)
}

fn nfts_from_tx_datums(tx: &Value) -> Vec<(String, String, i128)> {
    let mut out = Vec::new();
    let Some(map) = tx.get("datums").and_then(Value::as_object) else {
        return out;
    };
    for hex in map.values().filter_map(Value::as_str) {
        for nft in nfts_from_datum_hex(hex) {
            if !out.iter().any(|(p, n, _)| p == &nft.0 && n == &nft.1) {
                out.push(nft);
            }
        }
    }
    out
}

/// Scan Wayup/jpg Bid-style datum CBOR for `Map1 { policy => Constr [_, assets] }`.
///
/// Specific-asset offers encode `{ nameHex: 1 }`; collection offers use `{}`
/// (we still surface the policy so the card shows what was bid on).
fn nfts_from_datum_hex(hex: &str) -> Vec<(String, String, i128)> {
    let Ok(bytes) = hex::decode(hex.trim()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 31 < bytes.len() {
        // a1 58 1c <28-byte policy>
        if bytes[i] != 0xa1 || bytes[i + 1] != 0x58 || bytes[i + 2] != 0x1c {
            i += 1;
            continue;
        }
        let pol = hex::encode(&bytes[i + 3..i + 31]);
        let mut j = i + 31;
        // d879 9f = tag(121) + indefinite array (Plutus Constr 0)
        if j + 3 > bytes.len()
            || bytes[j] != 0xd8
            || bytes[j + 1] != 0x79
            || bytes[j + 2] != 0x9f
        {
            i += 1;
            continue;
        }
        j += 3;
        // Skip the leading Plutus integer (asset-map index / variant).
        if j >= bytes.len() {
            break;
        }
        if bytes[j] <= 0x17 {
            j += 1;
        } else if bytes[j] == 0x18 && j + 1 < bytes.len() {
            j += 2;
        } else if bytes[j] == 0x19 && j + 2 < bytes.len() {
            j += 3;
        } else {
            i += 1;
            continue;
        }
        if j >= bytes.len() {
            break;
        }
        if bytes[j] == 0xa0 {
            // Empty asset map → collection-level offer.
            out.push((pol, String::new(), 1));
            i = j + 1;
            continue;
        }
        if bytes[j] != 0xa1 {
            i += 1;
            continue;
        }
        j += 1;
        if j >= bytes.len() {
            break;
        }
        let name = if (0x40..=0x57).contains(&bytes[j]) {
            let n = (bytes[j] - 0x40) as usize;
            j += 1;
            if j + n > bytes.len() {
                break;
            }
            let name = hex::encode(&bytes[j..j + n]);
            j += n;
            name
        } else if bytes[j] == 0x58 && j + 1 < bytes.len() {
            let n = bytes[j + 1] as usize;
            j += 2;
            if j + n > bytes.len() {
                break;
            }
            let name = hex::encode(&bytes[j..j + n]);
            j += n;
            name
        } else {
            i += 1;
            continue;
        };
        // Optional qty (usually 1 / 0x01) — ignore exact value, treat as 1 NFT.
        let _ = j;
        out.push((pol, name, 1));
        i = j;
    }
    out
}

fn external_nft_outs(tx: &Value) -> bool {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if addr.is_empty() || crate::parse::address_has_script_payment(addr) {
            continue;
        }
        if NativeAssets::from_value(o.get("value")).has_nft() {
            return true;
        }
    }
    false
}

fn input_outpoint(input: &Value) -> Option<String> {
    let tx_id = input
        .get("transaction")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .or_else(|| input.get("txId").and_then(Value::as_str))?;
    let index = input.get("index").and_then(Value::as_u64)?;
    Some(format!("{tx_id}#{index}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn types(hits: &[(String, DappHit)]) -> Vec<&str> {
        hits.iter()
            .map(|(_, h)| h.data["eventType"].as_str().unwrap())
            .collect()
    }

    fn meta_wayup() -> Value {
        json!({
            "labels": {
                "674": {
                    "json": { "msg": "Wayup Transaction" }
                }
            }
        })
    }

    #[test]
    fn detects_list_nft_to_ask() {
        let s = Scanner::new();
        let pol = "6fb0ce0d80bce539333b0b16f4a29a0d40c786249f86850d3a36fa01";
        let name = "4646506f776572436f72657331363630";
        let tx = json!({
            "metadata": meta_wayup(),
            "inputs": [{ "transaction": { "id": "user" }, "index": 0 }],
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 1_330_000u64 },
                    pol: { name: 1 }
                }
            }, {
                "address": "addr1qylistingchange0000000000000000000000000000000000000000000000000000000000000000000000000000000000qqqqqq",
                "value": { "ada": { "lovelace": 5_000_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("list1", &tx)]);
        assert_eq!(types(&hits), vec!["list"]);
        assert_eq!(hits[0].1.title, "List NFT - Wayup");
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "1");
    }

    #[test]
    fn detects_make_offer_ada_to_bid() {
        let s = Scanner::new();
        let pol = "6fb0ce0d80bce539333b0b16f4a29a0d40c786249f86850d3a36fa01";
        // "FFPowerCores738" = 15 bytes → CBOR head 0x4f
        let name = "4646506f776572436f726573373338";
        let datum = format!("d87980a1581c{pol}d8799f00a14f{name}01ffff");
        let dh = "11".repeat(32);
        let tx = json!({
            "metadata": meta_wayup(),
            "datums": { dh.clone(): datum },
            "inputs": [{ "transaction": { "id": "user" }, "index": 0 }],
            "outputs": [{
                "address": BID_HASH,
                "datumHash": dh,
                "value": { "ada": { "lovelace": 6_000_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("offer1", &tx)]);
        assert_eq!(types(&hits), vec!["make_offer"]);
        assert_eq!(hits[0].1.data["ada"], 6_000_000u64);
        assert_eq!(hits[0].1.data["assets"]["items"][0]["nameHex"], name);
        assert_eq!(hits[0].1.data["assets"]["items"][0]["policy"], pol);
    }

    #[test]
    fn sale_shows_nft_from_datum_when_not_in_outputs() {
        let s = Scanner::new();
        let pol = "f7f5a12b681be1a2c02054414a726fefadd47e24b0343cd287c0283d";
        let datum = format!("d87980a1581c{pol}d8799f01a0ffff");
        let dh = "22".repeat(32);
        let tx = json!({
            "metadata": meta_wayup(),
            "datums": { dh: datum },
            "inputs": [{ "transaction": { "id": "bid" }, "index": 0 }],
            "redeemers": [{
                "redeemer": "d87980",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [{
                "address": "addr1qxseller000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                "value": { "ada": { "lovelace": 45_000_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("sale_datum", &tx)]);
        assert_eq!(types(&hits), vec!["sale"]);
        assert_eq!(hits[0].1.data["assets"]["items"][0]["policy"], pol);
    }

    #[test]
    fn detects_sale_via_buy_redeemer() {
        let s = Scanner::new();
        let pol = "81791e9e2b5929574039c38020374c753a548ef84bd7eaef8c908bdf";
        let name = "43617264616e6f4b6f6d626174533154303442303937";
        let buyer = "addr1qycx8lp2v7k9xzn8q068hqlm6r6a23hm7ulxpw8ylulqr0000000000000000000000000000000000000000000000000000";
        // Seed Ask UTxO.
        let list = json!({
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    pol: { name: 1 }
                }
            }]
        });
        let _ = s.scan_block(&[("ask1", &list)]);

        let buy = json!({
            "metadata": meta_wayup(),
            "inputs": [{
                "transaction": { "id": "ask1" },
                "index": 0
            }],
            "redeemers": [{
                "redeemer": "d8799f00ff",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [
                {
                    "address": "addr1qxseller000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "value": { "ada": { "lovelace": 20_000_000u64 } }
                },
                {
                    "address": buyer,
                    "value": {
                        "ada": { "lovelace": 3_000_000u64 },
                        pol: { name: 1 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("buy1", &buy)]);
        assert_eq!(types(&hits), vec!["sale"]);
        assert_eq!(hits[0].1.data["ada"], 20_000_000u64);
        assert_eq!(hits[0].1.data["assets"]["items"][0]["nameHex"], name);
    }

    #[test]
    fn detects_unlist_via_withdraw_redeemer() {
        let s = Scanner::new();
        let pol = "6fb0ce0d80bce539333b0b16f4a29a0d40c786249f86850d3a36fa01";
        let name = "4646506f776572436f7265733437";
        let list = json!({
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    pol: { name: 1 }
                }
            }]
        });
        let _ = s.scan_block(&[("ask2", &list)]);

        let unlist = json!({
            "metadata": meta_wayup(),
            "inputs": [{ "transaction": { "id": "ask2" }, "index": 0 }],
            "redeemers": [{
                "redeemer": "d87a80",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [{
                "address": "addr1qysellerback00000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    pol: { name: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("un1", &unlist)]);
        assert_eq!(types(&hits), vec!["unlist"]);
    }

    #[test]
    fn metadata_only_buy_emits_sale() {
        let s = Scanner::new();
        let pol = "aa".repeat(28);
        let name = "4e4654";
        let tx = json!({
            "metadata": meta_wayup(),
            "inputs": [{ "transaction": { "id": "foreign" }, "index": 0 }],
            "redeemers": [{
                "redeemer": "d87980",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [
                {
                    "address": "addr1qxseller000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "value": { "ada": { "lovelace": 10_000_000u64 } }
                },
                {
                    "address": "addr1qybuyer0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        pol: { name: 1 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("agg1", &tx)]);
        assert_eq!(types(&hits), vec!["sale"]);
    }

    #[test]
    fn detects_update_listing() {
        let s = Scanner::new();
        let pol = "6fb0ce0d80bce539333b0b16f4a29a0d40c786249f86850d3a36fa01";
        let name = "4646506f776572436f72657331363630";
        let list = json!({
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    pol: { name: 1 }
                }
            }]
        });
        let _ = s.scan_block(&[("ask3", &list)]);
        let update = json!({
            "metadata": meta_wayup(),
            "inputs": [{ "transaction": { "id": "ask3" }, "index": 0 }],
            "redeemers": [{
                "redeemer": "d87a80",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 2_500_000u64 },
                    pol: { name: 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("upd1", &update)]);
        assert_eq!(types(&hits), vec!["update_listing"]);
    }

    #[test]
    fn detects_cancel_offer() {
        let s = Scanner::new();
        let offer = json!({
            "outputs": [{
                "address": BID_HASH,
                "value": { "ada": { "lovelace": 6_000_000u64 } }
            }]
        });
        let _ = s.scan_block(&[("bid1", &offer)]);
        let cancel = json!({
            "metadata": meta_wayup(),
            "inputs": [{ "transaction": { "id": "bid1" }, "index": 0 }],
            "redeemers": [{
                "redeemer": "d87a80",
                "validator": { "index": 0, "purpose": "spend" }
            }],
            "outputs": [{
                "address": "addr1qybidders00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                "value": { "ada": { "lovelace": 5_500_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("cxl1", &cancel)]);
        assert_eq!(types(&hits), vec!["cancel_offer"]);
    }

    #[test]
    fn ignores_unrelated_tx() {
        let s = Scanner::new();
        let tx = json!({
            "inputs": [{ "transaction": { "id": "x" }, "index": 0 }],
            "outputs": [{
                "address": "addr1qyother000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                "value": { "ada": { "lovelace": 2_000_000u64 } }
            }]
        });
        assert!(s.scan_block(&[("noop", &tx)]).is_empty());
    }

    #[test]
    fn ask_outputs_are_spend_graph_hubs() {
        let s = Scanner::new();
        let tx = json!({
            "inputs": [],
            "outputs": [{
                "address": ASK_HASH,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    "aa": { "4e4654": 1 }
                }
            }]
        });
        s.note_spend_graph_hubs("h", &tx);
        assert!(s.is_spend_graph_hub("h#0"));
        assert!(s.is_spend_graph_hub_address(ASK_HASH));
    }
}
