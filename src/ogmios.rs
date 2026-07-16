//! Ogmios v6 chain-sync client: follows the chain from the current tip and
//! feeds parsed events into `AppState`. Reconnects forever with backoff.

use crate::config::{Config, Network};
use crate::dapp::DappRegistry;
use crate::deleg::{self, DelegationTracker};
use crate::dex::DexRegistry;
use crate::enrich::Enricher;
use crate::model::{BlockRef, EraSummary, ChainEvent, TimeModel, Tip};
use crate::parse;
use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const PIPELINE: usize = 25;

pub async fn run(
    config: Config,
    state: Arc<AppState>,
    dex: Arc<DexRegistry>,
    dapp: Arc<DappRegistry>,
    enricher: Arc<Enricher>,
    deleg: Arc<DelegationTracker>,
) {
    let mut backoff = 1u64;
    loop {
        state.set_status("connecting");
        match sync_once(&config, &state, &dex, &dapp, &enricher, &deleg).await {
            Ok(()) => backoff = 1,
            Err(e) => {
                tracing::warn!("ogmios connection lost: {e:#}");
                state.set_status("disconnected");
            }
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn sync_once(
    config: &Config,
    state: &Arc<AppState>,
    dex: &Arc<DexRegistry>,
    dapp: &Arc<DappRegistry>,
    enricher: &Arc<Enricher>,
    deleg: &Arc<DelegationTracker>,
) -> Result<()> {
    tracing::info!("connecting to ogmios at {}", config.ogmios_url);
    let (mut ws, _) = tokio_tungstenite::connect_async(&config.ogmios_url)
        .await
        .context("websocket connect failed")?;
    tracing::info!("connected to ogmios");

    // ── Discover slot→time conversion from the node itself ──────────────
    let start = rpc(&mut ws, "queryNetwork/startTime", json!({})).await?;
    let system_start = parse_start_time(&start)
        .ok_or_else(|| anyhow!("cannot parse queryNetwork/startTime: {start}"))?;
    let eras = rpc(&mut ws, "queryLedgerState/eraSummaries", json!({})).await?;
    let time_model = TimeModel { system_start, eras: parse_era_summaries(&eras) };
    *state.time_model.lock().unwrap() = time_model.clone();

    // ── Find where to start: our last-seen blocks, else the node's tip ──
    let recent_points: Vec<Value> = state
        .recent_blocks_points()
        .iter()
        .map(|b| json!({ "slot": b.slot, "id": b.hash }))
        .collect();
    let mut points = recent_points.clone();
    if points.is_empty() {
        let tip = rpc(&mut ws, "queryNetwork/tip", json!({})).await?;
        if tip.get("slot").is_some() {
            points.push(tip.clone());
        }
    }
    points.push(json!("origin"));
    let mut intersection = rpc(&mut ws, "findIntersection", json!({ "points": points })).await?;

    // If none of our resume points are on the node's chain (deep rollback or
    // stale persisted state), the intersection collapses to origin - never
    // replay the whole chain from genesis; restart from the tip instead.
    let matched_origin = intersection
        .get("intersection")
        .map(|i| i.get("slot").is_none())
        .unwrap_or(true);
    if matched_origin && !recent_points.is_empty() {
        tracing::warn!("saved resume points unknown to the node; starting from the tip instead");
        let tip = rpc(&mut ws, "queryNetwork/tip", json!({})).await?;
        let mut pts = Vec::new();
        if tip.get("slot").is_some() {
            pts.push(tip);
        }
        pts.push(json!("origin"));
        intersection = rpc(&mut ws, "findIntersection", json!({ "points": pts })).await?;
    }
    tracing::info!(
        "chain-sync intersection: {}",
        intersection.get("intersection").map(|v| v.to_string()).unwrap_or_default()
    );

    state.set_status("connected");

    // ── Pipeline nextBlock requests ──────────────────────────────────────
    for _ in 0..PIPELINE {
        send_req(&mut ws, "nextBlock", json!({}), json!("next")).await?;
    }

    loop {
        let msg = tokio::time::timeout(Duration::from_secs(600), ws.next())
            .await
            .context("no message from ogmios for 10 minutes")?
            .ok_or_else(|| anyhow!("ogmios closed the connection"))??;
        let text = match msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await?;
                continue;
            }
            Message::Close(_) => bail!("ogmios closed the connection"),
            _ => continue,
        };
        let v: Value = serde_json::from_str(text.as_ref()).context("bad json from ogmios")?;
        if v.get("method").and_then(Value::as_str) != Some("nextBlock") {
            continue;
        }
        let result = v
            .get("result")
            .ok_or_else(|| anyhow!("nextBlock error: {}", v.get("error").cloned().unwrap_or_default()))?;

        match result.get("direction").and_then(Value::as_str) {
            Some("forward") => {
                if let Some(block) = result.get("block") {
                    handle_forward(
                        state,
                        &time_model,
                        block,
                        result.get("tip"),
                        config.network,
                        dex,
                        dapp,
                        enricher,
                        deleg,
                    )
                    .await;
                }
            }
            Some("backward") => {
                // The first backward after (re)connecting normally just moves
                // us to the intersection point and orphans nothing. If blocks
                // we know about *are* above the intersection (a fork happened
                // while we were offline), handle_rollback reports them.
                let slot = result
                    .get("point")
                    .and_then(|p| p.get("slot"))
                    .and_then(Value::as_u64);
                if let Some(slot) = slot {
                    handle_rollback(state, &time_model, slot, result.get("point"));
                }
            }
            _ => {}
        }

        send_req(&mut ws, "nextBlock", json!({}), json!("next")).await?;
    }
}

async fn handle_forward(
    state: &Arc<AppState>,
    tm: &TimeModel,
    block: &Value,
    tip: Option<&Value>,
    network: Network,
    dex: &Arc<DexRegistry>,
    dapp: &Arc<DappRegistry>,
    enricher: &Arc<Enricher>,
    deleg: &Arc<DelegationTracker>,
) {
    let timestamp = block
        .get("slot")
        .and_then(Value::as_u64)
        .map(|s| tm.slot_to_unix(s))
        .unwrap_or(0);
    let Some(mut parsed) = parse::parse_block(block, timestamp, network, dex, dapp, deleg) else {
        return;
    };

    // Cache-miss from→to: one short batched Blockfrost account lookup.
    deleg::fill_missing_froms(enricher, &mut parsed.events).await;
    // Learn DRep names from any registration/update anchors in this block.
    enricher.warm_dreps_from_events(&parsed.events).await;
    // CIP-108 titles: Blockfrost once per new governance action.
    enricher.ensure_gov_action_titles(&parsed.events).await;

    state.counters.blocks.fetch_add(1, Ordering::Relaxed);
    state.counters.txs.fetch_add(parsed.txs.len() as u64, Ordering::Relaxed);

    // Cache tx details for the modal
    let block_ctx = json!({
        "hash": parsed.hash,
        "height": parsed.height,
        "slot": parsed.slot,
        "timestamp": timestamp,
    });
    for (hash, tx) in parsed.txs {
        state.cache_tx(hash, tx, block_ctx.clone());
    }

    // Orphan bookkeeping + slot/height battle detection
    let block_ref = BlockRef {
        hash: parsed.hash.clone(),
        slot: parsed.slot,
        height: parsed.height,
    };
    let battle = state.note_block(block_ref);

    for event in parsed.events {
        state.publish(event);
    }

    if let Some((loser, kind)) = battle {
        state.publish(ChainEvent {
            id: 0,
            kind: "slot_battle".into(),
            category: "alert".into(),
            slot: parsed.slot,
            height: Some(parsed.height),
            block_hash: Some(parsed.hash.clone()),
            tx_hash: None,
            timestamp,
            title: if kind == "slot" { "Slot Battle".into() } else { "Height Battle".into() },
            summary: String::new(),
            data: json!({
                "battle": kind,
                "winner": parsed.hash,
                "loser": loser.hash,
                "slot": parsed.slot,
                "loserSlot": loser.slot,
                "height": parsed.height,
            }),
        });
    }

    // Tip update
    let (epoch, epoch_progress) = tm.slot_to_epoch(parsed.slot);
    let network_height = tip
        .and_then(|t| t.get("height"))
        .and_then(Value::as_u64)
        .unwrap_or(parsed.height);
    state.set_tip(Tip {
        height: parsed.height,
        slot: parsed.slot,
        hash: parsed.hash,
        epoch,
        epoch_progress,
        timestamp,
    });
    if network_height > 0 && network_height != parsed.height {
        // still syncing towards the node tip; nothing special to do, the UI
        // simply renders blocks as fast as they arrive
    }
}

fn handle_rollback(state: &Arc<AppState>, tm: &TimeModel, slot: u64, point: Option<&Value>) {
    let orphaned = state.rollback_to(slot);
    if orphaned.is_empty() {
        return;
    }
    let timestamp = tm.slot_to_unix(slot);
    tracing::info!("rollback to slot {slot}: {} block(s) orphaned", orphaned.len());

    state.publish(ChainEvent {
        id: 0,
        kind: "rollback".into(),
        category: "alert".into(),
        slot,
        height: None,
        block_hash: point
            .and_then(|p| p.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        tx_hash: None,
        timestamp,
        title: "Chain Fork - Rollback".into(),
        summary: String::new(),
        data: json!({
            "toSlot": slot,
            "depth": orphaned.len(),
            "orphaned": orphaned.iter().map(|b| json!({
                "hash": b.hash, "slot": b.slot, "height": b.height,
            })).collect::<Vec<_>>(),
        }),
    });

    for b in &orphaned {
        state.publish(ChainEvent {
            id: 0,
            kind: "orphaned_block".into(),
            category: "alert".into(),
            slot: b.slot,
            height: Some(b.height),
            block_hash: Some(b.hash.clone()),
            tx_hash: None,
            timestamp,
            title: format!("Block {} Orphaned", b.height),
            summary: String::new(),
            data: json!({ "hash": b.hash, "slot": b.slot, "height": b.height }),
        });
    }
}

// ── JSON-RPC plumbing ─────────────────────────────────────────────────────

async fn send_req(ws: &mut WsStream, method: &str, params: Value, id: Value) -> Result<()> {
    let req = json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": id });
    ws.send(Message::Text(req.to_string().into())).await?;
    Ok(())
}

/// Send one request and wait for its (matching) response's `result`.
async fn rpc(ws: &mut WsStream, method: &str, params: Value) -> Result<Value> {
    send_req(ws, method, params, json!(method)).await?;
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .with_context(|| format!("timeout waiting for {method}"))?
            .ok_or_else(|| anyhow!("connection closed during {method}"))??;
        let Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(text.as_ref())?;
        if v.get("id") == Some(&json!(method)) || v.get("method").and_then(Value::as_str) == Some(method) {
            if let Some(err) = v.get("error") {
                bail!("{method} failed: {err}");
            }
            return v
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("{method}: no result"));
        }
    }
}

