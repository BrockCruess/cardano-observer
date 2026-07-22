//! Turns Ogmios v6 chain-sync blocks (JSON) into a stream of `ChainEvent`s.
//!
//! Parsing is deliberately `serde_json::Value`-based and defensive: the Ogmios
//! schema is large and era-dependent, and we'd rather drop a field than crash
//! the sync loop on an unexpected shape.

use crate::config::Network;
use crate::dapp::DappRegistry;
use crate::deleg::DelegationTracker;
use crate::dapp::dex::DexRegistry;
use crate::model::ChainEvent;
use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32, Hrp};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Everything extracted from one block.
pub struct ParsedBlock {
    pub hash: String,
    pub height: u64,
    pub slot: u64,
    pub events: Vec<ChainEvent>,
    /// (tx_hash, raw tx JSON) pairs for the detail cache
    pub txs: Vec<(String, Value)>,
}

struct EventBuilder {
    slot: u64,
    height: u64,
    block_hash: String,
    timestamp: i64,
}

impl EventBuilder {
    fn make(
        &self,
        kind: &'static str,
        category: &'static str,
        tx_hash: Option<&str>,
        title: String,
        summary: String,
        data: Value,
    ) -> ChainEvent {
        ChainEvent {
            id: 0, // assigned by state when published
            parent_id: None, // wired up in the publish loop (see ogmios.rs)
            kind: kind.into(),
            category: category.into(),
            slot: self.slot,
            height: Some(self.height),
            block_hash: Some(self.block_hash.clone()),
            tx_hash: tx_hash.map(str::to_string),
            timestamp: self.timestamp,
            title,
            summary,
            data,
        }
    }
}

pub fn parse_block(
    block: &Value,
    timestamp: i64,
    network: Network,
    dex: &DexRegistry,
    dapp: &DappRegistry,
    deleg: &DelegationTracker,
) -> Option<ParsedBlock> {
    let hash = block.get("id")?.as_str()?.to_string();
    let slot = block.get("slot").and_then(Value::as_u64).unwrap_or(0);
    let height = block.get("height").and_then(Value::as_u64).unwrap_or(0);
    let b = EventBuilder { slot, height, block_hash: hash.clone(), timestamp };

    let empty = Vec::new();
    let txs = block.get("transactions").and_then(Value::as_array).unwrap_or(&empty);

    let mut events = Vec::new();
    let mut cached_txs = Vec::new();

    // ── Block event ──────────────────────────────────────────────────────
    let size = block
        .get("size")
        .and_then(|s| s.get("bytes").and_then(Value::as_u64).or_else(|| s.as_u64()))
        .unwrap_or(0);
    let issuer_pool = block
        .get("issuer")
        .and_then(|i| i.get("verificationKey"))
        .and_then(Value::as_str)
        .and_then(pool_id_from_vkey);
    let total_fees: u64 = txs.iter().filter_map(lovelace_fee).sum();
    let total_output: u64 = txs.iter().map(tx_output_lovelace).sum();

    events.push(b.make(
        "block",
        "block",
        None,
        format!("Block {height}"),
        format!("{} tx", txs.len()),
        json!({
            "hash": hash,
            "height": height,
            "slot": slot,
            "size": size,
            "txCount": txs.len(),
            "issuerPool": issuer_pool,
            "totalFees": total_fees,
            "totalOutput": total_output,
            "era": block.get("era").and_then(Value::as_str),
        }),
    ));

    // ── Per-transaction events ───────────────────────────────────────────
    // DEX / dApp: two-pass over the whole block so place+fill (any tx order) → one Swap.
    // Note spend-graph hubs after each tx so same-block claim chains can omit
    // shared batcher edges from the next tx's inputTxs.
    let mut scan_txs: Vec<(&str, &Value)> = Vec::with_capacity(txs.len());
    for (tx_index, tx) in txs.iter().enumerate() {
        let Some(tx_hash) = tx.get("id").and_then(Value::as_str) else { continue };
        cached_txs.push((tx_hash.to_string(), tx.clone()));
        parse_tx(&b, tx, tx_hash, tx_index, network, deleg, dapp, &mut events);
        dapp.note_spend_graph_hubs(tx_hash, tx);
        scan_txs.push((tx_hash, tx));
    }
    for (tx_hash, hit) in dex.scan_block(&scan_txs) {
        // DEX/dApp cards are user actions — never publish without who did it.
        if !data_has_actor(&hit.data) {
            continue;
        }
        events.push(crate::dapp::dex::hit_to_event(hit, slot, height, &hash, &tx_hash, timestamp));
    }
    for (tx_hash, hit) in dapp.scan_block(&scan_txs) {
        if !data_has_actor(&hit.data) {
            continue;
        }
        events.push(crate::dapp::hit_to_event(hit, slot, height, &hash, &tx_hash, timestamp));
    }

    Some(ParsedBlock { hash, height, slot, events, txs: cached_txs })
}

