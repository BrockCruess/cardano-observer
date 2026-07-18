//! Indigo Protocol on-chain activity detection (mainnet V3).
//!
//! Sources:
//! - https://docs.indigoprotocol.io/
//! - https://config.indigoprotocol.io/mainnet/mainnet-system-params-v3.json
//! - Indigo MCP / SDK write actions (CDP, stability pool, staking, ROB, …)
//!
//! Tracked events (heuristic classification from script credentials + auth
//! token mint/burn + value flow):
//! - Open / Close CDP
//! - Mint / Burn iAsset
//! - Deposit / Withdraw collateral
//! - Liquidate CDP / Redeem against CDP
//! - Stability pool: create / adjust / close account
//! - INDY staking: open / adjust / close
//! - ROB: open / cancel / redeem
//! - Stableswap order
//! - Interest payment
//! - Governance (poll / vote touch)

use super::DappHit;
use crate::dex::payment_credential;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

const DAPP: &str = "Indigo Protocol";

/// Synthetic iAsset policy (iUSD, iBTC, iETH, …).
const IASSET_POLICY: &str = "f66d78b4a3cb3d37afa0ec36461e51ecbde00f26c8f0a68f94b69880";
const INDY_POLICY: &str = "533bb94a8850ee3ccbe483106489399112b74c905342cb1792a797a0";
const INDY_NAME_HEX: &str = "494e4459"; // INDY

/// CDP position NFT policy / asset name.
const CDP_AUTH_POLICY: &str = "708f5e6d597fc038d09a738d7be32edd6ea779d6feb32a53668d9050";
const CDP_AUTH_NAME: &str = "434450"; // CDP

/// Stability-pool account NFT.
const SP_ACCOUNT_POLICY: &str = "443c51db609bba8b2aa4c8af248bf797cbfcfa1e413c443296a50813";
const SP_ACCOUNT_NAME: &str = "53505f4143434f554e54"; // SP_ACCOUNT

/// INDY staking position NFT.
const STAKING_POS_POLICY: &str = "fd0d72fafee1d230a74c31ac503a192abd5b71888ae3f94128c1e634";
const STAKING_POS_NAME: &str = "5354414b494e475f504f534954494f4e"; // STAKING_POSITION

/// Stability pool auth token (marks the pool UTxO).
const SP_TOKEN_POLICY: &str = "3f28fb7d6c40468262dffb1c3adb568b342499826b664d940085d022";

/// Mainnet V3 validator payment credentials (= script hashes from system params).
const CDP_CREATOR_HASH: &str = "95c1458dad9dfddfab22d163fccbb9a8de483ace1e1c990b1f5e3eff";
const CDP_HASH: &str = "ff0b10bff20e4b68b491492e5ba6c8048a704763b0a45ce2995da0be";
const STABILITY_POOL_HASH: &str = "1c53ed6f616687b340ac83072ec65a9787583c01d6bae0314e1d61d0";
const STAKING_HASH: &str = "112996eb011f20eacc28ded86cde177a53ed22264e0131199c983a7e";
const ROB_HASH: &str = "d5c1360584a962b43f6acd8a511e23d28896deb9c45a218db3bc857c";
const STABLESWAP_HASH: &str = "245425bb49411414dca4af8f25097c5a27f923249c0779ffaa2bc5a3";
const INTEREST_HASH: &str = "2cac220a78a8943ed132b2cd2a24f2d26e80bdd66ab54d47517ffc04";
const GOV_HASH: &str = "adc336ca673fd1568b58ee970220110f8d6ef8dd6ec2b12b1dc1a64b";
const POLL_MANAGER_HASH: &str = "2c64c712d8ea012866a0e52119409992dcbee828b5e89f58b6c13c68";
const POLL_SHARD_HASH: &str = "814dc78cc5e5076ecffee9a2a0d9e0502531cc5d3f93de90b01c5a03";
const EXECUTE_HASH: &str = "0e764163b06d954f2dcfecf8d3b257e2bfe3d326d5670e54ca1974ce";
const IASSET_STATE_HASH: &str = "a9c613a0e6f6bef5a4f6b1d15f8bdd5b1105fede0a3c380d1a920028";

