//! Turns Ogmios v6 chain-sync blocks (JSON) into a stream of `ChainEvent`s.
//!
//! Parsing is deliberately `serde_json::Value`-based and defensive: the Ogmios
//! schema is large and era-dependent, and we'd rather drop a field than crash
//! the sync loop on an unexpected shape.

use crate::config::Network;
use crate::deleg::DelegationTracker;
use crate::dex::DexRegistry;
use crate::model::ChainEvent;
use bech32::{Bech32, Hrp};
use serde_json::{json, Value};

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
    // DEX: two-pass over the whole block so place+fill (any tx order) → one Swap.
    let mut dex_txs: Vec<(&str, &Value)> = Vec::with_capacity(txs.len());
    for (tx_index, tx) in txs.iter().enumerate() {
        let Some(tx_hash) = tx.get("id").and_then(Value::as_str) else { continue };
        cached_txs.push((tx_hash.to_string(), tx.clone()));
        parse_tx(&b, tx, tx_hash, tx_index, network, deleg, &mut events);
        dex_txs.push((tx_hash, tx));
    }
    for (tx_hash, hit) in dex.scan_block(&dex_txs) {
        events.push(crate::dex::hit_to_event(hit, slot, height, &hash, &tx_hash, timestamp));
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

    // ── Transaction event (always) ───────────────────────────────────────
    events.push(b.make(
        "transaction",
        "transaction",
        Some(tx_hash),
        "Transaction".into(),
        String::new(),
        json!({
            "index": tx_index,
            "inputs": inputs.len(),
            "outputs": outputs.len(),
            "ada": out_ada,
            "fee": fee,
            "script": is_script,
            "assets": moved.len(),
            "size": tx.get("size").and_then(|s| s.get("bytes")).and_then(Value::as_u64),
        }),
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
    if let Some(w) = tx.get("withdrawals").and_then(Value::as_object) {
        for (account, amount) in w {
            let lov = amount
                .get("ada")
                .and_then(|a| a.get("lovelace"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
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
        events.push(b.make(
            "gov_vote",
            "governance",
            Some(tx_hash),
            format!("Vote: {}", vote.to_uppercase()),
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
                    "Vote Delegation".into(),
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

/// Human-friendly DRep identifier (bech32 drep1… per CIP-105, or the special
/// always-abstain / no-confidence dreps).
pub fn drep_display(drep: &Value) -> String {
    match drep.get("type").and_then(Value::as_str) {
        Some("abstain") | Some("alwaysAbstain") => "Always Abstain".into(),
        Some("noConfidence") | Some("alwaysNoConfidence") => "Always No Confidence".into(),
        _ => {
            let id = drep.get("id").and_then(Value::as_str).unwrap_or("");
            match hex::decode(id).ok().filter(|b| b.len() == 28).and_then(|bytes| {
                let hrp = Hrp::parse("drep").ok()?;
                bech32::encode::<Bech32>(hrp, &bytes).ok()
            }) {
                Some(b32) => b32,
                None => id.to_string(),
            }
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
