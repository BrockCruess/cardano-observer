//! Iagon on-chain activity detection.
//!
//! Source: https://docs.iagon.com/blockchain/on-chain-activity
//!
//! Tracked events:
//! - Stake Delegation — lock IAG + delegation NFT at the delegation script
//! - Node Registration — mint of a node NFT into the node script
//! - Node Pledge — any IAG locked at the node script
//! - Earnings Claim — IAG staking rewards and/or ADA subscription fees paid
//!   out from the rewards/batcher address
//! - Node Retirement — burn of a node NFT
//! - Stake Withdrawal — staked IAG leaving node/delegation scripts to a user
//! - Position Listing — market batcher receives IAG (with batcher token)
//! - Position Sale — market batcher sends IAG to one wallet and ADA to another
//! - Subscription — ADA payment to the subscription address

use super::DappHit;
use crate::dex::payment_credential;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "Iagon";

const IAG_POLICY: &str = "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114";
const IAG_NAME_HEX: &str = "494147";

/// Node position NFT policy (= node script payment credential).
const NODE_POLICY: &str = "ac35ee89c26b1e582771ed05af54b67fd7717bbaebd7f722fbf430d6";
/// Delegation position NFT policy (= delegation script payment credential).
const DELEG_POLICY: &str = "faecb80eee6cadf9dac5184263ed4d164b38fe71d4f6f55e8f6b0da0";
/// Staking-market batcher identification token.
const MARKET_BATCHER_POLICY: &str = "3ec3f628639c989da3302724c61c90ced45571d611563d98cd528616";
const MARKET_BATCHER_NAME_HEX: &str = "4961676f6e62617463686572"; // "Iagonbatcher"

const NODE_ADDR: &str =
    "addr1zxkrtm5fcf43ukp8w8kstt65kelawutmht4a0aezl06rp43y2c4s7gthspjk2c4557c9zltqcssl4qz7x5syzf7yknhqma7zxx";
const DELEG_ADDR: &str =
    "addr1z8awewqwaek2m7w6c5vyycldf5tykw87w820da273a4smgpy2c4s7gthspjk2c4557c9zltqcssl4qz7x5syzf7yknhq6uv6j0";
/// Shared rewards / staking-market address.
const BATCHER_ADDR: &str = "addr1v8ckrqqrj4u34sxt45vdu8s8nqq3lm3lc8s7su5nyzaq9tcqy2n8j";
const SUBSCRIPTION_ADDR: &str =
    "addr1q84jyp334avpyc324jhhhpq7ect58dhs9snqxj52ncfc5cj4kyyq3nn28eplyq5vx78jq6xledxldfjm4ua4z5ggsrpqel0wm4";

const TRACKER_CAP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    StakeDelegation,
    NodeRegistration,
    NodePledge,
    EarningsClaim,
    NodeRetirement,
    StakeWithdrawal,
    PositionListing,
    PositionSale,
    Subscription,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::StakeDelegation => "stake_delegation",
            Self::NodeRegistration => "node_registration",
            Self::NodePledge => "node_pledge",
            Self::EarningsClaim => "earnings_claim",
            Self::NodeRetirement => "node_retirement",
            Self::StakeWithdrawal => "stake_withdrawal",
            Self::PositionListing => "position_listing",
            Self::PositionSale => "position_sale",
            Self::Subscription => "subscription",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::StakeDelegation => "Stake Delegation - Iagon",
            Self::NodeRegistration => "Node Registration - Iagon",
            Self::NodePledge => "Node Pledge - Iagon",
            Self::EarningsClaim => "Earnings Claim - Iagon",
            Self::NodeRetirement => "Node Retirement - Iagon",
            Self::StakeWithdrawal => "Stake Withdrawal - Iagon",
            Self::PositionListing => "Position Listing - Iagon",
            Self::PositionSale => "Position Sale - Iagon",
            Self::Subscription => "Subscription - Iagon",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    Node,
    Delegation,
    Batcher,
    Subscription,
}

#[derive(Default, Clone, Debug)]
struct ValueSummary {
    ada: u64,
    iag: u64,
    has_node_nft: bool,
    has_deleg_nft: bool,
    has_market_batcher: bool,
    node_nft_delta: i64,
    deleg_nft_delta: i64,
    /// Node position NFT asset name (hex) — Iagon's on-chain node id.
    node_id: Option<String>,
}