const TRACKER_CAP: usize = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EventType {
    OpenCdp,
    CloseCdp,
    MintIasset,
    BurnIasset,
    DepositCollateral,
    WithdrawCollateral,
    LiquidateCdp,
    RedeemCdp,
    CreateSpAccount,
    AdjustSpAccount,
    CloseSpAccount,
    OpenStaking,
    AdjustStaking,
    CloseStaking,
    OpenRob,
    CancelRob,
    RedeemRob,
    StableswapOrder,
    PayInterest,
    Governance,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenCdp => "open_cdp",
            Self::CloseCdp => "close_cdp",
            Self::MintIasset => "mint_iasset",
            Self::BurnIasset => "burn_iasset",
            Self::DepositCollateral => "deposit_collateral",
            Self::WithdrawCollateral => "withdraw_collateral",
            Self::LiquidateCdp => "liquidate_cdp",
            Self::RedeemCdp => "redeem_cdp",
            Self::CreateSpAccount => "create_sp_account",
            Self::AdjustSpAccount => "adjust_sp_account",
            Self::CloseSpAccount => "close_sp_account",
            Self::OpenStaking => "open_staking",
            Self::AdjustStaking => "adjust_staking",
            Self::CloseStaking => "close_staking",
            Self::OpenRob => "open_rob",
            Self::CancelRob => "cancel_rob",
            Self::RedeemRob => "redeem_rob",
            Self::StableswapOrder => "stableswap_order",
            Self::PayInterest => "pay_interest",
            Self::Governance => "governance",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::OpenCdp => "Open CDP - Indigo",
            Self::CloseCdp => "Close CDP - Indigo",
            Self::MintIasset => "Mint iAsset - Indigo",
            Self::BurnIasset => "Burn iAsset - Indigo",
            Self::DepositCollateral => "Deposit Collateral - Indigo",
            Self::WithdrawCollateral => "Withdraw Collateral - Indigo",
            Self::LiquidateCdp => "Liquidation - Indigo",
            Self::RedeemCdp => "Redemption - Indigo",
            Self::CreateSpAccount => "Stability Pool Deposit - Indigo",
            Self::AdjustSpAccount => "Stability Pool Adjust - Indigo",
            Self::CloseSpAccount => "Stability Pool Close - Indigo",
            Self::OpenStaking => "Stake INDY - Indigo",
            Self::AdjustStaking => "Adjust INDY Stake - Indigo",
            Self::CloseStaking => "Unstake INDY - Indigo",
            Self::OpenRob => "ROB Order - Indigo",
            Self::CancelRob => "Cancel ROB - Indigo",
            Self::RedeemRob => "ROB Redeem - Indigo",
            Self::StableswapOrder => "Stableswap - Indigo",
            Self::PayInterest => "Pay Interest - Indigo",
            Self::Governance => "Governance - Indigo",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    CdpCreator,
    Cdp,
    StabilityPool,
    Staking,
    Rob,
    Stableswap,
    Interest,
    Governance,
    /// iAsset / protocol state UTxOs at the iasset validator.
    IassetState,
}

#[derive(Default, Clone, Debug)]
struct ValueSummary {
    ada: u64,
    indy: u64,
    /// Sum of all tokens under the iAsset policy.
    iasset: u64,
    /// Primary iAsset ticker seen (if any).
    iasset_ticker: Option<String>,
    iasset_name_hex: Option<String>,
    cdp_nft_delta: i64,
    sp_account_delta: i64,
    staking_pos_delta: i64,
    has_cdp_nft: bool,
    has_sp_account: bool,
    has_staking_pos: bool,
    has_sp_token: bool,
}

