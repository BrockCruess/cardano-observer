//! FluidTokens on-chain activity detection (Cardano).
//!
//! Sources:
//! - https://docs.fluidtokens.com/
//! - FluidTokens/ft-cardano-loans-v3 `plutus.json` (V3 lending validators)
//! - FluidTokens/ft-cardano-aquarium-sc `plutus.json` (Aquarium)
//! - Legacy P2P lending SC (DefiLlama): addr1wxzqzlncct5g0686c07lyuq3q3j2a0t8l88uwdznw99k9asz6z0hq
//!
//! V3 uses a general-spend + withdraw-0 pattern: pool / request / loan UTxOs
//! sit at `general_spend`, while action scripts appear as ₳0 withdrawals.
//! Classification leans on mint/burn of the pool, request, loan, and bond
//! policies plus which action withdraw scripts are invoked.

use super::DappHit;
use crate::dex::payment_credential;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "FluidTokens";

/// $FLDT (CIP-68) — policy + asset name hex including label prefix.
const FLDT_POLICY: &str = "577f0b1342f8f8f4aed3388b80a8535812950c7a892495c0ecdf0f1e";
const FLDT_NAME_HEX: &str = "0014df10464c4454";

/// V3 mint policies (= validator script hashes from plutus.json).
const BOND_POLICY: &str = "eeb52c51efd7ae6ca9a3c5386660d9b5238bd99236e6561740e2c7ec";
const POOL_POLICY: &str = "2476173b9e9b22d333dc70ed939d6e146ca108523d6619638a47eee5";
const REQUEST_POLICY: &str = "bec931ee5e2d643a580e6c8f602d28a2db73ca2660f075c3f301a118";
const LOAN_POLICY: &str = "15bb8e810a3f1ed05ec92e03fa9ba3d5920329c187c725b27d511cb4";

/// V3 script payment / withdraw credentials.
const GENERAL_SPEND: &str = "27c3a2fc50bae2b32b5a73a4f6a12e3c8df30164d5f9154b432c709d";
const DUTCH_AUCTION: &str = "dae8ad83e362ed8695d251dfadc999cf4da31e5ce8f439901b7e8a97";
const LOCKED_BORROWER_MGR: &str = "25d2185345adffaaa738e957fdf8e721e6d1f8519a0dd27fdd7b8f0d";
const POOL_MANAGER: &str = "ef5396bce6dfbd77ce98fbac56cb3b48bd1284fa17bce2e44aa4b868";
const LENDER_MANAGER: &str = "f8c92c6f66d4f72f7f01af369319672e51c365c02ec7973488b5f21e";

/// Action withdraw scripts (₳0 withdrawal cue).
const ACTION_REPAY: &str = "6b28dda7ea05e494e8535faa796aa7b1c4d972c055f747972cf3ad0e";
const ACTION_RECAST: &str = "faf398af4263ad2e152b0699468027283c78f43b1e9c3abb237aadfe";
const ACTION_CHANGE_COLLATERAL: &str = "f710a2c395d21e7d3738f211705518f4fb34c48681d7edb37ba9823d";
const ACTION_CLAIM: &str = "ef832b355813a97fe8dbcec7cde12dbe1b41b096e1e877bae4277989";
const ACTION_POOL_BORROW: &str = "e1fed4e077ecf6c4b0b25ec1812f1e01d8c84bbfcc2238a3e537ccd8";
const ACTION_POOL_CANCEL: &str = "613f60f9f56943c695994c2dcda2720de910d346a455d1109e8ae28b";
const ACTION_POOL_COMPOUND: &str = "253001f685459e7f15b8b373aefbb6fc576ea611e81450511408ab14";
const ACTION_LIQUIDATE: &str = "285ee2a5a2c43ac59c128f18030edbd9d452257a064c2f2e6ba3a849";
const ACTION_LIQUIDATE_CONVERT: &str = "8c96b1ffda1500d821a40563473a229e1cbed0a10fe3c6eaf8430b19";
const ACTION_LIQUIDATE_PAY: &str = "93cb595fdec8351ab1d4635cbed8dab141eadbbaaec58b19433db9c7";
const ACTION_COMPOUND: &str = "a88afa270135c92161321123855f2b57ffe154879e73e67d70535dfe";