impl ValueSummary {
    fn merge(&mut self, other: &ValueSummary) {
        self.ada = self.ada.saturating_add(other.ada);
        self.iag = self.iag.saturating_add(other.iag);
        self.has_node_nft |= other.has_node_nft;
        self.has_deleg_nft |= other.has_deleg_nft;
        self.has_market_batcher |= other.has_market_batcher;
        self.node_nft_delta = self.node_nft_delta.saturating_add(other.node_nft_delta);
        self.deleg_nft_delta = self.deleg_nft_delta.saturating_add(other.deleg_nft_delta);
        if self.node_id.is_none() {
            self.node_id.clone_from(&other.node_id);
        }
    }

    fn with_iag(iag: u64) -> Self {
        Self {
            iag,
            ..Self::default()
        }
    }

    fn with_ada(ada: u64) -> Self {
        Self {
            ada,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SpentAmounts {
    iag: u64,
    ada: u64,
}

#[derive(Clone, Copy, Debug)]
struct TrackedUtxo {
    role: Role,
    iag: u64,
    ada: u64,
}

/// Bounded FIFO of script UTxOs so spends can be attributed after the fact.
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
        self.map.remove(outpoint)
    }
}

/// Stateful Iagon scanner (tracks script UTxOs across blocks for spend detection).
pub struct Scanner {
    roles: HashMap<String, Role>,
    tracked: Mutex<TrackedSet>,
}

impl Scanner {
    pub fn new() -> Self {
        let mut roles = HashMap::new();
        for (addr, role) in [
            (NODE_ADDR, Role::Node),
            (DELEG_ADDR, Role::Delegation),
            (BATCHER_ADDR, Role::Batcher),
            (SUBSCRIPTION_ADDR, Role::Subscription),
        ] {
            if let Some(cred) = payment_credential(addr) {
                roles.insert(cred, role);
            }
            roles.insert(addr.to_string(), role);
        }
        // Script payment credentials equal the NFT policies; match either form.
        roles.insert(NODE_POLICY.to_string(), Role::Node);
        roles.insert(DELEG_POLICY.to_string(), Role::Delegation);
        Self {
            roles,
            tracked: Mutex::new(TrackedSet::new()),
        }
    }

    fn role_for_addr(&self, addr: &str) -> Option<Role> {
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

    fn scan_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, DappHit)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
        let mint = summarize_value(tx.get("mint"));

        // One summarize pass — reused for classification and UTxO tracking.
        let output_sums: Vec<(&str, ValueSummary)> = outputs
            .iter()
            .map(|o| {
                let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                (addr, summarize_value(o.get("value")))
            })
            .collect();

        let mut by_role: HashMap<Role, ValueSummary> = HashMap::new();
        let mut external: Vec<ValueSummary> = Vec::new();
        for &(addr, ref sum) in &output_sums {
            if let Some(role) = self.role_for_addr(addr) {
                by_role.entry(role).or_default().merge(sum);
            } else if sum.iag > 0 || sum.ada > 0 {
                external.push(sum.clone());
            }
        }

        let mut spent_roles: HashMap<Role, SpentAmounts> = HashMap::new();
        {
            let mut tracked = self.tracked.lock().unwrap();
            for input in inputs {
                let Some(op) = input_outpoint(input) else { continue };
                if let Some(u) = tracked.take(&op) {
                    let s = spent_roles.entry(u.role).or_default();
                    s.iag = s.iag.saturating_add(u.iag);
                    s.ada = s.ada.saturating_add(u.ada);
                }
            }
        }

        let hits = classify(tx_hash, &mint, &by_role, &spent_roles, &external);

        {
            let mut tracked = self.tracked.lock().unwrap();
            for (index, &(addr, ref sum)) in output_sums.iter().enumerate() {
                let Some(role) = self.role_for_addr(addr) else { continue };
                if !matches!(role, Role::Node | Role::Delegation | Role::Batcher) {
                    continue;
                }
                // Track ADA-only batcher UTxOs too (subscription fee pool).
                let keep = sum.iag > 0
                    || sum.has_node_nft
                    || sum.has_deleg_nft
                    || sum.has_market_batcher
                    || (role == Role::Batcher && sum.ada > 0);
                if !keep {
                    continue;
                }
                tracked.insert(
                    format!("{tx_hash}#{index}"),
                    TrackedUtxo {
                        role,
                        iag: sum.iag,
                        ada: sum.ada,
                    },
                );
            }
        }