impl ValueSummary {
    fn merge(&mut self, other: &ValueSummary) {
        self.ada = self.ada.saturating_add(other.ada);
        self.indy = self.indy.saturating_add(other.indy);
        self.iasset = self.iasset.saturating_add(other.iasset);
        self.cdp_nft_delta = self.cdp_nft_delta.saturating_add(other.cdp_nft_delta);
        self.sp_account_delta = self.sp_account_delta.saturating_add(other.sp_account_delta);
        self.staking_pos_delta = self.staking_pos_delta.saturating_add(other.staking_pos_delta);
        self.has_cdp_nft |= other.has_cdp_nft;
        self.has_sp_account |= other.has_sp_account;
        self.has_staking_pos |= other.has_staking_pos;
        self.has_sp_token |= other.has_sp_token;
        if self.iasset_ticker.is_none() {
            self.iasset_ticker.clone_from(&other.iasset_ticker);
            self.iasset_name_hex.clone_from(&other.iasset_name_hex);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SpentAmounts {
    ada: u64,
    indy: u64,
    iasset: u64,
    /// iAsset on spent stability-pool *state* UTxOs (carry `STABILITY_POOL` token).
    pool_iasset: u64,
    /// iAsset on other spent SP-script UTxOs (accounts / pending requests).
    sp_other_iasset: u64,
}

#[derive(Clone, Copy, Debug)]
struct TrackedUtxo {
    role: Role,
    ada: u64,
    indy: u64,
    iasset: u64,
    /// Stability pool state UTxO (has `STABILITY_POOL` auth token).
    is_sp_pool: bool,
}

/// iAsset flows at the stability-pool script for deposit sizing.
#[derive(Clone, Copy, Debug, Default)]
struct SpFlows {
    pool_out: u64,
    account_out: u64,
    pool_spent: u64,
    other_spent: u64,
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

/// Stateful Indigo scanner (tracks script UTxOs for spend / net-flow detection).
pub struct Scanner {
    roles: HashMap<String, Role>,
    tracked: Mutex<TrackedSet>,
}

impl Scanner {
    pub fn new() -> Self {
        let mut roles = HashMap::new();
        for (hash, role) in [
            (CDP_CREATOR_HASH, Role::CdpCreator),
            (CDP_HASH, Role::Cdp),
            (STABILITY_POOL_HASH, Role::StabilityPool),
            (STAKING_HASH, Role::Staking),
            (ROB_HASH, Role::Rob),
            (STABLESWAP_HASH, Role::Stableswap),
            (INTEREST_HASH, Role::Interest),
            (GOV_HASH, Role::Governance),
            (POLL_MANAGER_HASH, Role::Governance),
            (POLL_SHARD_HASH, Role::Governance),
            (EXECUTE_HASH, Role::Governance),
            (IASSET_STATE_HASH, Role::IassetState),
        ] {
            roles.insert(hash.to_string(), role);
        }
        // DefiLlama-published staking script address (same credential as STAKING_HASH).
        if let Some(cred) =
            payment_credential("addr1wx3r0yl49yteuzwwlv7r0lr2uzq7p6v7nxl9ek645qy5rfgwwzxw6")
        {
            roles.insert(cred, Role::Staking);
        }
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

    /// Replay persisted tx bodies into the UTxO tracker without emitting events.
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
        tracing::info!("indigo: warmed script UTxO tracker from {n} cached txs");
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

        let mut spent_roles: HashMap<Role, SpentAmounts> = HashMap::new();
        let mut spent_any = false;
        {
            let mut tracked = self.tracked.lock().unwrap();
            for input in inputs {
                let Some(op) = input_outpoint(input) else { continue };
                if let Some(u) = tracked.take(&op) {
                    spent_any = true;
                    let s = spent_roles.entry(u.role).or_default();
                    s.ada = s.ada.saturating_add(u.ada);
                    s.indy = s.indy.saturating_add(u.indy);
                    s.iasset = s.iasset.saturating_add(u.iasset);
                    if u.role == Role::StabilityPool {
                        if u.is_sp_pool {
                            s.pool_iasset = s.pool_iasset.saturating_add(u.iasset);
                        } else {
                            s.sp_other_iasset = s.sp_other_iasset.saturating_add(u.iasset);
                        }
                    }
                }
            }
        }

        // Also treat known script inputs (credential on input address when present).
        for input in inputs {
            let addr = input.get("address").and_then(Value::as_str).unwrap_or("");
            if let Some(role) = self.role_for_addr(addr) {
                spent_roles.entry(role).or_default();
                spent_any = true;
            }
        }

        let spent_sp = spent_roles
            .get(&Role::StabilityPool)
            .copied()
            .unwrap_or_default();
        let mut sp_flows = SpFlows {
            pool_spent: spent_sp.pool_iasset,
            other_spent: spent_sp.sp_other_iasset,
            ..SpFlows::default()
        };
        for &(addr, ref sum) in &output_sums {
            if self.role_for_addr(addr) != Some(Role::StabilityPool) {
                continue;
            }
            if sum.has_sp_token {
                sp_flows.pool_out = sp_flows.pool_out.saturating_add(sum.iasset);
            } else if sum.has_sp_account {
                sp_flows.account_out = sp_flows.account_out.saturating_add(sum.iasset);
            }
        }

        let hits = if emit {
            classify(tx_hash, tx, &mint, &by_role, &spent_roles, spent_any, sp_flows)
        } else {
            Vec::new()
        };

        {
            let mut tracked = self.tracked.lock().unwrap();
            for (index, &(addr, ref sum)) in output_sums.iter().enumerate() {
                let Some(role) = self.role_for_addr(addr) else { continue };
                if !matches!(
                    role,
                    Role::Cdp
                        | Role::StabilityPool
                        | Role::Staking
                        | Role::Rob
                        | Role::CdpCreator
                ) {
                    continue;
                }
                let keep = sum.ada > 0
                    || sum.iasset > 0
                    || sum.indy > 0
                    || sum.has_cdp_nft
                    || sum.has_sp_account
                    || sum.has_staking_pos
                    || sum.has_sp_token;
                if !keep {
                    continue;
                }
                tracked.insert(
                    format!("{tx_hash}#{index}"),
                    TrackedUtxo {
                        role,
                        ada: sum.ada,
                        indy: sum.indy,
                        iasset: sum.iasset,
                        is_sp_pool: sum.has_sp_token,
                    },
                );
            }
        }

        hits
    }
}

fn classify(
    tx_hash: &str,
    tx: &Value,
    mint: &ValueSummary,
    by_role: &HashMap<Role, ValueSummary>,
    spent_roles: &HashMap<Role, SpentAmounts>,
    spent_any: bool,
    sp_flows: SpFlows,
) -> Vec<(String, DappHit)> {
    let auth_touch =
        mint.cdp_nft_delta != 0 || mint.sp_account_delta != 0 || mint.staking_pos_delta != 0;
    let script_touch = !by_role.is_empty() || !spent_roles.is_empty() || spent_any;
    if !script_touch && !auth_touch {
        return Vec::new();
    }

    let mut pending: Vec<(EventType, ValueSummary)> = Vec::new();
    let mut opened_cdp = false;
    let mut closed_cdp = false;
    let mut liquidated = false;
    let mut opened_staking = false;
    let mut closed_staking = false;

    let cdp_out = by_role.get(&Role::Cdp);
    let sp_out = by_role.get(&Role::StabilityPool);
    let staking_out = by_role.get(&Role::Staking);
    let rob_out = by_role.get(&Role::Rob);
    let spent_cdp = spent_roles.get(&Role::Cdp).copied().unwrap_or_default();
    let spent_sp = spent_roles.get(&Role::StabilityPool).copied().unwrap_or_default();
    let spent_staking = spent_roles.get(&Role::Staking).copied().unwrap_or_default();
    let spent_rob = spent_roles.contains_key(&Role::Rob);
    let touches_sp = sp_out.is_some() || spent_roles.contains_key(&Role::StabilityPool);
    let touches_cdp = cdp_out.is_some()
        || spent_roles.contains_key(&Role::Cdp)
        || by_role.contains_key(&Role::CdpCreator)
        || spent_roles.contains_key(&Role::CdpCreator);
    let touches_interest = by_role.contains_key(&Role::Interest)
        || spent_roles.contains_key(&Role::Interest);
    let touches_gov = by_role.contains_key(&Role::Governance)
        || spent_roles.contains_key(&Role::Governance);
    let touches_stableswap = by_role.contains_key(&Role::Stableswap)
        || spent_roles.contains_key(&Role::Stableswap);

    if mint.cdp_nft_delta > 0 {
        let mut sum = cdp_out.cloned().unwrap_or_default();
        if mint.iasset > 0 {
            sum.iasset = mint.iasset;
            sum.iasset_ticker.clone_from(&mint.iasset_ticker);
            sum.iasset_name_hex.clone_from(&mint.iasset_name_hex);
        }
        pending.push((EventType::OpenCdp, sum));
        opened_cdp = true;
    } else if mint.cdp_nft_delta < 0 {
        let sum = ValueSummary {
            iasset: mint.iasset,
            iasset_ticker: mint.iasset_ticker.clone(),
            iasset_name_hex: mint.iasset_name_hex.clone(),
            ada: spent_cdp.ada,
            ..ValueSummary::default()
        };
        if touches_sp {
            pending.push((EventType::LiquidateCdp, sum));
            liquidated = true;
        } else {
            pending.push((EventType::CloseCdp, sum));
            closed_cdp = true;
        }
    }

    if mint.sp_account_delta > 0 {
        // Never attribute the whole pool balance — only the net deposit.
        let mut sum = ValueSummary {
            iasset: sp_deposit_amount(sp_flows),
            ..ValueSummary::default()
        };
        fill_sp_iasset_meta(&mut sum, sp_out, mint, tx);
        pending.push((EventType::CreateSpAccount, sum));
    } else if mint.sp_account_delta < 0 {
        // Closing spends the shared pool UTxO + the account UTxO. Account
        // positions usually hold no iAsset (it sits in the pool), so never use
        // total spent SP iAsset — that is the full pool balance.
        let mut sum = ValueSummary {
            iasset: sp_close_amount(sp_flows, spent_sp, tx),
            ..ValueSummary::default()
        };
        fill_sp_iasset_meta(&mut sum, sp_out, mint, tx);
        pending.push((EventType::CloseSpAccount, sum));
    }

    if mint.staking_pos_delta > 0 {
        let mut sum = staking_out.cloned().unwrap_or_default();
        if sum.indy == 0 {
            sum.indy = mint.indy;
        }
        pending.push((EventType::OpenStaking, sum));
        opened_staking = true;
    } else if mint.staking_pos_delta < 0 {
        pending.push((
            EventType::CloseStaking,
            ValueSummary {
                indy: spent_staking.indy.max(mint.indy),
                ..ValueSummary::default()
            },
        ));
        closed_staking = true;
    }

    if rob_out.is_some() && !spent_rob {
        pending.push((EventType::OpenRob, rob_out.cloned().unwrap_or_default()));
    } else if spent_rob && rob_out.is_none() {
        pending.push((EventType::CancelRob, ValueSummary::default()));
    } else if spent_rob && touches_cdp {
        let mut sum = ValueSummary::default();
        sum.iasset = mint.iasset;
        sum.iasset_ticker.clone_from(&mint.iasset_ticker);
        sum.iasset_name_hex.clone_from(&mint.iasset_name_hex);
        pending.push((EventType::RedeemRob, sum));
    }

    if mint.cdp_nft_delta == 0 && touches_cdp {
        let cdp_ada_out = cdp_out.map(|s| s.ada).unwrap_or(0);
        let net_ada = cdp_ada_out as i128 - spent_cdp.ada as i128;
        let iasset_minted = signed_iasset_mint(tx);
        if iasset_minted > 0 && !opened_cdp {
            let mut sum = ValueSummary {
                iasset: iasset_minted as u64,
                ..ValueSummary::default()
            };
            fill_iasset_meta(&mut sum, tx);
            pending.push((EventType::MintIasset, sum));
        } else if iasset_minted < 0 && !closed_cdp && !liquidated {
            let mut sum = ValueSummary {
                iasset: (-iasset_minted) as u64,
                ..ValueSummary::default()
            };
            fill_iasset_meta(&mut sum, tx);
            if spent_cdp.ada > cdp_ada_out && cdp_out.map(|s| s.has_cdp_nft).unwrap_or(true) {
                pending.push((EventType::RedeemCdp, sum));
            } else {
                pending.push((EventType::BurnIasset, sum));
            }
        } else if spent_cdp.ada > 0
            && net_ada > 1_000_000
            && !opened_cdp
            && iasset_minted == 0
        {
            // Only on CDP rewrites (spent prior CDP UTxO), not first-sight creates.
            pending.push((
                EventType::DepositCollateral,
                ValueSummary {
                    ada: net_ada as u64,
                    ..ValueSummary::default()
                },
            ));
        } else if spent_cdp.ada > 0
            && net_ada < -1_000_000
            && !closed_cdp
            && !liquidated
            && iasset_minted == 0
        {
            pending.push((
                EventType::WithdrawCollateral,
                ValueSummary {
                    ada: (-net_ada) as u64,
                    ..ValueSummary::default()
                },
            ));
        }
    }

    if mint.sp_account_delta == 0 && touches_sp && !liquidated {
        // Prefer net change on the pool state UTxO (SP_TOKEN), not every SP output.
        let net = if sp_flows.pool_spent > 0 || sp_flows.pool_out > 0 {
            sp_flows.pool_out as i128 - sp_flows.pool_spent as i128
        } else {
            let sp_iasset_out = sp_out.map(|s| s.iasset).unwrap_or(0);
            sp_iasset_out as i128 - spent_sp.iasset as i128
        };
        if net != 0
            && (sp_out.is_some_and(|s| s.has_sp_account)
                || spent_sp.iasset > 0
                || sp_out.is_some_and(|s| s.has_sp_token))
        {
            let mut sum = ValueSummary {
                iasset: net.unsigned_abs() as u64,
                ..ValueSummary::default()
            };
            fill_sp_iasset_meta(&mut sum, sp_out, mint, tx);
            pending.push((EventType::AdjustSpAccount, sum));
        }
    }

    if mint.staking_pos_delta == 0
        && (staking_out.is_some() || spent_roles.contains_key(&Role::Staking))
    {
        let out_indy = staking_out.map(|s| s.indy).unwrap_or(0);
        let net = out_indy as i128 - spent_staking.indy as i128;
        if net != 0 && !opened_staking && !closed_staking {
            pending.push((
                EventType::AdjustStaking,
                ValueSummary {
                    indy: net.unsigned_abs() as u64,
                    ..ValueSummary::default()
                },
            ));
        }
    }

    if touches_stableswap {
        pending.push((
            EventType::StableswapOrder,
            by_role
                .get(&Role::Stableswap)
                .cloned()
                .unwrap_or_default(),
        ));
    }

    if touches_interest && pending.is_empty() {
        pending.push((
            EventType::PayInterest,
            ValueSummary {
                ada: by_role.get(&Role::Interest).map(|s| s.ada).unwrap_or(0),
                ..ValueSummary::default()
            },
        ));
    }

    if touches_gov && pending.is_empty() {
        pending.push((
            EventType::Governance,
            ValueSummary {
                indy: by_role
                    .get(&Role::Governance)
                    .map(|s| s.indy)
                    .unwrap_or(0)
                    .max(
                        spent_roles
                            .get(&Role::Governance)
                            .map(|s| s.indy)
                            .unwrap_or(0),
                    ),
                ..ValueSummary::default()
            },
        ));
    }

    let actor = crate::parse::actor_from_tx(tx);
    pending
        .into_iter()
        .map(|(et, sum)| (tx_hash.to_string(), hit_for(et, &sum, actor.clone())))
        .collect()
}

/// Deposit size for a new stability-pool account.
///
/// The SP script also holds the shared pool state UTxO (often hundreds of
/// thousands of iAsset). Using that balance as the "deposit" is wrong — use:
/// 1. net increase on the `STABILITY_POOL` token UTxO when the prior is known
/// 2. else iAsset spent from non-pool SP UTxOs (pending request consolidation)
/// 3. else iAsset on the new `SP_ACCOUNT` output (rare / tests)
fn sp_deposit_amount(f: SpFlows) -> u64 {
    if f.pool_spent > 0 {
        return f.pool_out.saturating_sub(f.pool_spent);
    }
    if f.other_spent > 0 {
        return f.other_spent;
    }
    f.account_out
}

/// Withdrawal size when closing a stability-pool account.
///
/// Prefer the net decrease on the shared pool UTxO (account iAsset lives there).
/// Fall back to spent non-pool SP iAsset, then iAsset paid to non-script outputs.
fn sp_close_amount(f: SpFlows, spent: SpentAmounts, tx: &Value) -> u64 {
    if f.pool_spent > 0 {
        return f.pool_spent.saturating_sub(f.pool_out);
    }
    if spent.sp_other_iasset > 0 {
        return spent.sp_other_iasset;
    }
    if f.other_spent > 0 {
        return f.other_spent;
    }
    iasset_paid_to_user(tx)
}

/// iAsset on outputs that are not at the stability-pool script (user payout).
fn iasset_paid_to_user(tx: &Value) -> u64 {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut total = 0u64;
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if payment_credential(addr).as_deref() == Some(STABILITY_POOL_HASH)
            || addr == STABILITY_POOL_HASH
        {
            continue;
        }
        let sum = summarize_value(o.get("value"));
        total = total.saturating_add(sum.iasset);
    }
    total
}

/// Resolve which iAsset pool an SP event belongs to (iUSD / iBTC / …).
///
/// Indigo has one stability pool per iAsset; there is no separate pool-id NFT
/// (`SP_ACCOUNT` / `STABILITY_POOL` names are fixed). The ticker *is* the pool id.
fn fill_sp_iasset_meta(
    sum: &mut ValueSummary,
    sp_out: Option<&ValueSummary>,
    mint: &ValueSummary,
    tx: &Value,
) {
    if let Some(s) = sp_out {
        if sum.iasset_ticker.is_none() {
            sum.iasset_ticker.clone_from(&s.iasset_ticker);
            sum.iasset_name_hex.clone_from(&s.iasset_name_hex);
        }
    }
    if sum.iasset_ticker.is_none() {
        sum.iasset_ticker.clone_from(&mint.iasset_ticker);
        sum.iasset_name_hex.clone_from(&mint.iasset_name_hex);
    }
    if sum.iasset_ticker.is_none() {
        fill_iasset_meta(sum, tx);
    }
    if sum.iasset_ticker.is_none() {
        // Scan output values (close burns only SP_ACCOUNT; iAsset sits on the pool).
        let empty = Vec::new();
        let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
        for o in outputs {
            let s = summarize_value(o.get("value"));
            if s.iasset_ticker.is_some() {
                sum.iasset_ticker = s.iasset_ticker;
                sum.iasset_name_hex = s.iasset_name_hex;
                break;
            }
        }
    }
}

fn signed_iasset_mint(tx: &Value) -> i128 {
    let Some(v) = tx.get("mint").and_then(|m| m.get(IASSET_POLICY)) else {
        return 0;
    };
    let Some(obj) = v.as_object() else { return 0 };
    obj.values().filter_map(Value::as_i64).map(i128::from).sum()
}

fn fill_iasset_meta(sum: &mut ValueSummary, tx: &Value) {
    let Some(v) = tx.get("mint").and_then(|m| m.get(IASSET_POLICY)) else {
        return;
    };
    let Some(obj) = v.as_object() else { return };
    if let Some((name, _)) = obj.iter().find(|(_, q)| q.as_i64().unwrap_or(0) != 0) {
        sum.iasset_name_hex = Some(name.clone());
        sum.iasset_ticker = decode_ticker(name);
    }
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
            EventType::OpenCdp
                | EventType::CloseCdp
                | EventType::DepositCollateral
                | EventType::WithdrawCollateral
                | EventType::LiquidateCdp
                | EventType::OpenRob
                | EventType::PayInterest
        )
    {
        obj.insert("ada".into(), json!(sum.ada));
    }

    if sum.indy > 0
        && matches!(
            et,
            EventType::OpenStaking
                | EventType::AdjustStaking
                | EventType::CloseStaking
                | EventType::Governance
        )
    {
        obj.insert("indy".into(), json!(sum.indy));
        obj.insert(
            "assets".into(),
            json!({
                "items": [{
                    "unit": format!("{INDY_POLICY}{INDY_NAME_HEX}"),
                    "policy": INDY_POLICY,
                    "nameHex": INDY_NAME_HEX,
                    "name": "INDY",
                    "qty": sum.indy.to_string(),
                    "ticker": "INDY",
                    "decimals": 6,
                }],
                "more": 0
            }),
        );
    }

    // Always surface which SP / iAsset this event belongs to when known
    // (one pool per iAsset; the ticker is the pool identifier).
    if let Some(ticker) = &sum.iasset_ticker {
        obj.insert("iasset".into(), json!(ticker));
    }

    if sum.iasset > 0 {
        if let Some(name_hex) = &sum.iasset_name_hex {
            let ticker = sum.iasset_ticker.clone().unwrap_or_else(|| "iAsset".into());
            obj.insert(
                "assets".into(),
                json!({
                    "items": [{
                        "unit": format!("{IASSET_POLICY}{name_hex}"),
                        "policy": IASSET_POLICY,
                        "nameHex": name_hex,
                        "name": ticker,
                        "qty": sum.iasset.to_string(),
                        "ticker": ticker,
                        "decimals": 6,
                    }],
                    "more": 0
                }),
            );
        } else {
            obj.insert("iassetQty".into(), json!(sum.iasset));
        }
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
            if policy == INDY_POLICY && name == INDY_NAME_HEX {
                s.indy = s.indy.saturating_add(q);
            } else if policy == IASSET_POLICY {
                s.iasset = s.iasset.saturating_add(q);
                if s.iasset_ticker.is_none() && !name.is_empty() {
                    s.iasset_name_hex = Some(name.clone());
                    s.iasset_ticker = decode_ticker(name);
                }
            } else if policy == CDP_AUTH_POLICY && name == CDP_AUTH_NAME {
                s.has_cdp_nft = true;
                s.cdp_nft_delta = s.cdp_nft_delta.saturating_add(signed);
            } else if policy == SP_ACCOUNT_POLICY && name == SP_ACCOUNT_NAME {
                s.has_sp_account = true;
                s.sp_account_delta = s.sp_account_delta.saturating_add(signed);
            } else if policy == STAKING_POS_POLICY && name == STAKING_POS_NAME {
                s.has_staking_pos = true;
                s.staking_pos_delta = s.staking_pos_delta.saturating_add(signed);
            } else if policy == SP_TOKEN_POLICY {
                s.has_sp_token = true;
            }
        }
    }
    s
}

fn decode_ticker(name_hex: &str) -> Option<String> {
    let bytes = hex::decode(name_hex).ok()?;
    let s = String::from_utf8(bytes).ok()?;
    if s.chars().all(|c| c.is_ascii_graphic()) {
        Some(s)
    } else {
        None
    }
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

    fn script_addr(payment_hash_hex: &str) -> String {
        // Build a mainnet script + key-stake enterprise-like address via known
        // bech32 helpers in parse — use credential matching on payment hash by
        // embedding hash as raw addr key in role map (scanner matches hash too).
        payment_hash_hex.to_string()
    }

    #[test]
    fn detects_open_cdp_with_mint() {
        let s = Scanner::new();
        let cdp = script_addr(CDP_HASH);
        let tx = json!({
            "mint": {
                CDP_AUTH_POLICY: { CDP_AUTH_NAME: 1 },
                IASSET_POLICY: { "69555344": 1_000_000_000i64 }
            },
            "outputs": [{
                "address": cdp,
                "value": {
                    "ada": { "lovelace": 50_000_000u64 },
                    CDP_AUTH_POLICY: { CDP_AUTH_NAME: 1 }
                }
            }, {
                "address": "addr1q8testuser0000000000000000000000000000000000000000000000000000000000000000000000000000000qwerty",
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    IASSET_POLICY: { "69555344": 1_000_000_000u64 }
                }
            }]
        });
        // role_for_addr needs real bech32 OR we pass payment hash as address —
        // Scanner matches roles.get(addr), so using raw hash works.
        let hits = s.scan_block(&[("open", &tx)]);
        assert!(types(&hits).contains(&"open_cdp"), "{:?}", types(&hits));
        assert_eq!(hits[0].1.data["dapp"], "Indigo Protocol");
        assert_eq!(hits[0].1.data["iasset"], "iUSD");
    }