fn parse_start_time(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    let t = humantime::parse_rfc3339_weak(s).ok().or_else(|| humantime::parse_rfc3339(s).ok())?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

fn parse_era_summaries(v: &Value) -> Vec<EraSummary> {
    let mut eras = Vec::new();
    let Some(arr) = v.as_array() else { return eras };
    for era in arr {
        let start = era.get("start");
        let params = era.get("parameters");
        let time_s = start
            .and_then(|s| s.get("time"))
            .map(|t| t.get("seconds").and_then(Value::as_i64).or_else(|| t.as_i64()).unwrap_or(0))
            .unwrap_or(0);
        eras.push(EraSummary {
            start_time_s: time_s,
            start_slot: start.and_then(|s| s.get("slot")).and_then(Value::as_u64).unwrap_or(0),
            start_epoch: start.and_then(|s| s.get("epoch")).and_then(Value::as_u64).unwrap_or(0),
            end_slot: era
                .get("end")
                .and_then(|e| e.get("slot"))
                .and_then(Value::as_u64),
            epoch_length: params
                .and_then(|p| p.get("epochLength"))
                .and_then(Value::as_u64)
                .unwrap_or(432000),
            slot_length_ms: params
                .and_then(|p| p.get("slotLength"))
                .map(|s| s.get("milliseconds").and_then(Value::as_u64).or_else(|| s.as_u64().map(|x| x * 1000)).unwrap_or(1000))
                .unwrap_or(1000),
        });
    }
    eras
}