        hits
    }
}

fn classify(
    tx_hash: &str,
    mint: &ValueSummary,
    by_role: &HashMap<Role, ValueSummary>,
    spent_roles: &HashMap<Role, SpentAmounts>,
    external: &[ValueSummary],
) -> Vec<(String, DappHit)> {
    let mut hits = Vec::new();
    let mut emitted = HashSet::new();
    let mut push = |et: EventType, sum: &ValueSummary| {
        if emitted.insert(et.as_str()) {
            hits.push((tx_hash.to_string(), hit_for(et, sum)));
        }
    };

    if mint.node_nft_delta < 0 {
        // Burn mint carries the node NFT asset name (= node id).
        push(EventType::NodeRetirement, mint);
    }

    if mint.node_nft_delta > 0 {
        let mut sum = by_role.get(&Role::Node).cloned().unwrap_or_default();
        if sum.node_id.is_none() {
            sum.node_id.clone_from(&mint.node_id);
        }
        sum.has_node_nft = true;
        push(EventType::NodeRegistration, &sum);
    }

    if let Some(sum) = by_role.get(&Role::Node) {
        if sum.iag > 0 {
            push(EventType::NodePledge, sum);
        }
    }

    if let Some(sum) = by_role.get(&Role::Delegation) {
        let spent_deleg = spent_roles.get(&Role::Delegation).map(|s| s.iag).unwrap_or(0);
        let fresh_lock = sum.has_deleg_nft && sum.iag > 0 && spent_deleg == 0;
        let topped_up = sum.has_deleg_nft && sum.iag > spent_deleg && spent_deleg > 0;
        if (mint.deleg_nft_delta > 0 && sum.iag > 0) || fresh_lock || topped_up {
            push(EventType::StakeDelegation, sum);
        }
    }

    if let Some(sum) = by_role.get(&Role::Subscription) {
        if sum.ada > 0 {
            push(EventType::Subscription, sum);
        }
    }

    let has_market = mint.has_market_batcher
        || by_role
            .get(&Role::Batcher)
            .is_some_and(|s| s.has_market_batcher)
        || external.iter().any(|s| s.has_market_batcher);

    if has_market {
        let batcher = by_role.get(&Role::Batcher);
        let batcher_iag = batcher.map(|s| s.iag).unwrap_or(0);
        let iag_out = external.iter().filter(|s| s.iag > 0);
        let ada_out = external.iter().filter(|s| s.ada >= 1_000_000 && s.iag == 0);

        if let (Some(iag), Some(ada)) = (
            iag_out.clone().max_by_key(|s| s.iag),
            ada_out.max_by_key(|s| s.ada),
        ) {
            push(
                EventType::PositionSale,
                &ValueSummary {
                    iag: iag.iag,
                    ada: ada.ada,
                    has_market_batcher: true,
                    ..ValueSummary::default()
                },
            );
        } else if batcher_iag > 0 {
            if let Some(sum) = batcher {
                push(EventType::PositionListing, sum);
            }
        }
    } else {
        let spent = spent_roles.get(&Role::Batcher).copied().unwrap_or_default();
        let batcher_out = by_role.get(&Role::Batcher);
        let paid_iag = spent
            .iag
            .saturating_sub(batcher_out.map(|s| s.iag).unwrap_or(0));
        let paid_ada = spent
            .ada
            .saturating_sub(batcher_out.map(|s| s.ada).unwrap_or(0));
        // IAG staking rewards take precedence; companion minADA is ignored.
        // ADA-only net outflow is a subscription-fee claim.
        if paid_iag > 0 {
            push(EventType::EarningsClaim, &ValueSummary::with_iag(paid_iag));
        } else if paid_ada > 0 {
            push(EventType::EarningsClaim, &ValueSummary::with_ada(paid_ada));
        }
    }

    let spent_stake = spent_roles.get(&Role::Node).map(|s| s.iag).unwrap_or(0)
        + spent_roles.get(&Role::Delegation).map(|s| s.iag).unwrap_or(0);
    let returned = by_role.get(&Role::Node).map(|s| s.iag).unwrap_or(0)
        + by_role.get(&Role::Delegation).map(|s| s.iag).unwrap_or(0);
    let external_iag: u64 = external.iter().map(|s| s.iag).sum();

    let withdrawn = if spent_stake > returned && external_iag > 0 {
        Some(spent_stake.saturating_sub(returned).min(external_iag))
    } else if (mint.deleg_nft_delta < 0 || mint.node_nft_delta < 0) && external_iag > 0 {
        Some(external_iag)
    } else {
        None
    };
    if let Some(iag) = withdrawn {
        push(EventType::StakeWithdrawal, &ValueSummary::with_iag(iag));
    }

    hits
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

fn summarize_value(value: Option<&Value>) -> ValueSummary {
    let mut s = ValueSummary::default();
    let Some(v) = value else { return s };
    if let Some(ada) = v
        .get("ada")
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_u64)
    {
        s.ada = ada;
    }
    let Some(obj) = v.as_object() else { return s };
    for (policy, names) in obj {
        if policy == "ada" {
            continue;
        }
        let Some(names) = names.as_object() else { continue };
        for (name, qty) in names {
            let signed = qty.as_i64().unwrap_or(0);
            let q = signed.unsigned_abs();
            if policy == IAG_POLICY && name == IAG_NAME_HEX {
                s.iag = s.iag.saturating_add(q);
            } else if policy == NODE_POLICY {
                s.has_node_nft = true;
                s.node_nft_delta = s.node_nft_delta.saturating_add(signed);
                if s.node_id.is_none() && !name.is_empty() {
                    s.node_id = Some(name.clone());
                }
            } else if policy == DELEG_POLICY {
                s.has_deleg_nft = true;
                s.deleg_nft_delta = s.deleg_nft_delta.saturating_add(signed);
            } else if policy == MARKET_BATCHER_POLICY && name == MARKET_BATCHER_NAME_HEX {
                s.has_market_batcher = true;
            }
        }
    }
    s
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

fn hit_for(et: EventType, sum: &ValueSummary) -> DappHit {
    let mut data = json!({
        "dapp": DAPP,
        "eventType": et.as_str(),
    });
    let obj = data.as_object_mut().unwrap();

    // ADA is the signal for fee claims, subscriptions, and position sales.
    // IAG reward claims omit companion minADA.
    let show_ada = match et {
        EventType::Subscription | EventType::PositionSale => sum.ada > 0,
        EventType::EarningsClaim => sum.ada > 0 && sum.iag == 0,
        _ => false,
    };
    if show_ada {
        obj.insert("ada".into(), json!(sum.ada));
    }

    if sum.iag > 0 {
        obj.insert("iag".into(), json!(sum.iag));
        obj.insert(
            "assets".into(),
            json!({
                "items": [{
                    "unit": format!("{IAG_POLICY}{IAG_NAME_HEX}"),
                    "policy": IAG_POLICY,
                    "nameHex": IAG_NAME_HEX,
                    "name": "IAG",
                    "qty": sum.iag.to_string(),
                    "ticker": "IAG",
                    "decimals": 6,
                }],
                "more": 0
            }),
        );
    }

    if matches!(
        et,
        EventType::NodeRegistration | EventType::NodeRetirement | EventType::NodePledge
    ) {
        if let Some(node_id) = &sum.node_id {
            obj.insert("nodeId".into(), json!(node_id));
        }
    }

    DappHit {
        kind: "dapp_activity",
        title: et.title().to_string(),
        data,
    }
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

    #[test]
    fn detects_node_registration_and_pledge() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { "ac35ee89c26b1e582771ed05af54b67fd7717bbaebd7f722fbf430d6": { "681d1b586436331781d7229f": 1 } },
            "outputs": [{
                "address": NODE_ADDR,
                "value": {
                    "ada": { "lovelace": 1_805_890 },
                    "ac35ee89c26b1e582771ed05af54b67fd7717bbaebd7f722fbf430d6": { "681d1b586436331781d7229f": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 214_885_735_768u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("abcd", &tx)]);
        let t = types(&hits);
        assert!(t.contains(&"node_registration"));
        assert!(t.contains(&"node_pledge"));
        let reg = hits
            .iter()
            .find(|(_, h)| h.data["eventType"] == "node_registration")
            .unwrap();
        assert_eq!(reg.1.data["nodeId"], "681d1b586436331781d7229f");
        assert!(reg.1.data.get("ada").is_none());
    }

    #[test]
    fn detects_node_pledge_without_mint() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": NODE_ADDR,
                "value": {
                    "ada": { "lovelace": 1_805_890 },
                    "ac35ee89c26b1e582771ed05af54b67fd7717bbaebd7f722fbf430d6": { "681d1b586436331781d7229f": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 50_000_000_000u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("pledge", &tx)]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.data["eventType"], "node_pledge");
        assert_eq!(hits[0].1.data["nodeId"], "681d1b586436331781d7229f");
    }

    #[test]
    fn detects_stake_delegation() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": DELEG_ADDR,
                "value": {
                    "ada": { "lovelace": 1_693_830 },
                    "faecb80eee6cadf9dac5184263ed4d164b38fe71d4f6f55e8f6b0da0": { "69bf9cfbfcd2bb5a35130d22": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 1_508_448_557u64 }
                }
            }]
        });
        assert_eq!(
            s.scan_block(&[("del", &tx)])[0].1.data["eventType"],
            "stake_delegation"
        );
    }

    #[test]
    fn ignores_delegation_rewrite_same_iag() {
        let s = Scanner::new();
        let create = json!({
            "outputs": [{
                "address": DELEG_ADDR,
                "value": {
                    "ada": { "lovelace": 1_719_690 },
                    "faecb80eee6cadf9dac5184263ed4d164b38fe71d4f6f55e8f6b0da0": { "aabb": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 1_508_448_557u64 }
                }
            }]
        });
        assert_eq!(s.scan_block(&[("c", &create)]).len(), 1);
        let rewrite = json!({
            "inputs": [{ "transaction": { "id": "c" }, "index": 0 }],
            "outputs": [{
                "address": DELEG_ADDR,
                "value": {
                    "ada": { "lovelace": 1_693_830 },
                    "faecb80eee6cadf9dac5184263ed4d164b38fe71d4f6f55e8f6b0da0": { "aabb": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 1_508_448_557u64 }
                }
            }]
        });
        assert!(s.scan_block(&[("r", &rewrite)]).is_empty());
    }

    #[test]
    fn earnings_claim_uses_net_iag_from_batcher() {
        let s = Scanner::new();
        let fund = json!({
            "outputs": [{
                "address": BATCHER_ADDR,
                "value": {
                    "ada": { "lovelace": 3_963_225_276u64 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 452_012_625_756u64 }
                }
            }]
        });
        assert!(s.scan_block(&[("fund", &fund)]).is_empty());
        let claim = json!({
            "inputs": [{ "transaction": { "id": "fund" }, "index": 0 }],
            "outputs": [
                {
                    "address": BATCHER_ADDR,
                    "value": {
                        "ada": { "lovelace": 3_963_225_276u64 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 452_011_082_975u64 }
                    }
                },
                {
                    "address": "addr1qusereraaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaauser",
                    "value": {
                        "ada": { "lovelace": 2_000_000 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 1_542_781u64 }
                    }
                },
                {
                    "address": "addr1qchangeaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaachg",
                    "value": {
                        "ada": { "lovelace": 44_397_681 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 171_144_875u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("claim", &claim)]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.data["eventType"], "earnings_claim");
        assert_eq!(hits[0].1.data["iag"], 1_542_781u64);
        assert!(hits[0].1.data.get("ada").is_none());
    }

    #[test]
    fn earnings_claim_ada_fee_only() {
        let s = Scanner::new();
        let fund = json!({
            "outputs": [{
                "address": BATCHER_ADDR,
                "value": {
                    "ada": { "lovelace": 50_000_000u64 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 100_000_000u64 }
                }
            }]
        });
        assert!(s.scan_block(&[("fund", &fund)]).is_empty());
        // IAG stays on the batcher; only ADA is paid out (subscription fee claim).
        let claim = json!({
            "inputs": [{ "transaction": { "id": "fund" }, "index": 0 }],
            "outputs": [
                {
                    "address": BATCHER_ADDR,
                    "value": {
                        "ada": { "lovelace": 35_000_000u64 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 100_000_000u64 }
                    }
                },
                {
                    "address": "addr1qfeeraaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaafeee",
                    "value": { "ada": { "lovelace": 15_000_000u64 } }
                }
            ]
        });
        let hits = s.scan_block(&[("fee", &claim)]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1.data["eventType"], "earnings_claim");
        assert_eq!(hits[0].1.data["ada"], 15_000_000u64);
        assert!(hits[0].1.data.get("iag").is_none());
    }

    #[test]
    fn detects_node_retirement_burn() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { "ac35ee89c26b1e582771ed05af54b67fd7717bbaebd7f722fbf430d6": { "681d1b586436331781d7229f": -1 } },
            "outputs": [{
                "address": "addr1qyhc8mlr47tn3dnj0y7wad2rt5g6ctuhty46fw2sqtr20hw8xrzzzz",
                "value": {
                    "ada": { "lovelace": 2_000_000 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 100_000_000u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("ret", &tx)]);
        let t = types(&hits);
        assert!(t.contains(&"node_retirement"));
        assert!(t.contains(&"stake_withdrawal"));
        let ret = hits
            .iter()
            .find(|(_, h)| h.data["eventType"] == "node_retirement")
            .unwrap();
        assert_eq!(ret.1.data["nodeId"], "681d1b586436331781d7229f");
    }

    #[test]
    fn detects_position_listing() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": BATCHER_ADDR,
                "value": {
                    "ada": { "lovelace": 2_000_000 },
                    "3ec3f628639c989da3302724c61c90ced45571d611563d98cd528616": { "4961676f6e62617463686572": 1 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 50_000_000_000u64 }
                }
            }]
        });
        assert_eq!(
            s.scan_block(&[("list", &tx)])[0].1.data["eventType"],
            "position_listing"
        );
    }

    #[test]
    fn detects_position_sale() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [
                {
                    "address": BATCHER_ADDR,
                    "value": {
                        "ada": { "lovelace": 2_000_000 },
                        "3ec3f628639c989da3302724c61c90ced45571d611563d98cd528616": { "4961676f6e62617463686572": 1 }
                    }
                },
                {
                    "address": "addr1qbuyeraaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaasxyz",
                    "value": {
                        "ada": { "lovelace": 2_000_000 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 50_000_000_000u64 },
                        "faecb80eee6cadf9dac5184263ed4d164b38fe71d4f6f55e8f6b0da0": { "aabbcc": 1 }
                    }
                },
                {
                    "address": "addr1qselleraaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabcd",
                    "value": { "ada": { "lovelace": 75_000_000 } }
                }
            ]
        });
        let hits = s.scan_block(&[("sale", &tx)]);
        assert_eq!(hits[0].1.data["eventType"], "position_sale");
        assert_eq!(hits[0].1.data["iag"], 50_000_000_000u64);
        assert_eq!(hits[0].1.data["ada"], 75_000_000u64);
    }

    #[test]
    fn detects_subscription() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": SUBSCRIPTION_ADDR,
                "value": { "ada": { "lovelace": 12_821_995 } }
            }]
        });
        let hits = s.scan_block(&[("sub", &tx)]);
        assert_eq!(hits[0].1.data["eventType"], "subscription");
        assert_eq!(hits[0].1.data["ada"], 12_821_995u64);
    }

    #[test]
    fn detects_earnings_claim_via_tracker() {
        let s = Scanner::new();
        let fund = json!({
            "outputs": [{
                "address": BATCHER_ADDR,
                "value": {
                    "ada": { "lovelace": 1_193_870 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 168_652_222u64 }
                }
            }]
        });
        assert!(s.scan_block(&[("fund", &fund)]).is_empty());
        let claim = json!({
            "inputs": [{ "transaction": { "id": "fund" }, "index": 0 }],
            "outputs": [{
                "address": "addr1qclaimeraaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaxyz",
                "value": {
                    "ada": { "lovelace": 1_500_000 },
                    "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 168_652_222u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("claim", &claim)]);
        assert_eq!(hits[0].1.data["eventType"], "earnings_claim");
        assert_eq!(hits[0].1.data["iag"], 168_652_222u64);
    }
}