/// Aquarium (babel fees / sponsored txs).
const AQUARIUM_TANK: &str = "9caaf87df65323a18ccffac95178ff59a167b1154d79fe9742ebd090";
const AQUARIUM_STAKER: &str = "738b6bb8efac22a0b9cc78ab376cf4b9264b0018993eea54b03a663d";

/// Legacy P2P lending smart-contract address (pre-V3).
const LEGACY_LEND_ADDR: &str =
    "addr1wxzqzlncct5g0686c07lyuq3q3j2a0t8l88uwdznw99k9asz6z0hq";

const TRACKER_CAP: usize = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    CreatePool,
    CancelPool,
    AdjustPool,
    CompoundPool,
    CreateRequest,
    CancelRequest,
    OpenLoan,
    RepayLoan,
    LiquidateLoan,
    ChangeCollateral,
    RecastLoan,
    ClaimLoan,
    DutchAuction,
    AquariumTank,
    AquariumStake,
    FldtStake,
    LegacyLend,
    LendingActivity,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::CreatePool => "create_pool",
            Self::CancelPool => "cancel_pool",
            Self::AdjustPool => "adjust_pool",
            Self::CompoundPool => "compound_pool",
            Self::CreateRequest => "create_request",
            Self::CancelRequest => "cancel_request",
            Self::OpenLoan => "open_loan",
            Self::RepayLoan => "repay_loan",
            Self::LiquidateLoan => "liquidate_loan",
            Self::ChangeCollateral => "change_collateral",
            Self::RecastLoan => "recast_loan",
            Self::ClaimLoan => "claim_loan",
            Self::DutchAuction => "dutch_auction",
            Self::AquariumTank => "aquarium_tank",
            Self::AquariumStake => "aquarium_stake",
            Self::FldtStake => "fldt_stake",
            Self::LegacyLend => "legacy_lend",
            Self::LendingActivity => "lending_activity",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::CreatePool => "Create Pool - FluidTokens",
            Self::CancelPool => "Cancel Pool - FluidTokens",
            Self::AdjustPool => "Adjust Pool - FluidTokens",
            Self::CompoundPool => "Compound Pool - FluidTokens",
            Self::CreateRequest => "Loan Request - FluidTokens",
            Self::CancelRequest => "Cancel Request - FluidTokens",
            Self::OpenLoan => "Open Loan - FluidTokens",
            Self::RepayLoan => "Repay Loan - FluidTokens",
            Self::LiquidateLoan => "Liquidation - FluidTokens",
            Self::ChangeCollateral => "Change Collateral - FluidTokens",
            Self::RecastLoan => "Recast Loan - FluidTokens",
            Self::ClaimLoan => "Claim Loan - FluidTokens",
            Self::DutchAuction => "Dutch Auction - FluidTokens",
            Self::AquariumTank => "Aquarium - FluidTokens",
            Self::AquariumStake => "Aquarium Stake - FluidTokens",
            Self::FldtStake => "Stake FLDT - FluidTokens",
            Self::LegacyLend => "P2P Lending - FluidTokens",
            Self::LendingActivity => "Lending - FluidTokens",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    GeneralSpend,
    Pool,
    Request,
    Loan,
    DutchAuction,
    LockedBorrower,
    PoolManager,
    LenderManager,
    AquariumTank,
    AquariumStaker,
    LegacyLend,
}

#[derive(Default, Clone, Debug)]
struct ValueSummary {
    ada: u64,
    fldt: u64,
    pool_delta: i64,
    request_delta: i64,
    loan_delta: i64,
    bond_delta: i64,
    has_pool: bool,
    has_request: bool,
    has_loan: bool,
    has_bond: bool,
}

impl ValueSummary {
    fn merge(&mut self, other: &ValueSummary) {
        self.ada = self.ada.saturating_add(other.ada);
        self.fldt = self.fldt.saturating_add(other.fldt);
        self.pool_delta = self.pool_delta.saturating_add(other.pool_delta);
        self.request_delta = self.request_delta.saturating_add(other.request_delta);
        self.loan_delta = self.loan_delta.saturating_add(other.loan_delta);
        self.bond_delta = self.bond_delta.saturating_add(other.bond_delta);
        self.has_pool |= other.has_pool;
        self.has_request |= other.has_request;
        self.has_loan |= other.has_loan;
        self.has_bond |= other.has_bond;
    }
}