    #[test]
    fn detects_liquidate_vs_close() {
        let s = Scanner::new();
        let cdp = CDP_HASH;
        let sp = STABILITY_POOL_HASH;
        // Seed a CDP UTxO in the tracker.
        let fund = json!({
            "outputs": [{
                "address": cdp,
                "value": {
                    "ada": { "lovelace": 40_000_000u64 },
                    CDP_AUTH_POLICY: { CDP_AUTH_NAME: 1 }
                }
            }]
        });
        assert!(s.scan_block(&[("fund", &fund)]).is_empty());

        let liq = json!({
            "mint": {
                CDP_AUTH_POLICY: { CDP_AUTH_NAME: -1 },
                IASSET_POLICY: { "69555344": -500_000_000i64 }
            },
            "inputs": [{ "transaction": { "id": "fund" }, "index": 0 }],
            "outputs": [{
                "address": sp,
                "value": {
                    "ada": { "lovelace": 35_000_000u64 },
                    SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 }
                }
            }]
        });
        let hits = s.scan_block(&[("liq", &liq)]);
        assert!(types(&hits).contains(&"liquidate_cdp"), "{:?}", types(&hits));
    }

    #[test]
    fn detects_sp_account_create() {
        let s = Scanner::new();
        let sp = STABILITY_POOL_HASH;
        let tx = json!({
            "mint": {
                SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 }
            },
            "outputs": [{
                "address": sp,
                "value": {
                    "ada": { "lovelace": 5_000_000u64 },
                    SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 },
                    IASSET_POLICY: { "69555344": 2_000_000_000u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("sp", &tx)]);
        assert_eq!(types(&hits), vec!["create_sp_account"]);
        // iUSD on the new account output (no prior pool) is the deposit.
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "2000000000");
    }

    #[test]
    fn sp_deposit_uses_pool_net_not_full_balance() {
        // Mirrors c1ce2f25…: rewrite pool UTxO (+105 iUSD) while minting SP_ACCOUNT.
        // Must not report the entire ~579k pool balance as the deposit.
        let s = Scanner::new();
        let sp = STABILITY_POOL_HASH;
        let fund_pool = json!({
            "outputs": [{
                "address": sp,
                "value": {
                    "ada": { "lovelace": 537_992_488_392u64 },
                    SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 },
                    IASSET_POLICY: { "69555344": 579_373_658_500u64 }
                }
            }]
        });
        // Seed tracker with prior pool state (may emit an unrelated adjust event).
        let _ = s.scan_block(&[("fund_pool", &fund_pool)]);

        let deposit = json!({
            "mint": {
                SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 }
            },
            "inputs": [{ "transaction": { "id": "fund_pool" }, "index": 0 }],
            "outputs": [
                {
                    "address": sp,
                    "value": {
                        "ada": { "lovelace": 537_997_488_392u64 },
                        SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 },
                        IASSET_POLICY: { "69555344": 579_478_658_500u64 }
                    }
                },
                {
                    "address": sp,
                    "value": {
                        "ada": { "lovelace": 2_368_822u64 },
                        SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("dep", &deposit)]);
        assert_eq!(types(&hits), vec!["create_sp_account"]);
        // 579478658500 - 579373658500 = 105000000 (105 iUSD, 6 decimals)
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "105000000");
    }

    #[test]
    fn sp_deposit_omits_amount_when_prior_pool_unknown() {
        // Without a tracked prior pool, do not treat the full pool UTxO as a deposit.
        let s = Scanner::new();
        let sp = STABILITY_POOL_HASH;
        let tx = json!({
            "mint": {
                SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 }
            },
            "outputs": [
                {
                    "address": sp,
                    "value": {
                        "ada": { "lovelace": 537_997_488_392u64 },
                        SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 },
                        IASSET_POLICY: { "69555344": 579_478_658_500u64 }
                    }
                },
                {
                    "address": sp,
                    "value": {
                        "ada": { "lovelace": 2_368_822u64 },
                        SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: 1 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("blind", &tx)]);
        assert_eq!(types(&hits), vec!["create_sp_account"]);
        assert!(hits[0].1.data.get("assets").is_none());
    }

    #[test]
    fn sp_close_uses_pool_net_and_iasset_ticker() {
        // Mirrors 764988fa…: burn SP_ACCOUNT while rewriting the iBTC pool.
        // Must report withdrawn net (not full pool) and label the iBTC pool.
        let s = Scanner::new();
        let sp = STABILITY_POOL_HASH;
        let fund_pool = json!({
            "outputs": [{
                "address": sp,
                "value": {
                    "ada": { "lovelace": 600_000_000_000u64 },
                    SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 },
                    IASSET_POLICY: { "69425443": 3_757_309u64 }
                }
            }]
        });
        let _ = s.scan_block(&[("fund_ibtc", &fund_pool)]);

        let close = json!({
            "mint": {
                SP_ACCOUNT_POLICY: { SP_ACCOUNT_NAME: -1 }
            },
            "inputs": [
                { "transaction": { "id": "fund_ibtc" }, "index": 0 },
                { "transaction": { "id": "acct" }, "index": 0 }
            ],
            "outputs": [
                {
                    "address": sp,
                    "value": {
                        "ada": { "lovelace": 600_000_000_000u64 },
                        SP_TOKEN_POLICY: { "53544142494c4954595f504f4f4c": 1 },
                        IASSET_POLICY: { "69425443": 3_754_106u64 }
                    }
                },
                {
                    "address": "addr1q8yaqrgnhgqvz6gnw42y5szh2vtmf2m64pr0e64e5tu9pd6xadtk57euser000000000000000000000000000000000000000qwerty",
                    "value": {
                        "ada": { "lovelace": 2_316_616u64 },
                        IASSET_POLICY: { "69425443": 3_203u64 }
                    }
                }
            ]
        });
        let hits = s.scan_block(&[("close", &close)]);
        assert_eq!(types(&hits), vec!["close_sp_account"]);
        assert_eq!(hits[0].1.data["iasset"], "iBTC");
        assert_eq!(hits[0].1.data["assets"]["items"][0]["qty"], "3203");
    }

    #[test]
    fn detects_indy_stake_open() {
        let s = Scanner::new();
        let staking = STAKING_HASH;
        let tx = json!({
            "mint": {
                STAKING_POS_POLICY: { STAKING_POS_NAME: 1 }
            },
            "outputs": [{
                "address": staking,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    STAKING_POS_POLICY: { STAKING_POS_NAME: 1 },
                    INDY_POLICY: { INDY_NAME_HEX: 100_000_000u64 }
                }
            }]
        });
        let hits = s.scan_block(&[("stake", &tx)]);
        assert_eq!(types(&hits), vec!["open_staking"]);
        assert_eq!(hits[0].1.data["indy"], 100_000_000);
    }

    #[test]
    fn detects_open_rob() {
        let s = Scanner::new();
        let rob = ROB_HASH;
        let tx = json!({
            "outputs": [{
                "address": rob,
                "value": { "ada": { "lovelace": 25_000_000u64 } }
            }]
        });
        let hits = s.scan_block(&[("rob", &tx)]);
        assert_eq!(types(&hits), vec!["open_rob"]);
    }

    #[test]
    fn ignores_unrelated_iusd_transfer() {
        let s = Scanner::new();
        let tx = json!({
            "outputs": [{
                "address": "addr1q8testuser0000000000000000000000000000000000000000000000000000000000000000000000000000000qwerty",
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    IASSET_POLICY: { "69555344": 1_000_000u64 }
                }
            }]
        });
        assert!(s.scan_block(&[("xfer", &tx)]).is_empty());
    }
}
