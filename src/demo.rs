//! Demo mode: generates a realistic-looking stream of synthetic chain events
//! so the UI can be explored without a running node. Enabled with DEMO=true.

use crate::model::{BlockRef, ChainEvent, Tip};
use crate::state::AppState;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub async fn run(state: Arc<AppState>) {
    tracing::info!("DEMO mode: generating synthetic events");
    state.set_status("demo");
    let mut rng = StdRng::from_entropy();
    let mut height: u64 = 12_083_411;
    let mut slot: u64 = 151_243_622;
    let mut last_block: Option<BlockRef> = None;
    let mut blocks_since_fork = 0u32;

    loop {
        slot += rng.gen_range(4..30) as u64;
        height += 1;
        blocks_since_fork += 1;

        // Occasionally simulate a fork: orphan the previous block, then have
        // the replacement fight a slot battle.
        if blocks_since_fork > rng.gen_range(12..30) {
            blocks_since_fork = 0;
            if let Some(prev) = last_block.clone() {
                let orphaned = state.rollback_to(prev.slot.saturating_sub(1));
                emit_rollback(&state, &orphaned, prev.slot.saturating_sub(1));
                height = prev.height;
                slot = prev.slot;
            }
        }

        let hash = rand_hex(&mut rng, 64);
        let now = unix_now();
        let b = BlockRef { hash: hash.clone(), slot, height };
        let battle = state.note_block(b);
        state.counters.blocks.fetch_add(1, Ordering::Relaxed);

        let tx_count = rng.gen_range(0..14usize);
        state.publish(block_event(&hash, height, slot, now, tx_count, &mut rng));

        if let Some((loser, kind)) = battle {
            state.publish(ChainEvent {
                id: 0,
                parent_id: None,
                kind: "slot_battle".into(),
                category: "alert".into(),
                slot,
                height: Some(height),
                block_hash: Some(hash.clone()),
                tx_hash: None,
                timestamp: now,
                title: if kind == "slot" { "Slot Battle".into() } else { "Height Battle".into() },
                summary: String::new(),
                data: json!({ "battle": kind, "winner": hash, "loser": loser.hash, "slot": slot }),
            });
        }

        for i in 0..tx_count {
            tokio::time::sleep(Duration::from_millis(rng.gen_range(60..420))).await;
            emit_tx(&state, &hash, height, slot, i, &mut rng);
        }

        let (epoch, progress) = (slot / 432_000 + 208, (slot % 432_000) as f64 / 432_000.0);
        state.set_tip(Tip {
            height,
            slot,
            hash: hash.clone(),
            epoch,
            epoch_progress: progress,
            timestamp: now,
        });
        last_block = Some(BlockRef { hash, slot, height });

        tokio::time::sleep(Duration::from_millis(rng.gen_range(3500..9000))).await;
    }
}