#[derive(Clone, Copy, Debug)]
struct TrackedUtxo {
    role: Role,
    /// Kept for future net-flow classification (ADA / FLDT adjusts).
    #[allow(dead_code)]
    ada: u64,
    #[allow(dead_code)]
    fldt: u64,
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
        self.map.remove(outpoint)
    }
}

/// Stateful FluidTokens scanner.
pub struct Scanner {
    roles: HashMap<String, Role>,
    /// Withdraw-script hash → event type (₳0 withdrawal cue).
    actions: HashMap<&'static str, EventType>,
    tracked: Mutex<TrackedSet>,
}

impl Scanner {
    pub fn new() -> Self {
        let mut roles = HashMap::new();
        for (hash, role) in [
            (GENERAL_SPEND, Role::GeneralSpend),
            (POOL_POLICY, Role::Pool),
            (REQUEST_POLICY, Role::Request),
            (LOAN_POLICY, Role::Loan),
            (DUTCH_AUCTION, Role::DutchAuction),
            (LOCKED_BORROWER_MGR, Role::LockedBorrower),
            (POOL_MANAGER, Role::PoolManager),
            (LENDER_MANAGER, Role::LenderManager),
            (AQUARIUM_TANK, Role::AquariumTank),
            (AQUARIUM_STAKER, Role::AquariumStaker),
        ] {
            roles.insert(hash.to_string(), role);
        }
        if let Some(cred) = payment_credential(LEGACY_LEND_ADDR) {
            roles.insert(cred, Role::LegacyLend);
        }
        roles.insert(LEGACY_LEND_ADDR.to_string(), Role::LegacyLend);

        let mut actions = HashMap::new();
        for (hash, et) in [
            (ACTION_REPAY, EventType::RepayLoan),
            (ACTION_RECAST, EventType::RecastLoan),
            (ACTION_CHANGE_COLLATERAL, EventType::ChangeCollateral),
            (ACTION_CLAIM, EventType::ClaimLoan),
            (ACTION_POOL_BORROW, EventType::OpenLoan),
            (ACTION_POOL_CANCEL, EventType::CancelPool),
            (ACTION_POOL_COMPOUND, EventType::CompoundPool),
            (ACTION_LIQUIDATE, EventType::LiquidateLoan),
            (ACTION_LIQUIDATE_CONVERT, EventType::LiquidateLoan),
            (ACTION_LIQUIDATE_PAY, EventType::LiquidateLoan),
            (ACTION_COMPOUND, EventType::CompoundPool),
        ] {
            actions.insert(hash, et);
        }

        Self {
            roles,
            actions,
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
        tracing::info!("fluidtokens: warmed script UTxO tracker from {n} cached txs");
    }

    fn scan_tx(&self, tx_hash: &str, tx: &Value) -> Vec<(String, DappHit)> {
        self.scan_tx_inner(tx_hash, tx, true)
    }

    fn scan_tx_inner(&self, tx_hash: &str, tx: &Value, emit: bool) -> Vec<(String, DappHit)> {
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
        let mint = summarize_value(tx.get("mint"));

        let output_sums: Vec<(&str, ValueSummary)> = outputs
            .iter()
            .map(|o| {
                let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                (addr, summarize_value(o.get("value")))
            })
            .collect();

        let mut by_role: HashMap<Role, ValueSummary> = HashMap::new();
        for &(addr, ref sum) in &output_sums {
            if let Some(role) = self.role_for_addr(addr) {
                by_role.entry(role).or_default().merge(sum);
            }
        }

        let mut spent_roles: HashSet<Role> = HashSet::new();
        {
            let mut tracked = self.tracked.lock().unwrap();
            for input in inputs {
                let Some(op) = input_outpoint(input) else { continue };
                if let Some(u) = tracked.take(&op) {
                    spent_roles.insert(u.role);
                }
            }
        }
        for input in inputs {
            let addr = input.get("address").and_then(Value::as_str).unwrap_or("");
            if let Some(role) = self.role_for_addr(addr) {
                spent_roles.insert(role);
            }
        }

        let action_hits = action_events_from_withdrawals(tx, &self.actions);

        let hits = if emit {
            classify(tx_hash, tx, &mint, &by_role, &spent_roles, &action_hits)
        } else {
            Vec::new()
        };

        {
            let mut tracked = self.tracked.lock().unwrap();
            for (index, &(addr, ref sum)) in output_sums.iter().enumerate() {
                let Some(role) = self.role_for_addr(addr) else { continue };
                if !matches!(
                    role,
                    Role::GeneralSpend
                        | Role::LegacyLend
                        | Role::AquariumTank
                        | Role::AquariumStaker
                        | Role::DutchAuction
                ) {
                    continue;
                }
                if sum.ada == 0 && sum.fldt == 0 && !sum.has_pool && !sum.has_request && !sum.has_loan
                {
                    continue;
                }
                tracked.insert(
                    format!("{tx_hash}#{index}"),
                    TrackedUtxo {
                        role,
                        ada: sum.ada,
                        fldt: sum.fldt,
                    },
                );
            }
        }

        hits
    }
}

fn action_events_from_withdrawals(
    tx: &Value,
    actions: &HashMap<&'static str, EventType>,
) -> Vec<EventType> {
    let Some(w) = tx.get("withdrawals") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    match w {
        Value::Object(map) => {
            for key in map.keys() {
                for (hash, et) in actions {
                    if key.contains(hash) && seen.insert(*et) {
                        out.push(*et);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                let key = item
                    .get("credential")
                    .and_then(Value::as_str)
                    .or_else(|| item.as_str())
                    .unwrap_or("");
                for (hash, et) in actions {
                    if key.contains(hash) && seen.insert(*et) {
                        out.push(*et);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn classify(
    tx_hash: &str,
    tx: &Value,
    mint: &ValueSummary,
    by_role: &HashMap<Role, ValueSummary>,
    spent_roles: &HashSet<Role>,
    action_hits: &[EventType],
) -> Vec<(String, DappHit)> {
    let script_touch = !by_role.is_empty() || !spent_roles.is_empty() || !action_hits.is_empty();
    let auth_touch = mint.pool_delta != 0
        || mint.request_delta != 0
        || mint.loan_delta != 0
        || mint.bond_delta != 0;
    if !script_touch && !auth_touch {
        return Vec::new();
    }

    let mut pending: Vec<(EventType, ValueSummary)> = Vec::new();
    let mut opened_loan = false;
    let mut repaid = false;
    let mut liquidated = false;
    let mut cancelled_pool = false;
    let mut cancelled_request = false;
    let mut claimed = false;

    for &et in action_hits {
        let sum = ValueSummary {
            ada: by_role
                .get(&Role::GeneralSpend)
                .map(|s| s.ada)
                .unwrap_or(0),
            ..ValueSummary::default()
        };
        match et {
            EventType::OpenLoan => opened_loan = true,
            EventType::RepayLoan => repaid = true,
            EventType::LiquidateLoan => liquidated = true,
            EventType::CancelPool => cancelled_pool = true,
            EventType::CancelRequest => cancelled_request = true,
            EventType::ClaimLoan => claimed = true,
            _ => {}
        }
        if !pending.iter().any(|(e, _)| *e == et) {
            pending.push((et, sum));
        }
    }

    if mint.pool_delta > 0 {
        pending.push((
            EventType::CreatePool,
            by_role
                .get(&Role::GeneralSpend)
                .cloned()
                .or_else(|| by_role.get(&Role::Pool).cloned())
                .unwrap_or_default(),
        ));
    } else if mint.pool_delta < 0 && !cancelled_pool && !opened_loan && !liquidated {
        pending.push((EventType::CancelPool, ValueSummary::default()));
        cancelled_pool = true;
    }

    if mint.request_delta > 0 {
        pending.push((
            EventType::CreateRequest,
            by_role
                .get(&Role::GeneralSpend)
                .cloned()
                .unwrap_or_default(),
        ));
    } else if mint.request_delta < 0 && !opened_loan && !cancelled_request {
        pending.push((EventType::CancelRequest, ValueSummary::default()));
        cancelled_request = true;
    }

    if mint.loan_delta > 0 && !opened_loan {
        let mut sum = by_role
            .get(&Role::GeneralSpend)
            .cloned()
            .unwrap_or_default();
        sum.ada = sum
            .ada
            .max(by_role.values().map(|s| s.ada).max().unwrap_or(0));
        pending.push((EventType::OpenLoan, sum));
        opened_loan = true;
    } else if mint.loan_delta < 0 && !repaid && !liquidated && !claimed {
        pending.push((EventType::RepayLoan, ValueSummary::default()));
        repaid = true;
    }

    if by_role.contains_key(&Role::AquariumTank) || spent_roles.contains(&Role::AquariumTank) {
        pending.push((
            EventType::AquariumTank,
            by_role
                .get(&Role::AquariumTank)
                .cloned()
                .unwrap_or_default(),
        ));
    }
    if by_role.contains_key(&Role::AquariumStaker) || spent_roles.contains(&Role::AquariumStaker)
    {
        let sum = by_role
            .get(&Role::AquariumStaker)
            .cloned()
            .unwrap_or_default();
        if sum.fldt > 0 || mint.fldt > 0 {
            let mut s = sum;
            if s.fldt == 0 {
                s.fldt = mint.fldt;
            }
            pending.push((EventType::FldtStake, s));
        } else {
            pending.push((EventType::AquariumStake, sum));
        }
    }

    if by_role.contains_key(&Role::DutchAuction) || spent_roles.contains(&Role::DutchAuction) {
        pending.push((EventType::DutchAuction, ValueSummary::default()));
    }

    if by_role.contains_key(&Role::LegacyLend) || spent_roles.contains(&Role::LegacyLend) {
        pending.push((
            EventType::LegacyLend,
            by_role
                .get(&Role::LegacyLend)
                .cloned()
                .unwrap_or_default(),
        ));
    }

    if pending.is_empty()
        && (by_role.contains_key(&Role::GeneralSpend)
            || spent_roles.contains(&Role::GeneralSpend)
            || by_role.contains_key(&Role::PoolManager)
            || spent_roles.contains(&Role::PoolManager)
            || by_role.contains_key(&Role::LenderManager)
            || spent_roles.contains(&Role::LenderManager))
    {
        if spent_roles.contains(&Role::GeneralSpend)
            && by_role.contains_key(&Role::GeneralSpend)
            && mint.pool_delta == 0
            && mint.loan_delta == 0
            && mint.request_delta == 0
        {
            pending.push((
                EventType::AdjustPool,
                by_role
                    .get(&Role::GeneralSpend)
                    .cloned()
                    .unwrap_or_default(),
            ));
        } else {
            pending.push((
                EventType::LendingActivity,
                by_role
                    .get(&Role::GeneralSpend)
                    .cloned()
                    .unwrap_or_default(),
            ));
        }
    }

    let _ = (opened_loan, repaid, liquidated, cancelled_pool, cancelled_request, claimed);

    let actor = crate::parse::actor_from_tx(tx);
    pending
        .into_iter()
        .map(|(et, sum)| (tx_hash.to_string(), hit_for(et, &sum, actor.clone())))
        .collect()
}

fn hit_for(et: EventType, sum: &ValueSummary, actor: Option<String>) -> DappHit {
    let mut data = json!({
        "dapp": DAPP,
        "eventType": et.as_str(),
    });
    let obj = data.as_object_mut().unwrap();

    if sum.ada > 0
        && matches!(
            et,
            EventType::CreatePool
                | EventType::AdjustPool
                | EventType::OpenLoan
                | EventType::LegacyLend
                | EventType::LendingActivity
                | EventType::AquariumTank
        )
    {
        obj.insert("ada".into(), json!(sum.ada));
    }

    if sum.fldt > 0 {
        obj.insert("fldt".into(), json!(sum.fldt));
        obj.insert(
            "assets".into(),
            json!({
                "items": [{
                    "unit": format!("{FLDT_POLICY}{FLDT_NAME_HEX}"),
                    "policy": FLDT_POLICY,
                    "nameHex": FLDT_NAME_HEX,
                    "name": "FLDT",
                    "qty": sum.fldt.to_string(),
                    "ticker": "FLDT",
                    "decimals": 6,
                }],
                "more": 0
            }),
        );
    }

    crate::parse::attach_actor(obj, actor.as_deref());

    DappHit {
        kind: "dapp_activity",
        title: et.title().to_string(),
        data,
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
    } else if let Some(ada) = v
        .get("ada")
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_i64)
    {
        s.ada = ada.unsigned_abs();
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
            if policy == FLDT_POLICY && (name == FLDT_NAME_HEX || name.ends_with("464c4454")) {
                s.fldt = s.fldt.saturating_add(q);
            } else if policy == POOL_POLICY {
                s.has_pool = true;
                s.pool_delta = s.pool_delta.saturating_add(signed);
            } else if policy == REQUEST_POLICY {
                s.has_request = true;
                s.request_delta = s.request_delta.saturating_add(signed);
            } else if policy == LOAN_POLICY {
                s.has_loan = true;
                s.loan_delta = s.loan_delta.saturating_add(signed);
            } else if policy == BOND_POLICY {
                s.has_bond = true;
                s.bond_delta = s.bond_delta.saturating_add(signed);
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

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
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
    fn detects_create_pool() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { POOL_POLICY: { "abc": 1 }, BOND_POLICY: { "def": 1 } },
            "outputs": [{
                "address": GENERAL_SPEND,
                "value": {
                    "ada": { "lovelace": 100_000_000u64 },
                    POOL_POLICY: { "abc": 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("pool", &tx)]);
        assert!(types(&hits).contains(&"create_pool"), "{:?}", types(&hits));
        assert_eq!(hits[0].1.data["dapp"], "FluidTokens");
    }

    #[test]
    fn detects_open_loan_via_mint() {
        let s = Scanner::new();
        let tx = json!({
            "mint": { LOAN_POLICY: { "loan1": 1 }, BOND_POLICY: { "b1": 1 } },
            "outputs": [{
                "address": GENERAL_SPEND,
                "value": {
                    "ada": { "lovelace": 50_000_000u64 },
                    LOAN_POLICY: { "loan1": 1 }
                }
            }]
        });
        assert_eq!(types(&s.scan_block(&[("loan", &tx)])), vec!["open_loan"]);
    }

    #[test]
    fn detects_repay_via_withdrawal_action() {
        let s = Scanner::new();
        let tx = json!({
            "withdrawals": { ACTION_REPAY: 0 },
            "inputs": [{ "address": GENERAL_SPEND, "transaction": { "id": "x" }, "index": 0 }],
            "outputs": [{
                "address": "addr1q8testuser0000000000000000000000000000000000000000000000000000000000000000000000000000000qwerty",
                "value": { "ada": { "lovelace": 2_000_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("repay", &tx)]);
        assert!(types(&hits).contains(&"repay_loan"), "{:?}", types(&hits));
    }

    #[test]
    fn detects_liquidate_action() {
        let s = Scanner::new();
        let tx = json!({
            "withdrawals": { ACTION_LIQUIDATE: 0 },
            "outputs": [{
                "address": GENERAL_SPEND,
                "value": { "ada": { "lovelace": 1_000_000u64 } }
            }]
        });
        assert!(types(&s.scan_block(&[("liq", &tx)])).contains(&"liquidate_loan"));
    }

    #[test]
    fn detects_aquarium_tank() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": AQUARIUM_TANK,
                "value": { "ada": { "lovelace": 5_000_000u64 } }
            }]
        });
        assert_eq!(types(&s.scan_block(&[("aq", &tx)])), vec!["aquarium_tank"]);
    }

    #[test]
    fn detects_fldt_stake_on_aquarium_staker() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": AQUARIUM_STAKER,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    FLDT_POLICY: { FLDT_NAME_HEX: 10_000_000u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("fldt", &tx)]);
        assert_eq!(types(&hits), vec!["fldt_stake"]);
        assert_eq!(hits[0].1.data["fldt"], 10_000_000);
    }

    #[test]
    fn detects_legacy_lend() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": LEGACY_LEND_ADDR,
                "value": { "ada": { "lovelace": 25_000_000u64 } }
            }]
        });
        assert_eq!(types(&s.scan_block(&[("leg", &tx)])), vec!["legacy_lend"]);
    }

    #[test]
    fn ignores_unrelated() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": "addr1q8testuser0000000000000000000000000000000000000000000000000000000000000000000000000000000qwerty",
                "value": { "ada": { "lovelace": 2_000_000u64 } }
            }]
        });
        assert!(s.scan_block(&[("x", &tx)]).is_empty());
    }
}