fn parse_tx(
    b: &EventBuilder,
    tx: &Value,
    tx_hash: &str,
    tx_index: usize,
    network: Network,
    deleg: &DelegationTracker,
    dapp: &DappRegistry,
    events: &mut Vec<ChainEvent>,
) {
    let empty = Vec::new();

    let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let fee = lovelace_fee(tx).unwrap_or(0);
    let out_ada = tx_output_lovelace(tx);
    let is_script = tx
        .get("redeemers")
        .map(|r| match r {
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
            _ => false,
        })
        .unwrap_or(false);

    // Collect native assets appearing in outputs: policy -> name -> qty
    let mut moved: Vec<(String, String, i128)> = Vec::new();
    for o in outputs {
        collect_assets(o.get("value"), &mut moved);
    }
    // Assets minted/burned in this tx
    let mut minted: Vec<(String, String, i128)> = Vec::new();
    collect_assets(tx.get("mint"), &mut minted);

    let input_txs = collect_input_txs(tx_hash, inputs, dapp, |_, _| None);

    let stakes = collect_tx_stakes(tx, network);

    // ── Transaction event (always) ───────────────────────────────────────
    let mut tx_data = json!({
        "index": tx_index,
        "inputs": inputs.len(),
        "inputTxs": input_txs,
        "outputs": outputs.len(),
        "ada": out_ada,
        "fee": fee,
        "script": is_script,
        "assets": moved.len(),
        "size": tx.get("size").and_then(|s| s.get("bytes")).and_then(Value::as_u64),
    });
    if !stakes.is_empty() {
        tx_data
            .as_object_mut()
            .unwrap()
            .insert("stakes".into(), json!(stakes));
    }
    events.push(b.make(
        "transaction",
        "transaction",
        Some(tx_hash),
        "Transaction".into(),
        String::new(),
        tx_data,
    ));

    // ── Mint / burn ──────────────────────────────────────────────────────
    let mints: Vec<_> = minted.iter().filter(|(_, _, q)| *q > 0).collect();
    let burns: Vec<_> = minted.iter().filter(|(_, _, q)| *q < 0).collect();
    if !mints.is_empty() {
        events.push(b.make(
            "mint",
            "mint",
            Some(tx_hash),
            if mints.len() == 1 { "Token Mint".into() } else { format!("Token Mint ×{}", mints.len()) },
            String::new(),
            json!({ "assets": asset_list(&mints) }),
        ));
    }
    if !burns.is_empty() {
        events.push(b.make(
            "burn",
            "mint",
            Some(tx_hash),
            if burns.len() == 1 { "Token Burn".into() } else { format!("Token Burn ×{}", burns.len()) },
            String::new(),
            json!({ "assets": asset_list(&burns) }),
        ));
    }

    // ── Token transfer: assets moved but not minted in this tx ──────────
    let transferred: Vec<_> = moved
        .iter()
        .filter(|(p, n, _)| !minted.iter().any(|(mp, mn, _)| mp == p && mn == n))
        .collect();
    if !transferred.is_empty() {
        events.push(b.make(
            "token_transfer",
            "token",
            Some(tx_hash),
            if transferred.len() == 1 {
                "Token Transfer".into()
            } else {
                format!("Token Transfer ×{}", transferred.len())
            },
            String::new(),
            json!({ "assets": asset_list(&transferred) }),
        ));
    }

    // ── Certificates ─────────────────────────────────────────────────────
    for cert in tx.get("certificates").and_then(Value::as_array).unwrap_or(&empty) {
        parse_certificate(b, cert, tx_hash, network, deleg, events);
    }

    // ── Withdrawals ──────────────────────────────────────────────────────
    // Skip 0-lovelace withdrawals: scripts often withdraw ₳0 from a script
    // stake address solely to force a reward-account script purpose. Those
    // aren't real reward claims and would spam the feed as "₳ 0".
    if let Some(w) = tx.get("withdrawals").and_then(Value::as_object) {
        for (account, amount) in w {
            let lov = amount
                .get("ada")
                .and_then(|a| a.get("lovelace"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if lov == 0 {
                continue;
            }
            events.push(b.make(
                "withdrawal",
                "staking",
                Some(tx_hash),
                "Reward Withdrawal".into(),
                String::new(),
                json!({ "account": account, "lovelace": lov }),
            ));
        }
    }

    // ── Governance proposals ─────────────────────────────────────────────
    for (i, p) in tx.get("proposals").and_then(Value::as_array).unwrap_or(&empty).iter().enumerate() {
        let action_type = p
            .get("action")
            .and_then(|a| a.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let deposit = p
            .get("deposit")
            .and_then(|d| d.get("ada"))
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        events.push(b.make(
            "gov_proposal",
            "governance",
            Some(tx_hash),
            format!("Governance Action: {}", gov_action_label(action_type)),
            String::new(),
            json!({
                "actionType": action_type,
                "index": i,
                "deposit": deposit,
                "anchorUrl": p.get("metadata").and_then(|m| m.get("url")).and_then(Value::as_str),
                "withdrawals": p.get("action").and_then(|a| a.get("withdrawals")),
            }),
        ));
    }

    // ── Governance votes ─────────────────────────────────────────────────
    for v in tx.get("votes").and_then(Value::as_array).unwrap_or(&empty) {
        let vote = v.get("vote").and_then(Value::as_str).unwrap_or("?");
        let role = v
            .get("issuer")
            .and_then(|i| i.get("role"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let voter = v.get("issuer").map(voter_display).unwrap_or_default();
        let proposal_tx = v
            .get("proposal")
            .and_then(|p| p.get("transaction"))
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str);
        let who = match role {
            "delegateRepresentative" => "DRep Vote",
            "stakePoolOperator" => "SPO Vote",
            "constitutionalCommittee" => "CC Vote",
            _ => "Vote",
        };
        events.push(b.make(
            "gov_vote",
            "governance",
            Some(tx_hash),
            format!("{who}: {}", vote.to_uppercase()),
            String::new(),
            json!({
                "vote": vote,
                "role": role,
                "voter": voter,
                "proposalTx": proposal_tx,
                "proposalIndex": v.get("proposal").and_then(|p| p.get("index")),
                "anchorUrl": v.get("metadata").and_then(|m| m.get("url")).and_then(Value::as_str),
            }),
        ));
    }

    // ── Metadata (incl. CIP-20 messages) ─────────────────────────────────
    if let Some(labels) = tx
        .get("metadata")
        .and_then(|m| m.get("labels"))
        .and_then(Value::as_object)
        .filter(|l| !l.is_empty())
    {
        let label_keys: Vec<&String> = labels.keys().collect();
        // CIP-20: label 674 { "msg": [ ...lines ] }
        let msg = labels
            .get("674")
            .and_then(|l| l.get("json"))
            .and_then(|j| j.get("msg"))
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        events.push(b.make(
            "tx_metadata",
            "metadata",
            Some(tx_hash),
            if msg.is_some() { "Message".into() } else { "Metadata".into() },
            String::new(),
            json!({ "labels": label_keys, "msg": msg }),
        ));
    }
}

fn parse_certificate(
    b: &EventBuilder,
    cert: &Value,
    tx_hash: &str,
    network: Network,
    deleg: &DelegationTracker,
    events: &mut Vec<ChainEvent>,
) {
    let ctype = cert.get("type").and_then(Value::as_str).unwrap_or("unknown");
    let stake_addr = cert
        .get("credential")
        .and_then(Value::as_str)
        .map(|c| stake_address(c, cert.get("from").and_then(Value::as_str), network));

    let ev = |kind: &'static str, category: &'static str, title: String, data: Value| {
        b.make(kind, category, Some(tx_hash), title, String::new(), data)
    };

    match ctype {
        "stakeDelegation" => {
            let pool = cert
                .get("stakePool")
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let drep = cert.get("delegateRepresentative").map(drep_display);
            // A single Conway cert can delegate stake to a pool and voting
            // power to a DRep at once; surface each as its own event.
            if let Some(pool) = &pool {
                let mut data = json!({ "stake": stake_addr, "pool": pool });
                if let Some(stake) = stake_addr.as_deref() {
                    if let Some(from) = deleg.swap_pool(stake, pool) {
                        data.as_object_mut().unwrap().insert("fromPool".into(), json!(from));
                    }
                }
                events.push(ev(
                    "delegation",
                    "staking",
                    "Stake Delegation".into(),
                    data,
                ));
            }
            if let Some(drep) = drep {
                let mut data = json!({ "stake": stake_addr, "drep": drep });
                if let Some(stake) = stake_addr.as_deref() {
                    if let Some(from) = deleg.swap_drep(stake, &drep) {
                        data.as_object_mut().unwrap().insert("fromDrep".into(), json!(from));
                    }
                }
                events.push(ev(
                    "vote_delegation",
                    "governance",
                    "DRep Delegation".into(),
                    data,
                ));
            }
            if pool.is_none() && cert.get("delegateRepresentative").is_none() {
                events.push(ev(
                    "delegation",
                    "staking",
                    "Stake Delegation".into(),
                    json!({ "stake": stake_addr }),
                ));
            }
        }
        "stakeCredentialRegistration" => events.push(ev(
            "stake_registration",
            "staking",
            "Stake Key Registered".into(),
            json!({ "stake": stake_addr }),
        )),
        "stakeCredentialDeregistration" => events.push(ev(
            "stake_deregistration",
            "staking",
            "Stake Key Deregistered".into(),
            json!({ "stake": stake_addr }),
        )),
        "stakePoolRegistration" => {
            let p = cert.get("stakePool").cloned().unwrap_or(Value::Null);
            events.push(ev(
                "pool_registration",
                "pool",
                "Pool Registration".into(),
                json!({
                    "pool": p.get("id").and_then(Value::as_str),
                    "pledge": p.get("pledge").and_then(|v| v.get("ada")).and_then(|a| a.get("lovelace")),
                    "cost": p.get("cost").and_then(|v| v.get("ada")).and_then(|a| a.get("lovelace")),
                    "margin": p.get("margin"),
                    "metadataUrl": p.get("metadata").and_then(|m| m.get("url")),
                }),
            ));
        }
        "stakePoolRetirement" => {
            let p = cert.get("stakePool").cloned().unwrap_or(Value::Null);
            events.push(ev(
                "pool_retirement",
                "pool",
                "Pool Retirement".into(),
                json!({
                    "pool": p.get("id").and_then(Value::as_str),
                    "retirementEpoch": p.get("retirementEpoch"),
                }),
            ));
        }
        "delegateRepresentativeRegistration" => events.push(ev(
            "drep_registration",
            "governance",
            "DRep registered".into(),
            json!({
                "drep": cert.get("delegateRepresentative").map(drep_display),
                "anchorUrl": cert.get("metadata").and_then(|m| m.get("url")),
            }),
        )),
        "delegateRepresentativeUpdate" => events.push(ev(
            "drep_update",
            "governance",
            "DRep updated".into(),
            json!({
                "drep": cert.get("delegateRepresentative").map(drep_display),
                "anchorUrl": cert.get("metadata").and_then(|m| m.get("url")),
            }),
        )),
        "delegateRepresentativeRetirement" => events.push(ev(
            "drep_retirement",
            "governance",
            "DRep retired".into(),
            json!({ "drep": cert.get("delegateRepresentative").map(drep_display) }),
        )),
        "constitutionalCommitteeDelegation" | "constitutionalCommitteeHotKeyRegistration" => {
            events.push(ev(
                "committee_auth",
                "governance",
                "Committee Hot Key Authorized".into(),
                json!({ "member": cert.get("member"), "delegate": cert.get("delegate") }),
            ))
        }
        "constitutionalCommitteeRetirement" => events.push(ev(
            "committee_resign",
            "governance",
            "Committee Member Resigned".into(),
            json!({ "member": cert.get("member") }),
        )),
        other => events.push(ev(
            "certificate",
            "staking",
            format!("Certificate: {other}"),
            json!({ "certType": other }),
        )),
    }
}

// ── Value helpers ─────────────────────────────────────────────────────────

fn lovelace_fee(tx: &Value) -> Option<u64> {
    tx.get("fee")?.get("ada")?.get("lovelace")?.as_u64()
}

fn tx_output_lovelace(tx: &Value) -> u64 {
    tx.get("outputs")
        .and_then(Value::as_array)
        .map(|outs| {
            outs.iter()
                .filter_map(|o| {
                    o.get("value")?.get("ada")?.get("lovelace")?.as_u64()
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Ogmios value shape: { "ada": {"lovelace": n}, "<policy>": {"<nameHex>": qty} }
pub fn collect_assets(value: Option<&Value>, into: &mut Vec<(String, String, i128)>) {
    let Some(obj) = value.and_then(Value::as_object) else { return };
    for (policy, assets) in obj {
        if policy == "ada" {
            continue;
        }
        let Some(assets) = assets.as_object() else { continue };
        for (name_hex, qty) in assets {
            let q = qty
                .as_i64()
                .map(i128::from)
                .or_else(|| qty.as_u64().map(i128::from))
                .or_else(|| qty.as_f64().map(|f| f as i128))
                .unwrap_or(0);
            match into.iter_mut().find(|(p, n, _)| p == policy && n == name_hex) {
                Some(entry) => entry.2 += q,
                None => into.push((policy.clone(), name_hex.clone(), q)),
            }
        }
    }
}

pub fn asset_list(assets: &[&(String, String, i128)]) -> Value {
    const MAX: usize = 12;
    let items: Vec<Value> = assets
        .iter()
        .take(MAX)
        .map(|(policy, name_hex, qty)| {
            json!({
                "unit": format!("{policy}{name_hex}"),
                "policy": policy,
                "nameHex": name_hex,
                "name": decode_asset_name(name_hex),
                "qty": qty.to_string(),
                "fingerprint": asset_fingerprint(policy, name_hex),
            })
        })
        .collect();
    json!({ "items": items, "more": assets.len().saturating_sub(MAX) })
}

/// True when `value` carries a positive qty of `policy`/`name_hex`.
pub fn value_holds_asset(value: Option<&Value>, policy: &str, name_hex: &str) -> bool {
    let Some(obj) = value.and_then(Value::as_object) else {
        return false;
    };
    let Some(assets) = obj.get(policy).and_then(Value::as_object) else {
        return false;
    };
    let Some(qty) = assets.get(name_hex) else {
        return false;
    };
    qty.as_i64()
        .map(|q| q > 0)
        .or_else(|| qty.as_u64().map(|q| q > 0))
        .or_else(|| qty.as_f64().map(|q| q > 0.0))
        .unwrap_or(false)
}

/// Payment credentials (28-byte hex) on outputs that currently hold `policy`/`name_hex`.
fn output_creds_holding_asset(tx: &Value, policy: &str, name_hex: &str) -> HashSet<String> {
    let empty = Vec::new();
    let mut out = HashSet::new();
    for o in tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty) {
        if !value_holds_asset(o.get("value"), policy, name_hex) {
            continue;
        }
        let Some(addr) = o.get("address").and_then(Value::as_str) else {
            continue;
        };
        if let Some(cred) = payment_credential(addr) {
            out.insert(cred);
        }
    }
    out
}

/// All key-payment credentials appearing on any tx output (excludes script payments).
fn output_key_payment_creds(tx: &Value) -> HashSet<String> {
    let empty = Vec::new();
    let mut out = HashSet::new();
    for o in tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty) {
        let Some(addr) = o.get("address").and_then(Value::as_str) else {
            continue;
        };
        if address_has_script_payment(addr) {
            continue;
        }
        if let Some(cred) = payment_credential(addr) {
            out.insert(cred);
        }
    }
    out
}

/// True when `policy`/`name_hex` changes payment-credential ownership in this tx
/// (key→key across different pkhs, or key→script). Same-pkh consolidations /
/// change reshuffles return false.
///
/// `resolve_spent(tx_hash, index)` should return `(address, value)` of the spent
/// output when known (from the tx cache). When no inputs resolve, falls back to
/// an output-only heuristic: single key-payment wallet txs are treated as
/// internal; multi-wallet txs flag as a hand-change.
pub fn asset_changes_hands(
    tx: &Value,
    policy: &str,
    name_hex: &str,
    mut resolve_spent: impl FnMut(&str, u64) -> Option<(String, Value)>,
) -> bool {
    let out_creds = output_creds_holding_asset(tx, policy, name_hex);
    if out_creds.is_empty() {
        return false;
    }

    let empty = Vec::new();
    let mut in_creds = HashSet::new();
    let mut resolved_any = false;
    for i in tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty) {
        let Some(src) = i
            .get("transaction")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .or_else(|| i.get("txId").and_then(Value::as_str))
        else {
            continue;
        };
        let Some(index) = i
            .get("index")
            .and_then(Value::as_u64)
            .or_else(|| i.get("outputIndex").and_then(Value::as_u64))
        else {
            continue;
        };
        let Some((addr, value)) = resolve_spent(src, index) else {
            continue;
        };
        resolved_any = true;
        if !value_holds_asset(Some(&value), policy, name_hex) {
            continue;
        }
        if let Some(cred) = payment_credential(&addr) {
            in_creds.insert(cred);
        }
    }

    if resolved_any {
        // New payment credential receives the asset → it changed hands.
        return out_creds.iter().any(|c| !in_creds.contains(c));
    }

    // No resolved inputs (typical for fresh Ogmios txs whose parents aren't
    // cached yet). Single key-payment wallet ⇒ treat as internal reshuffle.
    let key_creds = output_key_payment_creds(tx);
    if key_creds.len() <= 1 {
        // Asset only on that one key (or only on scripts with ≤1 key change).
        return out_creds.iter().any(|c| !key_creds.contains(c));
    }
    // Multiple key wallets in the tx: asset on any of them is a hand-change
    // unless every receiving cred is somehow empty (already handled).
    true
}

/// Decode an asset-name hex string to UTF-8 when printable, stripping a
/// CIP-67 label prefix (e.g. CIP-68 000de140…) when present.
pub fn decode_asset_name(name_hex: &str) -> Option<String> {
    let bytes = hex::decode(name_hex).ok()?;
    let bytes = if bytes.len() >= 4 && bytes[0] == 0x00 && (bytes[3] & 0x0f) == 0 {
        &bytes[4..] // CIP-67 asset-name label
    } else {
        &bytes[..]
    };
    let s = std::str::from_utf8(bytes).ok()?;
    let clean = !s.is_empty() && s.chars().all(|c| !c.is_control());
    clean.then(|| s.to_string())
}

/// CIP-14 asset fingerprint: bech32("asset", blake2b-160(policy ++ name))
pub fn asset_fingerprint(policy_hex: &str, name_hex: &str) -> Option<String> {
    let mut bytes = hex::decode(policy_hex).ok()?;
    bytes.extend(hex::decode(name_hex).ok()?);
    let digest = blake2b_simd::Params::new().hash_length(20).hash(&bytes);
    let hrp = Hrp::parse("asset").ok()?;
    bech32::encode::<Bech32>(hrp, digest.as_bytes()).ok()
}

/// Pool id (bech32 "pool1…") from the block issuer's cold verification key.
pub fn pool_id_from_vkey(vkey_hex: &str) -> Option<String> {
    let bytes = hex::decode(vkey_hex).ok()?;
    let digest = blake2b_simd::Params::new().hash_length(28).hash(&bytes);
    let hrp = Hrp::parse("pool").ok()?;
    bech32::encode::<Bech32>(hrp, digest.as_bytes()).ok()
}

/// Bech32 stake address from a credential key/script hash.
pub fn stake_address(cred_hex: &str, from: Option<&str>, network: Network) -> String {
    let Ok(hash) = hex::decode(cred_hex) else { return cred_hex.to_string() };
    if hash.len() != 28 {
        return cred_hex.to_string();
    }
    let header: u8 = if from == Some("script") {
        0xf0 | network.id_bit()
    } else {
        0xe0 | network.id_bit()
    };
    let mut bytes = vec![header];
    bytes.extend(hash);
    match Hrp::parse(network.stake_hrp())
        .ok()
        .and_then(|hrp| bech32::encode::<Bech32>(hrp, &bytes).ok())
    {
        Some(addr) => addr,
        None => cred_hex.to_string(),
    }
}

/// CIP-19: address types 1/3/5/7 have a script payment credential.
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

pub fn address_has_script_payment(addr: &str) -> bool {
    let Some(header) = address_header(addr) else {
        return false;
    };
    matches!(header >> 4, 1 | 3 | 5 | 7)
}

/// Derive the stake address embedded in a Shelley base payment address.
pub fn stake_from_address(addr: &str) -> Option<String> {
    let (hrp, bytes) = decode_address(addr)?;
    if !hrp.starts_with("addr") {
        return None;
    }
    let header = *bytes.first()?;
    let ty = header >> 4;
    let net = header & 0x0f;
    // Base addresses only (payment + stake credential).
    if !matches!(ty, 0 | 1 | 2 | 3) || bytes.len() < 57 {
        return None;
    }
    let stake_hash = &bytes[29..57];
    let stake_is_script = matches!(ty, 2 | 3);
    let stake_header = if stake_is_script {
        0xf0 | net
    } else {
        0xe0 | net
    };
    let mut stake_bytes = Vec::with_capacity(29);
    stake_bytes.push(stake_header);
    stake_bytes.extend_from_slice(stake_hash);
    let stake_hrp = if hrp.contains("test") {
        "stake_test"
    } else {
        "stake"
    };
    let hrp = Hrp::parse(stake_hrp).ok()?;
    bech32::encode::<Bech32>(hrp, &stake_bytes).ok()
}

/// Distinct source tx-hashes for light-cone edges. Skips shared dApp hub
/// outpoints (Iagon rewards batcher, etc.). `parent_addr` resolves a spent
/// outpoint to its output address when available (buffered rewrite path).
pub fn collect_input_txs(
    tx_hash: &str,
    inputs: &[Value],
    dapp: &DappRegistry,
    mut parent_addr: impl FnMut(&str, u64) -> Option<String>,
) -> Vec<String> {
    const MAX_INPUT_TXS: usize = 30;
    let mut input_txs: Vec<String> = Vec::new();
    for i in inputs {
        let Some(id) = i
            .get("transaction")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .or_else(|| i.get("txId").and_then(Value::as_str))
        else {
            continue;
        };
        let index = i
            .get("index")
            .and_then(Value::as_u64)
            .or_else(|| i.get("outputIndex").and_then(Value::as_u64));
        if let Some(index) = index {
            let op = format!("{id}#{index}");
            if dapp.is_spend_graph_hub(&op) {
                continue;
            }
            if let Some(addr) = parent_addr(id, index) {
                if dapp.is_spend_graph_hub_address(&addr) {
                    continue;
                }
            } else if let Some(addr) = i.get("address").and_then(Value::as_str) {
                if dapp.is_spend_graph_hub_address(addr) {
                    continue;
                }
            }
        }
        if id != tx_hash && !input_txs.iter().any(|s| s == id) {
            input_txs.push(id.to_string());
            if input_txs.len() >= MAX_INPUT_TXS {
                break;
            }
        }
    }
    input_txs
}

/// Best-effort **user** actor for a tx: largest key-payment output, preferring
/// an embedded stake address over the payment address. Skips script outs
/// (DEX pools, dApp contracts, order scripts) so we don't label the venue.
/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
pub fn actor_from_tx(tx: &Value) -> Option<String> {
    actor_from_outputs(tx.get("outputs").and_then(Value::as_array)?, None)
}

/// Prefer the key-payment output whose asset qty is closest to `target`
/// (e.g. IAG earnings equal to the claimed amount — not a large change UTxO).
/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
pub fn actor_receiving_asset(
    tx: &Value,
    policy: &str,
    name_hex: &str,
    target: u64,
) -> Option<String> {
    let outputs = tx.get("outputs").and_then(Value::as_array)?;
    let target = target as u128;
    let mut best: Option<(u128, &str)> = None; // distance, addr
    for output in outputs {
        let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
        if addr.is_empty() || address_has_script_payment(addr) {
            continue;
        }
        let qty = asset_qty(output.get("value"), policy, name_hex);
        if qty == 0 {
            continue;
        }
        let dist = qty.abs_diff(target);
        if best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, addr));
        }
    }
    best.map(|(_, addr)| prefer_stake(addr))
}

/// Prefer the key-payment output receiving the most ADA (e.g. position seller).
/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
pub fn actor_receiving_ada(tx: &Value, min_lovelace: u64) -> Option<String> {
    let outputs = tx.get("outputs").and_then(Value::as_array)?;
    actor_from_outputs(outputs, Some(min_lovelace))
}

/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
fn actor_from_outputs<'a>(
    outputs: &'a [Value],
    min_lovelace: Option<u64>,
) -> Option<String> {
    let mut best: Option<(u64, u8, &'a str)> = None; // ada, has_stake, addr
    for output in outputs {
        let addr = output.get("address").and_then(Value::as_str).unwrap_or("");
        if addr.is_empty() || address_has_script_payment(addr) {
            continue;
        }
        if !(addr.starts_with("addr1") || addr.starts_with("addr_test1")) {
            continue;
        }
        let ada = output
            .get("value")
            .and_then(|v| v.get("ada"))
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if let Some(min) = min_lovelace {
            if ada < min {
                continue;
            }
        }
        let has_stake = u8::from(stake_from_address(addr).is_some());
        let better = match best {
            None => true,
            Some((b_ada, b_stake, _)) => ada > b_ada || (ada == b_ada && has_stake > b_stake),
        };
        if better {
            best = Some((ada, has_stake, addr));
        }
    }
    best.map(|(_, _, addr)| prefer_stake(addr))
}

/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
fn prefer_stake(addr: &str) -> String {
    stake_from_address(addr).unwrap_or_else(|| addr.to_string())
}

/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
fn asset_qty(value: Option<&Value>, policy: &str, name_hex: &str) -> u128 {
    value
        .and_then(|v| v.get(policy))
        .and_then(|n| n.get(name_hex))
        .and_then(|q| {
            q.as_u64()
                .map(|n| n as u128)
                .or_else(|| q.as_i64().map(|n| n.unsigned_abs() as u128))
                .or_else(|| q.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0)
}

/// Attach actor onto event data: `stake` when we have a stake address, else `address`.
/// Only reachable from the optional dApp/DEX pack (`src/dapp/`).
#[cfg_attr(not(has_dapp), allow(dead_code))]
pub fn attach_actor(data: &mut serde_json::Map<String, Value>, actor: Option<&str>) {
    let Some(a) = actor.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if a.starts_with("stake1") || a.starts_with("stake_test1") {
        data.insert("stake".into(), json!(a));
    } else {
        data.insert("address".into(), json!(a));
    }
}

/// True when event `data` names who performed the action (stake preferred).
pub fn data_has_actor(data: &Value) -> bool {
    let Some(obj) = data.as_object() else {
        return false;
    };
    for key in ["stake", "address", "handle"] {
        if obj
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
        {
            return true;
        }
    }
    false
}

const MAX_TX_STAKES: usize = 24;

/// Unique stake addresses involved in a tx (order preserved): outputs, any
/// resolved input addresses, withdrawals, and certificate credentials.
/// Ogmios usually omits input addresses, so the list is output-heavy by design.
fn collect_tx_stakes(tx: &Value, network: Network) -> Vec<String> {
    let empty = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if out.len() >= MAX_TX_STAKES {
            return;
        }
        if (s.starts_with("stake1") || s.starts_with("stake_test1")) && !out.iter().any(|x| x == &s)
        {
            out.push(s);
        }
    };

    for side in ["inputs", "outputs", "collaterals"] {
        for o in tx.get(side).and_then(Value::as_array).unwrap_or(&empty) {
            let Some(addr) = o.get("address").and_then(Value::as_str) else {
                continue;
            };
            if let Some(stake) = stake_from_address(addr) {
                push(stake);
            } else if addr.starts_with("stake1") || addr.starts_with("stake_test1") {
                push(addr.to_string());
            }
        }
    }

    if let Some(w) = tx.get("withdrawals").and_then(Value::as_object) {
        for account in w.keys() {
            push(account.clone());
        }
    }

    for cert in tx.get("certificates").and_then(Value::as_array).unwrap_or(&empty) {
        if let Some(cred) = cert.get("credential").and_then(Value::as_str) {
            push(stake_address(
                cred,
                cert.get("from").and_then(Value::as_str),
                network,
            ));
        }
    }

    out
}

fn address_header(addr: &str) -> Option<u8> {
    decode_address(addr).and_then(|(_, b)| b.first().copied())
}

fn decode_address(addr: &str) -> Option<(String, Vec<u8>)> {
    if !(addr.starts_with("addr1")
        || addr.starts_with("addr_test1")
        || addr.starts_with("stake1")
        || addr.starts_with("stake_test1"))
    {
        return None;
    }
    let checked = CheckedHrpstring::new::<Bech32>(addr).ok()?;
    let hrp = checked.hrp().to_string();
    let bytes: Vec<u8> = checked.byte_iter().collect();
    Some((hrp, bytes))
}

/// Human-friendly DRep identifier (CIP-129 bech32 `drep1…`, or the special
/// always-abstain / no-confidence dreps).
///
/// Encodes the 28-byte credential with the CIP-129 header byte so the id matches
/// modern explorers. Legacy CIP-105 forms still resolve via
/// [`crate::dreps::drep_id_aliases`].
pub fn drep_display(drep: &Value) -> String {
    match drep.get("type").and_then(Value::as_str) {
        Some("abstain") | Some("alwaysAbstain") => "Always Abstain".into(),
        Some("noConfidence") | Some("alwaysNoConfidence") => "Always No Confidence".into(),
        _ => {
            let id = drep.get("id").and_then(Value::as_str).unwrap_or("");
            let from = drep.get("from").and_then(Value::as_str);
            let encoded = crate::dreps::drep_bech32_cip129(id, from).unwrap_or_else(|| id.to_string());
            crate::dreps::normalize_drep_id(&encoded)
        }
    }
}

fn voter_display(issuer: &Value) -> String {
    match issuer.get("role").and_then(Value::as_str) {
        Some("delegateRepresentative") => drep_display(issuer),
        _ => issuer
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

pub fn gov_action_label(action_type: &str) -> &'static str {
    match action_type {
        "treasuryWithdrawals" => "Treasury Withdrawal",
        "protocolParametersUpdate" => "Protocol Parameter Update",
        "hardForkInitiation" => "Hard Fork Initiation",
        "constitutionalCommittee" => "Committee Update",
        "constitution" => "New Constitution",
        "noConfidence" => "No Confidence",
        "information" => "Info Action",
        _ => "Governance Action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dapp::DappRegistry;
    use crate::deleg::DelegationTracker;
    use crate::dapp::dex::DexRegistry;

    fn encode_base_addr(payment_hex: &str, stake_hex: &str) -> String {
        let payment = hex::decode(payment_hex).unwrap();
        let stake = hex::decode(stake_hex).unwrap();
        assert_eq!(payment.len(), 28);
        assert_eq!(stake.len(), 28);
        // CIP-19 type 0 (key+key), mainnet network bit 1 → header 0x01.
        let mut bytes = vec![0x01];
        bytes.extend(payment);
        bytes.extend(stake);
        let hrp = Hrp::parse("addr").unwrap();
        bech32::encode::<Bech32>(hrp, &bytes).unwrap()
    }

    #[test]
    fn stake_from_base_payment_address() {
        let payment = "81c784f7113c761123af5442f282b4ef43a325f3537cf0b9c3542eec";
        let stake_hash = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let addr = encode_base_addr(payment, stake_hash);
        let expected = stake_address(stake_hash, Some("verificationKey"), Network::Mainnet);
        assert_eq!(stake_from_address(&addr).as_deref(), Some(expected.as_str()));
        assert!(!address_has_script_payment(&addr));
    }

    #[test]
    fn collect_tx_stakes_from_outputs_and_withdrawals() {
        let payment = "81c784f7113c761123af5442f282b4ef43a325f3537cf0b9c3542eec";
        let stake_hash = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let user = encode_base_addr(payment, stake_hash);
        let stake = stake_address(stake_hash, Some("verificationKey"), Network::Mainnet);
        let other = stake_address(
            "11111111111111111111111111111111111111111111111111111111",
            Some("verificationKey"),
            Network::Mainnet,
        );
        let tx = json!({
            "outputs": [
                { "address": user, "value": { "ada": { "lovelace": 2_000_000 } } },
                { "address": user, "value": { "ada": { "lovelace": 1_000_000 } } },
            ],
            "withdrawals": {
                (other.clone()): { "ada": { "lovelace": 5_000_000 } }
            },
            "certificates": [{
                "type": "stakeDelegation",
                "credential": stake_hash,
                "from": "verificationKey",
                "stakePool": { "id": "pool1demo" }
            }]
        });
        let stakes = collect_tx_stakes(&tx, Network::Mainnet);
        assert_eq!(stakes, vec![stake.clone(), other]);
        // Same stake from output + cert is deduped; order is first-seen.
        assert_eq!(stakes[0], stake);
    }

    #[test]
    fn data_has_actor_requires_nonempty_field() {
        assert!(!data_has_actor(&json!({})));
        assert!(!data_has_actor(&json!({ "stake": "" })));
        assert!(data_has_actor(&json!({ "stake": "stake1u…" })));
        assert!(data_has_actor(&json!({ "address": "addr1v…" })));
        assert!(data_has_actor(&json!({ "handle": "ada" })));
    }

    #[test]
    fn actor_from_tx_prefers_user_change_not_script() {
        let payment = "81c784f7113c761123af5442f282b4ef43a325f3537cf0b9c3542eec";
        let stake_hash = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let user = encode_base_addr(payment, stake_hash);
        // Minswap V1 order script (script payment) — must not be chosen.
        let order = "addr1wyx22z2s4kasd3w976pnjf9xdty88epjqfvgkmfnscpd0rg3z8y6v";
        let tx = json!({
            "outputs": [
                {
                    "address": order,
                    "value": { "ada": { "lovelace": 73_000_000u64 } }
                },
                {
                    "address": user,
                    "value": { "ada": { "lovelace": 12_500_000u64 } }
                }
            ]
        });
        let expected = stake_address(stake_hash, Some("verificationKey"), Network::Mainnet);
        assert_eq!(actor_from_tx(&tx).as_deref(), Some(expected.as_str()));
    }

    /// A tx spending two distinct earlier outputs (one of them twice) exposes
    /// the deduped set of source tx-hashes on its `transaction` event so the
    /// client can build the light-cone spend graph.
    #[test]
    fn transaction_event_carries_deduped_input_txs() {
        let block = json!({
            "id": "block0",
            "slot": 100,
            "height": 42,
            "transactions": [{
                "id": "txB",
                "inputs": [
                    { "transaction": { "id": "txA" }, "index": 0 },
                    { "transaction": { "id": "txA" }, "index": 1 },
                    { "transaction": { "id": "txC" }, "index": 0 },
                ],
                "outputs": [{ "value": { "ada": { "lovelace": 1_000_000 } } }],
                "fee": { "ada": { "lovelace": 170_000 } },
            }],
        });
        let parsed = parse_block(
            &block,
            1_700_000_000,
            Network::Mainnet,
            &DexRegistry::new(),
            &DappRegistry::new(),
            &DelegationTracker::new(),
        )
        .expect("block parses");

        let tx = parsed
            .events
            .iter()
            .find(|e| e.kind == "transaction")
            .expect("transaction event present");
        let input_txs: Vec<&str> = tx.data["inputTxs"]
            .as_array()
            .expect("inputTxs is an array")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // Deduped, order-preserving: txA once, then txC.
        assert_eq!(input_txs, vec!["txA", "txC"]);
        // Block event is emitted first so the publish loop can parent txs to it.
        assert_eq!(parsed.events.first().map(|e| e.kind.as_str()), Some("block"));
    }

    /// Unrelated Iagon claimers only share the rewards batcher UTxO — that hub
    /// edge must not appear in inputTxs. Same-user repeats still link via change.
    #[test]
    fn iagon_batcher_hub_omitted_from_input_txs_user_change_kept() {
        const BATCHER: &str = "addr1v8ckrqqrj4u34sxt45vdu8s8nqq3lm3lc8s7su5nyzaq9tcqy2n8j";
        let user_a = "addr1qaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let user_b = "addr1qbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

        let claim_a = json!({
            "id": "claimA",
            "inputs": [{ "transaction": { "id": "seed" }, "index": 0 }],
            "outputs": [
                {
                    "address": BATCHER,
                    "value": {
                        "ada": { "lovelace": 3_963_225_276u64 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 100_000u64 }
                    }
                },
                {
                    "address": user_a,
                    "value": { "ada": { "lovelace": 50_000_000u64 } }
                }
            ],
            "fee": { "ada": { "lovelace": 170_000 } },
        });

        fn input_txs(parsed: &ParsedBlock, hash: &str) -> Vec<String> {
            parsed
                .events
                .iter()
                .find(|e| e.kind == "transaction" && e.tx_hash.as_deref() == Some(hash))
                .expect("tx event")
                .data["inputTxs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        }

        // Path 1: different user spends only the batcher from claimA.
        let dapp = DappRegistry::new();
        let block_a = json!({
            "id": "blockA", "slot": 100, "height": 42,
            "transactions": [claim_a.clone()],
        });
        parse_block(
            &block_a,
            1_700_000_000,
            Network::Mainnet,
            &DexRegistry::new(),
            &dapp,
            &DelegationTracker::new(),
        )
        .unwrap();

        let claim_b = json!({
            "id": "claimB",
            "inputs": [
                { "transaction": { "id": "claimA" }, "index": 0 },
                { "transaction": { "id": "userBwallet" }, "index": 0 },
            ],
            "outputs": [
                {
                    "address": BATCHER,
                    "value": {
                        "ada": { "lovelace": 3_963_225_276u64 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 90_000u64 }
                    }
                },
                {
                    "address": user_b,
                    "value": {
                        "ada": { "lovelace": 2_000_000 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 10_000u64 }
                    }
                }
            ],
            "fee": { "ada": { "lovelace": 170_000 } },
        });
        let parsed_b = parse_block(
            &json!({
                "id": "blockB", "slot": 101, "height": 43,
                "transactions": [claim_b],
            }),
            1_700_000_001,
            Network::Mainnet,
            &DexRegistry::new(),
            &dapp,
            &DelegationTracker::new(),
        )
        .unwrap();
        assert_eq!(input_txs(&parsed_b, "claimB"), vec!["userBwallet".to_string()]);

        // Path 2: same user spends batcher + own change — change keeps the edge.
        let dapp2 = DappRegistry::new();
        parse_block(
            &block_a,
            1_700_000_000,
            Network::Mainnet,
            &DexRegistry::new(),
            &dapp2,
            &DelegationTracker::new(),
        )
        .unwrap();
        let claim_a2 = json!({
            "id": "claimA2",
            "inputs": [
                { "transaction": { "id": "claimA" }, "index": 0 },
                { "transaction": { "id": "claimA" }, "index": 1 },
            ],
            "outputs": [
                {
                    "address": BATCHER,
                    "value": {
                        "ada": { "lovelace": 3_963_225_276u64 },
                        "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114": { "494147": 80_000u64 }
                    }
                },
                {
                    "address": user_a,
                    "value": { "ada": { "lovelace": 48_000_000u64 } }
                }
            ],
            "fee": { "ada": { "lovelace": 170_000 } },
        });
        let parsed_a2 = parse_block(
            &json!({
                "id": "blockA2", "slot": 102, "height": 44,
                "transactions": [claim_a2],
            }),
            1_700_000_002,
            Network::Mainnet,
            &DexRegistry::new(),
            &dapp2,
            &DelegationTracker::new(),
        )
        .unwrap();
        assert_eq!(input_txs(&parsed_a2, "claimA2"), vec!["claimA".to_string()]);
    }

    #[test]
    fn asset_changes_hands_same_pkh_consolidation_is_internal() {
        let stake = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let alice = encode_base_addr(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            stake,
        );
        let policy = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let name = "5343414d"; // SCAM
        let parent = json!({
            "outputs": [{
                "address": alice,
                "value": {
                    "ada": { "lovelace": 5_000_000u64 },
                    (policy): { (name): 1 }
                }
            }]
        });
        let tx = json!({
            "inputs": [{ "transaction": { "id": "parent" }, "index": 0 }],
            "outputs": [
                {
                    "address": alice,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        (policy): { (name): 1 }
                    }
                },
                { "address": alice, "value": { "ada": { "lovelace": 2_000_000u64 } } },
            ]
        });
        let parents = std::collections::HashMap::from([("parent".to_string(), parent)]);
        let changes = asset_changes_hands(&tx, policy, name, |src, idx| {
            let p = parents.get(src)?;
            let o = p.get("outputs")?.as_array()?.get(idx as usize)?;
            Some((
                o.get("address")?.as_str()?.to_string(),
                o.get("value")?.clone(),
            ))
        });
        assert!(!changes, "same pkh reshape must not count as a hand-change");
    }

    #[test]
    fn asset_changes_hands_alice_to_bob_is_external() {
        let stake = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let alice = encode_base_addr(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            stake,
        );
        let bob = encode_base_addr(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            stake,
        );
        let policy = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let name = "5343414d";
        let parent = json!({
            "outputs": [{
                "address": alice,
                "value": {
                    "ada": { "lovelace": 5_000_000u64 },
                    (policy): { (name): 1 }
                }
            }]
        });
        let tx = json!({
            "inputs": [{ "transaction": { "id": "parent" }, "index": 0 }],
            "outputs": [
                {
                    "address": bob,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        (policy): { (name): 1 }
                    }
                },
                { "address": alice, "value": { "ada": { "lovelace": 2_000_000u64 } } },
            ]
        });
        let parents = std::collections::HashMap::from([("parent".to_string(), parent)]);
        let changes = asset_changes_hands(&tx, policy, name, |src, idx| {
            let p = parents.get(src)?;
            let o = p.get("outputs")?.as_array()?.get(idx as usize)?;
            Some((
                o.get("address")?.as_str()?.to_string(),
                o.get("value")?.clone(),
            ))
        });
        assert!(changes, "alice→bob must count as a hand-change");
    }

    #[test]
    fn asset_changes_hands_fallback_single_wallet_without_inputs() {
        let stake = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let alice = encode_base_addr(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            stake,
        );
        let policy = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let name = "5343414d";
        let tx = json!({
            "inputs": [{ "transaction": { "id": "missing" }, "index": 0 }],
            "outputs": [{
                "address": alice,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    (policy): { (name): 1 }
                }
            }]
        });
        let changes = asset_changes_hands(&tx, policy, name, |_, _| None);
        assert!(!changes, "unresolved inputs + single key wallet ⇒ internal");
    }

    #[test]
    fn asset_changes_hands_fallback_two_wallets_without_inputs() {
        let stake = "87098a3cfda9c3a1dec5657ce7bd4cf0757f0474d2cdf4032db71360";
        let alice = encode_base_addr(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            stake,
        );
        let bob = encode_base_addr(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            stake,
        );
        let policy = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let name = "5343414d";
        let tx = json!({
            "inputs": [{ "transaction": { "id": "missing" }, "index": 0 }],
            "outputs": [
                {
                    "address": bob,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        (policy): { (name): 1 }
                    }
                },
                { "address": alice, "value": { "ada": { "lovelace": 2_000_000u64 } } },
            ]
        });
        let changes = asset_changes_hands(&tx, policy, name, |_, _| None);
        assert!(changes, "unresolved inputs + two key wallets ⇒ hand-change");
    }
}