fn emit_tx(state: &Arc<AppState>, block_hash: &str, height: u64, slot: u64, index: usize, rng: &mut StdRng) {
    let tx_hash = rand_hex(rng, 64);
    let now = unix_now();
    state.counters.txs.fetch_add(1, Ordering::Relaxed);
    let ada = 10f64.powf(rng.gen_range(0.0..6.5)) as u64 * 1_000_000;
    let fee = rng.gen_range(165_000..900_000u64);
    let script = rng.gen_bool(0.35);
    let n_in = rng.gen_range(1..6usize);
    let n_out = rng.gen_range(1..8usize);

    let mk = |kind: &'static str, category: &'static str, title: String, data: Value| ChainEvent {
        id: 0,
        parent_id: None,
        kind: kind.into(),
        category: category.into(),
        slot,
        height: Some(height),
        block_hash: Some(block_hash.to_string()),
        tx_hash: Some(tx_hash.clone()),
        timestamp: now,
        title,
        summary: String::new(),
        data,
    };

    state.publish(mk(
        "transaction",
        "transaction",
        "Transaction".into(),
        json!({ "index": index, "inputs": n_in, "outputs": n_out, "ada": ada, "fee": fee, "script": script, "assets": 0 }),
    ));

    let demo_tokens: [(&str, &str, &str); 6] = [
        ("f66d78b4a3cb3d37afa0ec36461e51ecbde00f26c8f0a68f94b69880", "69555344", "iUSD"),
        ("29d222ce763455e3d7a09a665ce554f00ac89d2e99a1a83d267170c6", "4d494e", "MIN"),
        ("8fef2d34078659493ce161a6c7fba4b56afefa8535296a5743f69587", "41414441", "AADA"),
        ("1d7f33bd23d85e1a25d87d86fac4f199c3197a2f7afeb662a0f34e1e", "776f726c646d6f62696c65746f6b656e", "WMT"),
        ("a0028f350aaabe0545fdcb56b039bfb08e4bb4d8c4d7c3c7d481c235", "484f534b59", "HOSKY"),
        ("533bb94a8850ee3ccbe483106489399112b74c905342cb1792a797a0", "494e4459", "INDY"),
    ];

    if rng.gen_bool(0.45) {
        let k = rng.gen_range(1..4usize);
        let items: Vec<Value> = (0..k)
            .map(|_| {
                let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
                json!({
                    "unit": format!("{p}{n}"),
                    "policy": p,
                    "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": rng.gen_range(1..5_000_000i64).to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                })
            })
            .collect();
        state.publish(mk(
            "token_transfer",
            "token",
            if items.len() == 1 { "Token Transfer".into() } else { format!("Token Transfer ×{}", items.len()) },
            json!({ "assets": { "items": items, "more": 0 } }),
        ));
    }

    if rng.gen_bool(0.10) {
        let name_hex = hex::encode(format!("DemoNFT{}", rng.gen_range(1..9999)));
        let policy = rand_hex(rng, 56);
        let burn = rng.gen_bool(0.25);
        state.publish(mk(
            if burn { "burn" } else { "mint" },
            "mint",
            if burn { "Token Burn".into() } else { "Token Mint".into() },
            json!({ "assets": { "items": [{
                "unit": format!("{policy}{name_hex}"),
                "policy": policy,
                "nameHex": name_hex,
                "name": crate::parse::decode_asset_name(&name_hex),
                "qty": if burn { "-1" } else { "1" },
                "fingerprint": crate::parse::asset_fingerprint(&policy, &name_hex),
            }], "more": 0 } }),
        ));
    }

    if rng.gen_bool(0.12) {
        let from = fake_pool(rng);
        let mut to = fake_pool(rng);
        while to == from { to = fake_pool(rng); }
        state.publish(mk(
            "delegation",
            "staking",
            "Stake Delegation".into(),
            json!({ "stake": fake_stake(rng), "fromPool": from, "pool": to }),
        ));
    }
    if rng.gen_bool(0.05) {
        state.publish(mk(
            "stake_registration",
            "staking",
            "Stake Key Registered".into(),
            json!({ "stake": fake_stake(rng) }),
        ));
    }
    if rng.gen_bool(0.08) {
        state.publish(mk(
            "withdrawal",
            "staking",
            "Reward Withdrawal".into(),
            json!({ "account": fake_stake(rng), "lovelace": rng.gen_range(1_000_000..2_000_000_000u64) }),
        ));
    }
    if rng.gen_bool(0.03) {
        state.publish(mk(
            "pool_registration",
            "pool",
            "Pool Registration".into(),
            json!({ "pool": fake_pool(rng), "pledge": 25_000_000_000u64, "cost": 340_000_000u64, "margin": "1/50" }),
        ));
    }
    if rng.gen_bool(0.05) {
        let votes = ["yes", "no", "abstain"];
        let vote = votes[rng.gen_range(0..3)];
        state.publish(mk(
            "gov_vote",
            "governance",
            format!("Vote: {}", vote.to_uppercase()),
            json!({
                "vote": vote,
                "role": "delegateRepresentative",
                "voter": fake_drep(rng),
                "proposalTx": rand_hex(rng, 64),
                "proposalIndex": 0,
            }),
        ));
    }
    if rng.gen_bool(0.02) {
        state.publish(mk(
            "gov_proposal",
            "governance",
            "Governance Action: Treasury withdrawal".into(),
            json!({ "actionType": "treasuryWithdrawals", "index": 0, "deposit": 100_000_000_000u64, "anchorUrl": "ipfs://demo" }),
        ));
    }
    if rng.gen_bool(0.04) {
        let from = fake_drep(rng);
        let mut to = fake_drep(rng);
        while to == from { to = fake_drep(rng); }
        state.publish(mk(
            "vote_delegation",
            "governance",
            "DRep Delegation".into(),
            json!({ "stake": fake_stake(rng), "fromDrep": from, "drep": to }),
        ));
    }
    if rng.gen_bool(0.07) {
        state.publish(mk(
            "tx_metadata",
            "metadata",
            "Message".into(),
            json!({ "labels": ["674"], "msg": "gm Cardano - sent from cardano-observer demo" }),
        ));
    }

    // ── DEX activity ────────────────────────────────────────────────────
    let dexes = ["Minswap", "SundaeSwap", "WingRiders", "MuesliSwap", "Splash", "VyFinance"];
    if rng.gen_bool(0.22) {
        let dex = dexes[rng.gen_range(0..dexes.len())];
        let side = ["buy", "sell", "swap"][rng.gen_range(0..3)];
        let order_ada = rng.gen_range(5..25_000u64) * 1_000_000;
        let assets = if side == "buy" {
            json!({ "items": [], "more": 0 })
        } else {
            let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
            json!({ "items": [{
                "unit": format!("{p}{n}"),
                "policy": p,
                "nameHex": n,
                "name": crate::parse::decode_asset_name(n),
                "qty": rng.gen_range(10..80_000_000i64).to_string(),
                "fingerprint": crate::parse::asset_fingerprint(p, n),
            }], "more": 0 })
        };
        let mut order_data = json!({ "dex": dex, "side": side, "ada": order_ada, "assets": assets });
        if side == "buy" && rng.gen_bool(0.85) {
            let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
            let qty = rng.gen_range(100..5_000_000i64);
            let obj = order_data.as_object_mut().unwrap();
            obj.insert("wantMin".into(), json!(true));
            obj.insert("wantQty".into(), json!(qty.to_string()));
            obj.insert(
                "want".into(),
                json!({ "items": [{
                    "unit": format!("{p}{n}"),
                    "policy": p,
                    "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                }], "more": 0 }),
            );
        } else if side == "sell" && rng.gen_bool(0.85) {
            let obj = order_data.as_object_mut().unwrap();
            let want_ada = rng.gen_range(5..20_000u64) * 1_000_000;
            obj.insert("wantMin".into(), json!(true));
            obj.insert("wantQty".into(), json!(want_ada.to_string()));
            obj.insert("wantAda".into(), json!(want_ada));
        }
        state.publish(mk(
            "dex_order",
            "dex",
            match side {
                "buy" => format!("Buy Order - {dex}"),
                "sell" => format!("Sell Order - {dex}"),
                _ => format!("Swap - {dex}"),
            },
            order_data,
        ));
    }
    if rng.gen_bool(0.14) {
        let dex = dexes[rng.gen_range(0..dexes.len())];
        let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
        let qty = rng.gen_range(100..5_000_000i64);
        let ada = rng.gen_range(50..9_000u64) * 1_000_000;
        let buy = rng.gen_bool(0.55);
        let data = if buy {
            json!({
                "dex": dex, "side": "buy", "ada": ada,
                "assets": { "items": [], "more": 0 },
                "wantMin": true, "wantQty": qty.to_string(),
                "want": { "items": [{
                    "unit": format!("{p}{n}"), "policy": p, "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                }], "more": 0 },
            })
        } else {
            json!({
                "dex": dex, "side": "sell", "ada": 4_000_000u64,
                "assets": { "items": [{
                    "unit": format!("{p}{n}"), "policy": p, "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                }], "more": 0 },
                "wantMin": true, "wantAda": ada, "wantQty": ada.to_string(),
            })
        };
        state.publish(mk("dex_fill", "dex", format!("Order Fill - {dex}"), data));
    }
    if rng.gen_bool(0.08) {
        let dex = dexes[rng.gen_range(0..dexes.len())];
        let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
        let redeem = rng.gen_bool(0.45);
        let ada = rng.gen_range(3..800u64) * 1_000_000;
        let qty = rng.gen_range(1_000..900_000i64);
        let lp_name = format!("{}_ADA_LQ", crate::parse::decode_asset_name(n).unwrap_or_else(|| "POOL".into()));
        let lp_hex = hex::encode(lp_name.as_bytes());
        let (side, assets, title) = if redeem {
            (
                "redeem",
                json!({ "items": [{
                    "unit": format!("{p}{lp_hex}"), "policy": p, "nameHex": lp_hex,
                    "name": lp_name, "qty": qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, &lp_hex),
                }], "more": 0 }),
                format!("LP Redeem - {dex}"),
            )
        } else {
            (
                "deposit",
                json!({ "items": [{
                    "unit": format!("{p}{n}"), "policy": p, "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                }], "more": 0 }),
                format!("LP Deposit - {dex}"),
            )
        };
        state.publish(mk(
            "dex_lp",
            "dex",
            title,
            json!({ "dex": dex, "side": side, "ada": ada, "assets": assets, "filled": false }),
        ));
    }
    if rng.gen_bool(0.03) {
        let dex = dexes[rng.gen_range(0..dexes.len())];
        let (p, n, _t) = demo_tokens[rng.gen_range(0..demo_tokens.len())];
        let ada = rng.gen_range(20..400u64) * 1_000_000;
        let want_qty = rng.gen_range(1_000..5_000_000i64);
        state.publish(mk(
            "dex_cancel",
            "dex",
            format!("Buy Cancelled - {dex}"),
            json!({
                "dex": dex,
                "side": "buy",
                "ada": ada,
                "assets": { "items": [], "more": 0 },
                "filled": false,
                "wantMin": true,
                "wantQty": want_qty.to_string(),
                "want": { "items": [{
                    "unit": format!("{p}{n}"), "policy": p, "nameHex": n,
                    "name": crate::parse::decode_asset_name(n),
                    "qty": want_qty.to_string(),
                    "fingerprint": crate::parse::asset_fingerprint(p, n),
                }], "more": 0 },
            }),
        ));
    }
    if rng.gen_bool(0.10) {
        let (event_type, title, iag, ada) = match rng.gen_range(0..9u8) {
            0 => ("stake_delegation", "Stake Delegation - Iagon", rng.gen_range(10_000..2_000_000u64) * 1_000_000, 2_000_000u64),
            1 => ("node_registration", "Node Registration - Iagon", 0u64, 2_000_000u64),
            2 => ("node_pledge", "Node Pledge - Iagon", rng.gen_range(50_000..5_000_000u64) * 1_000_000, 2_000_000u64),
            3 => ("earnings_claim", "Earnings Claim - Iagon", rng.gen_range(100..50_000u64) * 1_000_000, 1_500_000u64),
            4 => ("node_retirement", "Node Retirement - Iagon", 0u64, 2_000_000u64),
            5 => ("stake_withdrawal", "Stake Withdrawal - Iagon", rng.gen_range(10_000..2_000_000u64) * 1_000_000, 2_000_000u64),
            6 => ("position_listing", "Position Listing - Iagon", rng.gen_range(5_000..500_000u64) * 1_000_000, 2_000_000u64),
            7 => ("position_sale", "Position Sale - Iagon", rng.gen_range(5_000..500_000u64) * 1_000_000, rng.gen_range(20..500u64) * 1_000_000),
            _ => ("subscription", "Subscription - Iagon", 0u64, rng.gen_range(5..50u64) * 1_000_000),
        };
        let mut data = json!({
            "dapp": "Iagon",
            "eventType": event_type,
            "ada": ada,
        });
        if iag > 0 {
            let obj = data.as_object_mut().unwrap();
            obj.insert("iag".into(), json!(iag));
            obj.insert(
                "assets".into(),
                json!({
                    "items": [{
                        "unit": "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114494147",
                        "policy": "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114",
                        "nameHex": "494147",
                        "name": "IAG",
                        "qty": iag.to_string(),
                        "ticker": "IAG",
                        "decimals": 6,
                    }],
                    "more": 0
                }),
            );
        }
        state.publish(mk("dapp_activity", "dapp", title.into(), data));
    }

    // Cache a plausible raw tx so the detail modal has something to show.
    let inputs: Vec<Value> = (0..n_in)
        .map(|_| json!({ "transaction": { "id": rand_hex(rng, 64) }, "index": rng.gen_range(0..4) }))
        .collect();
    let outputs: Vec<Value> = (0..n_out)
        .map(|_| {
            json!({
                "address": fake_addr(rng),
                "value": { "ada": { "lovelace": ada / n_out as u64 } },
            })
        })
        .collect();
    state.cache_tx(
        tx_hash.clone(),
        json!({
            "id": tx_hash,
            "inputs": inputs,
            "outputs": outputs,
            "fee": { "ada": { "lovelace": fee } },
            "metadata": { "labels": { "674": { "json": { "msg": ["demo transaction"] } } } },
        }),
        json!({ "hash": block_hash, "height": height, "slot": slot, "timestamp": now }),
    );
}

fn block_event(hash: &str, height: u64, slot: u64, now: i64, tx_count: usize, rng: &mut StdRng) -> ChainEvent {
    ChainEvent {
        id: 0,
        parent_id: None,
        kind: "block".into(),
        category: "block".into(),
        slot,
        height: Some(height),
        block_hash: Some(hash.to_string()),
        tx_hash: None,
        timestamp: now,
        title: format!("Block {height}"),
        summary: format!("{tx_count} tx"),
        data: json!({
            "hash": hash,
            "height": height,
            "slot": slot,
            "size": rng.gen_range(600..88_000),
            "txCount": tx_count,
            "issuerPool": fake_pool(rng),
            "totalFees": tx_count as u64 * rng.gen_range(170_000..400_000u64),
            "totalOutput": tx_count as u64 * rng.gen_range(1..90_000u64) * 1_000_000,
            "era": "conway",
        }),
    }
}

fn emit_rollback(state: &Arc<AppState>, orphaned: &[BlockRef], to_slot: u64) {
    if orphaned.is_empty() {
        return;
    }
    let now = unix_now();
    state.publish(ChainEvent {
        id: 0,
        parent_id: None,
        kind: "rollback".into(),
        category: "alert".into(),
        slot: to_slot,
        height: None,
        block_hash: None,
        tx_hash: None,
        timestamp: now,
        title: "Chain Fork - Rollback".into(),
        summary: String::new(),
        data: json!({
            "toSlot": to_slot,
            "depth": orphaned.len(),
            "orphaned": orphaned.iter().map(|b| json!({ "hash": b.hash, "slot": b.slot, "height": b.height })).collect::<Vec<_>>(),
        }),
    });
    for b in orphaned {
        state.publish(ChainEvent {
            id: 0,
            parent_id: None,
            kind: "orphaned_block".into(),
            category: "alert".into(),
            slot: b.slot,
            height: Some(b.height),
            block_hash: Some(b.hash.clone()),
            tx_hash: None,
            timestamp: now,
            title: format!("Block {} Orphaned", b.height),
            summary: String::new(),
            data: json!({ "hash": b.hash, "slot": b.slot, "height": b.height }),
        });
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn rand_hex(rng: &mut StdRng, len: usize) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    (0..len).map(|_| HEX[rng.gen_range(0..16)] as char).collect()
}

fn fake_stake(rng: &mut StdRng) -> String {
    crate::parse::stake_address(&rand_hex(rng, 56), Some("verificationKey"), crate::config::Network::Mainnet)
}

fn fake_pool(rng: &mut StdRng) -> String {
    crate::parse::pool_id_from_vkey(&rand_hex(rng, 64)).unwrap_or_else(|| "pool1demo".into())
}

fn fake_drep(rng: &mut StdRng) -> String {
    crate::parse::drep_display(&json!({ "type": "registered", "id": rand_hex(rng, 56) }))
}

fn fake_addr(rng: &mut StdRng) -> String {
    format!("addr1q{}", rand_hex(rng, 50))
}
