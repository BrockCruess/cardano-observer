//! Shared application state: event ring buffer, tx detail cache, broadcast
//! channel to browsers, orphan/slot-battle tracking, and chain tip stats.

use crate::dapp::DappRegistry;
use crate::enrich::Enricher;
use crate::model::{BlockRef, ChainEvent, TimeModel, Tip};
use crate::parse;
use crate::persist::Persister;
use crate::trending::{KeywordMeta, TrendTerm, Trending};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct AppState {
    pub network: &'static str,
    /// How long events stay in the in-memory ring (trending + fast search).
    event_retention_secs: i64,
    tx_cache_size: usize,
    next_id: AtomicU64,
    persister: Option<Arc<Persister>>,
    pub events: Mutex<VecDeque<ChainEvent>>,
    /// tx hash -> (raw ogmios tx, block context)
    txs: Mutex<TxCache>,
    pub tip: Mutex<Tip>,
    pub time_model: Mutex<TimeModel>,
    /// Recent blocks on the (believed) current chain, for orphan detection
    recent_blocks: Mutex<VecDeque<BlockRef>>,
    /// Recently orphaned blocks, checked for slot/height battles
    orphans: Mutex<Vec<BlockRef>>,
    pub sender: broadcast::Sender<String>,
    pub source_status: Mutex<&'static str>,
    pub counters: Counters,
    trending: Mutex<Trending>,
    /// Optional enricher for resolving asset/pool tickers into trending terms.
    keyword_meta: Mutex<Option<Arc<Enricher>>>,
}

#[derive(Default)]
pub struct Counters {
    pub blocks: AtomicU64,
    pub txs: AtomicU64,
    pub events: AtomicU64,
    pub rollbacks: AtomicU64,
}

struct TxCache {
    order: VecDeque<String>,
    map: HashMap<String, Value>,
}

impl AppState {
    pub fn new(
        network: &'static str,
        event_retention_hours: u64,
        tx_cache_size: usize,
        persister: Option<Arc<Persister>>,
    ) -> Self {
        let event_retention_secs = (event_retention_hours.max(1) as i64) * 3600;
        let (sender, _) = broadcast::channel(4096);
        AppState {
            network,
            event_retention_secs,
            tx_cache_size,
            next_id: AtomicU64::new(1),
            persister,
            events: Mutex::new(VecDeque::new()),
            txs: Mutex::new(TxCache {
                order: VecDeque::new(),
                map: HashMap::new(),
            }),
            tip: Mutex::new(Tip::default()),
            time_model: Mutex::new(TimeModel::default()),
            recent_blocks: Mutex::new(VecDeque::with_capacity(720)),
            orphans: Mutex::new(Vec::new()),
            sender,
            source_status: Mutex::new("connecting"),
            counters: Counters::default(),
            trending: Mutex::new(Trending::new(event_retention_secs)),
            keyword_meta: Mutex::new(None),
        }
    }

    pub fn event_retention_secs(&self) -> i64 {
        self.event_retention_secs
    }

    pub fn persister(&self) -> Option<Arc<Persister>> {
        self.persister.clone()
    }

    /// Attach the enricher so trending can resolve CIP-26 / pool tickers.
    pub fn set_keyword_meta(&self, enricher: Arc<Enricher>) {
        *self.keyword_meta.lock().unwrap() = Some(enricher);
    }

    fn meta_ref(&self) -> Option<Arc<Enricher>> {
        self.keyword_meta.lock().unwrap().clone()
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn retention_cutoff(now: i64, retention_secs: i64) -> i64 {
        now.saturating_sub(retention_secs)
    }

    /// Drop events older than the retention window (oldest are at the front).
    fn trim_events(buf: &mut VecDeque<ChainEvent>, cutoff: i64) {
        while buf.front().is_some_and(|e| e.timestamp < cutoff) {
            buf.pop_front();
        }
    }

    /// Seed trending from restored / historical events.
    pub fn seed_trending(&self, events: impl IntoIterator<Item = ChainEvent>) {
        let meta = self.meta_ref();
        let meta_ref = meta.as_ref().map(|e| e.as_ref() as &dyn KeywordMeta);
        let mut trending = self.trending.lock().unwrap();
        for event in events {
            trending.record_event(&event, meta_ref);
        }
        let _ = trending.snapshot(Self::now_unix());
    }

    /// Current top-10 trending terms.
    pub fn trending_top(&self) -> Vec<TrendTerm> {
        self.trending.lock().unwrap().top(Self::now_unix())
    }

    /// Restore state saved by a previous run. Call before serving clients;
    /// nothing is re-broadcast or re-persisted. Events outside the retention
    /// window are skipped.
    pub fn restore(&self, events: Vec<ChainEvent>, txs: Vec<(String, Value)>) {
        if events.is_empty() && txs.is_empty() {
            return;
        }
        let cutoff = Self::retention_cutoff(Self::now_unix(), self.event_retention_secs);
        let mut kept: Vec<ChainEvent> = events
            .into_iter()
            .filter(|e| e.timestamp >= cutoff)
            .collect();
        // Drop no-op redelegations restored from disk (from == to).
        crate::deleg::drop_noop_redelegations(&mut kept);
        // Keep ring in chain order. Disk order alone is wrong if events were
        // ever appended out of slot order (e.g. a one-off historical inject).
        kept.sort_by(|a, b| a.slot.cmp(&b.slot).then_with(|| a.id.cmp(&b.id)));
        tracing::info!(
            "restored {} events (≤{}h window) and {} cached txs from disk",
            kept.len(),
            self.event_retention_secs / 3600,
            txs.len()
        );
        let max_id = kept.iter().map(|e| e.id).max().unwrap_or(0);
        self.next_id.store(max_id + 1, Ordering::Relaxed);
        {
            let mut buf = self.events.lock().unwrap();
            buf.clear();
            buf.extend(kept);
        }
        let mut cache = self.txs.lock().unwrap();
        for (hash, entry) in txs {
            if !cache.map.contains_key(&hash) {
                cache.order.push_back(hash.clone());
            }
            cache.map.insert(hash, entry);
        }
    }

    /// Seed the recent-blocks list (oldest first) from a previous run so the
    /// chain-sync client can re-intersect where it left off and backfill
    /// everything the server missed while it was down.
    pub fn restore_recent_blocks(&self, blocks: Vec<BlockRef>) {
        if blocks.is_empty() {
            return;
        }
        tracing::info!(
            "resuming chain-sync from block {} (slot {}) - missed blocks will be backfilled",
            blocks.last().map(|b| b.height).unwrap_or(0),
            blocks.last().map(|b| b.slot).unwrap_or(0),
        );
        let mut recent = self.recent_blocks.lock().unwrap();
        for b in blocks {
            if recent.len() >= 720 {
                recent.pop_front();
            }
            recent.push_back(b);
        }
    }

    /// Assign an id, buffer, persist, and broadcast a single event.
    ///
    /// Returns the id assigned to the event, or `None` if it was dropped before
    /// publishing (e.g. a DEX event for a token outside the CIP-26 registry).
    /// Callers use the returned id to wire up child events' `parent_id`.
    pub fn publish(&self, mut event: ChainEvent) -> Option<u64> {
        let meta = self.meta_ref();
        if let Some(e) = meta.as_ref() {
            e.stamp_event_assets(&mut event);
            e.stamp_event_dreps(&mut event);
            e.stamp_event_gov_actions(&mut event);
            self.stamp_event_scam_with_tx(&e, &mut event);
            if !e.keep_dex_event(&event) {
                return None;
            }
        }
        event.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let assigned_id = event.id;
        self.counters.events.fetch_add(1, Ordering::Relaxed);
        {
            let meta_ref = meta.as_ref().map(|e| e.as_ref() as &dyn KeywordMeta);
            self.trending
                .lock()
                .unwrap()
                .record_event(&event, meta_ref);
        }
        let msg = json!({ "type": "event", "event": &event }).to_string();
        if let Some(p) = &self.persister {
            p.append_event(&event);
        }
        let cutoff = Self::retention_cutoff(Self::now_unix(), self.event_retention_secs);
        let trimmed_events;
        {
            let mut buf = self.events.lock().unwrap();
            let before = buf.len();
            buf.push_back(event);
            Self::trim_events(&mut buf, cutoff);
            trimmed_events = buf.len() < before + 1;
        }
        if trimmed_events {
            self.trim_txs_to_retention(cutoff);
        }
        let _ = self.sender.send(msg);
        Some(assigned_id)
    }

    /// Apply in-memory CIP-26 decimals/tickers onto every buffered event (boot).
    pub fn stamp_buffered_assets(&self) {
        let Some(enricher) = self.meta_ref() else { return };
        let mut buf = self.events.lock().unwrap();
        for ev in buf.iter_mut() {
            enricher.stamp_event_assets(ev);
        }
        let before = buf.len();
        buf.retain(|ev| enricher.keep_dex_event(ev));
        let dropped = before - buf.len();
        if dropped > 0 {
            tracing::info!("dropped {dropped} DEX events with tokens outside CIP-26");
        }
    }

    /// Narrow token-transfer events to only the assets that actually move to a
    /// *new* payment credential, dropping any event that turns out to be a pure
    /// internal reshuffle (self-send, change output, consolidation).
    ///
    /// Meant to run in the ingest path after this block's txs are cached and
    /// before events are published, so internal reshuffles never reach the feed
    /// or persistence. Ownership is proven via [`parse::asset_changes_hands`],
    /// resolving spent-input credentials from the tx cache (memory + on-disk
    /// hash index). When an asset cannot be proven internal (e.g. a spent parent
    /// isn't cached), it is kept.
    pub fn drop_internal_token_transfers(&self, events: &mut Vec<ChainEvent>) {
        let before = events.len();
        events.retain_mut(|ev| self.keep_token_transfer(ev));
        let dropped = before - events.len();
        if dropped > 0 {
            tracing::debug!("dropped {dropped} internal token-transfer event(s)");
        }
    }

    /// Rewrites a token-transfer event's asset list to only the assets that
    /// change hands and returns whether it survives (any external asset left).
    /// Non-transfer events pass through untouched.
    fn keep_token_transfer(&self, ev: &mut ChainEvent) -> bool {
        if ev.kind != "token_transfer" {
            return true;
        }
        let Some(hash) = ev.tx_hash.clone() else {
            return true;
        };
        let Some(entry) = self.get_tx(&hash) else {
            return true; // no body ⇒ can't prove internal; keep as-is
        };
        let Some(tx) = entry.get("tx") else {
            return true;
        };
        let transferred = parse::transferred_assets(tx);
        if transferred.is_empty() {
            return true;
        }

        // Pull spent parents so input payment credentials can be resolved.
        let mut parents: HashMap<String, Value> = HashMap::new();
        let empty = Vec::new();
        for i in tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty) {
            let Some(src) = i
                .get("transaction")
                .and_then(|t| t.get("id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if parents.contains_key(src) {
                continue;
            }
            if let Some(pentry) = self.get_tx(src) {
                if let Some(ptx) = pentry.get("tx").cloned() {
                    parents.insert(src.to_string(), ptx);
                }
            }
        }

        let total = transferred.len();
        let external: Vec<(String, String, i128)> = transferred
            .into_iter()
            .filter(|(policy, name_hex, _)| {
                parse::asset_changes_hands(tx, policy, name_hex, |src, idx| {
                    let parent = parents.get(src)?;
                    let o = parent.get("outputs")?.as_array()?.get(idx as usize)?;
                    Some((
                        o.get("address")?.as_str()?.to_string(),
                        o.get("value")?.clone(),
                    ))
                })
            })
            .collect();

        if external.is_empty() {
            return false; // pure internal reshuffle → drop
        }
        if external.len() < total {
            // Some assets were internal change; keep only the ones that moved.
            let refs: Vec<&(String, String, i128)> = external.iter().collect();
            if let Some(obj) = ev.data.as_object_mut() {
                obj.insert("assets".into(), parse::asset_list(&refs));
            }
            ev.title = if external.len() == 1 {
                "Token Transfer".into()
            } else {
                format!("Token Transfer ×{}", external.len())
            };
        }
        true
    }

    /// Flag buffered token-transfer events that move a known scam fingerprint
    /// across payment credentials (not same-pkh internal reshuffles).
    pub fn stamp_buffered_scam(&self) {
        let Some(enricher) = self.meta_ref() else { return };
        let hashes: Vec<String> = {
            let buf = self.events.lock().unwrap();
            buf.iter()
                .filter(|e| e.kind == "token_transfer")
                .filter_map(|e| e.tx_hash.clone())
                .collect()
        };
        let mut txs: HashMap<String, Value> = HashMap::new();
        for h in &hashes {
            if let Some(entry) = self.get_tx(h) {
                if let Some(tx) = entry.get("tx").cloned() {
                    // Pull parent bodies so input payment creds can be resolved.
                    let empty = Vec::new();
                    for i in tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty) {
                        let Some(src) = i
                            .get("transaction")
                            .and_then(|t| t.get("id"))
                            .and_then(Value::as_str)
                        else {
                            continue;
                        };
                        if txs.contains_key(src) {
                            continue;
                        }
                        if let Some(pentry) = self.get_tx(src) {
                            if let Some(ptx) = pentry.get("tx").cloned() {
                                txs.insert(src.to_string(), ptx);
                            }
                        }
                    }
                    txs.insert(h.clone(), tx);
                }
            }
        }
        let mut buf = self.events.lock().unwrap();
        for ev in buf.iter_mut() {
            self.stamp_event_scam_with_maps(&enricher, ev, &txs);
        }
    }

    fn stamp_event_scam_with_tx(&self, enricher: &Enricher, event: &mut ChainEvent) {
        let tx = event
            .tx_hash
            .as_deref()
            .and_then(|h| self.get_tx(h))
            .and_then(|entry| entry.get("tx").cloned());
        let mut parents: HashMap<String, Value> = HashMap::new();
        if let Some(ref tx) = tx {
            let empty = Vec::new();
            for i in tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty) {
                let Some(src) = i
                    .get("transaction")
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                if parents.contains_key(src) {
                    continue;
                }
                if let Some(entry) = self.get_tx(src) {
                    if let Some(ptx) = entry.get("tx").cloned() {
                        parents.insert(src.to_string(), ptx);
                    }
                }
            }
        }
        let mut txs = HashMap::new();
        if let (Some(h), Some(tx)) = (event.tx_hash.clone(), tx) {
            txs.insert(h, tx);
        }
        for (k, v) in parents {
            txs.insert(k, v);
        }
        self.stamp_event_scam_with_maps(enricher, event, &txs);
    }

    fn stamp_event_scam_with_maps(
        &self,
        enricher: &Enricher,
        event: &mut ChainEvent,
        txs: &HashMap<String, Value>,
    ) {
        let tx = event
            .tx_hash
            .as_deref()
            .and_then(|h| txs.get(h));
        enricher.stamp_event_scam(event, |policy, name_hex| {
            let Some(tx) = tx else {
                // No body ⇒ can't prove a hand-change; don't false-flag consolidations.
                return false;
            };
            parse::asset_changes_hands(tx, policy, name_hex, |src, idx| {
                let parent = txs.get(src)?;
                let outs = parent.get("outputs")?.as_array()?;
                let o = outs.get(idx as usize)?;
                Some((
                    o.get("address")?.as_str()?.to_string(),
                    o.get("value")?.clone(),
                ))
            })
        });
    }

    /// Apply cached DRep givenNames onto every buffered event (boot).
    pub fn stamp_buffered_dreps(&self) {
        let Some(enricher) = self.meta_ref() else { return };
        let mut buf = self.events.lock().unwrap();
        for ev in buf.iter_mut() {
            enricher.stamp_event_dreps(ev);
        }
    }

    /// Recompute `inputTxs` on buffered transaction events using cached tx
    /// bodies + current spend-graph hub rules (Iagon batcher, …). Historical
    /// JSONL edges predate hub filtering; without this, light-cone keeps the
    /// old all-claimers-linked graph until each tx ages out of retention.
    pub fn rewrite_buffered_input_txs(&self, dapp: &DappRegistry) {
        let cache = self.txs.lock().unwrap();
        let mut rewritten = 0u64;
        let mut buf = self.events.lock().unwrap();
        for ev in buf.iter_mut() {
            if ev.kind != "transaction" {
                continue;
            }
            let Some(hash) = ev.tx_hash.as_deref() else { continue };
            let Some(entry) = cache.map.get(hash) else { continue };
            let Some(tx) = entry.get("tx") else { continue };
            let empty = Vec::new();
            let inputs = tx.get("inputs").and_then(Value::as_array).unwrap_or(&empty);
            let parent_addr = |src: &str, index: u64| -> Option<String> {
                let parent = cache.map.get(src)?;
                let ptx = parent.get("tx")?;
                let outs = ptx.get("outputs")?.as_array()?;
                outs.get(index as usize)?
                    .get("address")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            };
            let next = parse::collect_input_txs(hash, inputs, dapp, parent_addr);
            let prev = ev
                .data
                .get("inputTxs")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if next != prev {
                if let Some(obj) = ev.data.as_object_mut() {
                    obj.insert("inputTxs".into(), json!(next));
                    rewritten += 1;
                }
            }
        }
        if rewritten > 0 {
            tracing::info!("rewrote inputTxs on {rewritten} buffered transaction events (spend-graph hubs)");
        }
    }

    /// Apply cached CIP-108 gov-action titles onto every buffered event (boot).
    pub fn stamp_buffered_gov_actions(&self) {
        let Some(enricher) = self.meta_ref() else { return };
        let mut buf = self.events.lock().unwrap();
        for ev in buf.iter_mut() {
            enricher.stamp_event_gov_actions(ev);
        }
    }

    pub fn cache_tx(&self, hash: String, tx: Value, block: Value) {
        let entry = json!({ "tx": tx, "block": block });
        if let Some(p) = &self.persister {
            p.append_tx(&hash, &entry);
        }
        {
            let mut cache = self.txs.lock().unwrap();
            if !cache.map.contains_key(&hash) {
                cache.order.push_back(hash.clone());
            }
            cache.map.insert(hash, entry);
        }
        let cutoff = Self::retention_cutoff(Self::now_unix(), self.event_retention_secs);
        self.trim_txs_memory(cutoff);
    }

    /// Persist a tx body only if it is not already indexed on disk (or memory).
    pub fn cache_tx_if_absent(&self, hash: String, tx: Value, block: Value) -> bool {
        if let Some(p) = &self.persister {
            if p.has_tx(&hash) {
                return false;
            }
        } else if self.txs.lock().unwrap().map.contains_key(&hash) {
            return false;
        }
        self.cache_tx(hash, tx, block);
        true
    }

    /// Drop in-memory tx bodies older than the retention window.
    /// Full history stays on disk (indexed) for deep scrollback modals.
    fn trim_txs_to_retention(&self, cutoff: i64) {
        self.trim_txs_memory(cutoff);
    }

    fn trim_txs_memory(&self, cutoff: i64) {
        let mut cache = self.txs.lock().unwrap();
        let stale: Vec<String> = cache
            .map
            .iter()
            .filter(|(_, e)| tx_entry_timestamp(e) < cutoff)
            .map(|(h, _)| h.clone())
            .collect();
        for h in &stale {
            cache.map.remove(h);
        }
        if !stale.is_empty() {
            let stale_set: std::collections::HashSet<&str> =
                stale.iter().map(String::as_str).collect();
            cache.order.retain(|h| !stale_set.contains(h.as_str()));
        }
        // Optional soft ceiling (TX_CACHE > 0) — OOM guard; can drop in-window txs.
        if self.tx_cache_size > 0 {
            while cache.map.len() > self.tx_cache_size {
                let Some(old) = cache.order.pop_front() else { break };
                cache.map.remove(&old);
            }
        }
    }

    pub fn get_tx(&self, hash: &str) -> Option<Value> {
        if let Some(v) = self.txs.lock().unwrap().map.get(hash).cloned() {
            return Some(v);
        }
        let Some(p) = &self.persister else {
            return None;
        };
        let entry = p.find_tx(hash)?;
        // Re-warm memory for txs still inside the retention window.
        let cutoff = Self::retention_cutoff(Self::now_unix(), self.event_retention_secs);
        if tx_entry_timestamp(&entry) >= cutoff {
            let mut cache = self.txs.lock().unwrap();
            if !cache.map.contains_key(hash) {
                cache.order.push_back(hash.to_string());
                cache.map.insert(hash.to_string(), entry.clone());
            }
        }
        Some(entry)
    }

    /// Snapshot of in-memory tx cache entries (`hash` → `{ tx, block }`) for
    /// warming stateful scanners after restore.
    pub fn cached_tx_entries(&self) -> Vec<(String, Value)> {
        self.txs
            .lock()
            .unwrap()
            .map
            .iter()
            .map(|(h, e)| (h.clone(), e.clone()))
            .collect()
    }

    pub fn set_tip(&self, tip: Tip) {
        let msg = json!({ "type": "tip", "tip": &tip }).to_string();
        *self.tip.lock().unwrap() = tip;
        let _ = self.sender.send(msg);
    }

    pub fn set_status(&self, status: &'static str) {
        *self.source_status.lock().unwrap() = status;
        let _ = self
            .sender
            .send(json!({ "type": "status", "source": status }).to_string());
    }

    /// Record a block believed to extend the current chain. Returns a
    /// battle event descriptor if this block competes with a recent orphan.
    pub fn note_block(&self, block: BlockRef) -> Option<(BlockRef, &'static str)> {
        let battle = {
            let orphans = self.orphans.lock().unwrap();
            orphans
                .iter()
                .find(|o| o.slot == block.slot && o.hash != block.hash)
                .map(|o| (o.clone(), "slot"))
                .or_else(|| {
                    orphans
                        .iter()
                        .find(|o| o.height == block.height && o.hash != block.hash)
                        .map(|o| (o.clone(), "height"))
                })
        };
        let mut recent = self.recent_blocks.lock().unwrap();
        if recent.len() >= 720 {
            recent.pop_front();
        }
        recent.push_back(block);
        battle
    }

    /// Handle a chain-sync rollback: returns the blocks now orphaned.
    pub fn rollback_to(&self, slot: u64) -> Vec<BlockRef> {
        let mut recent = self.recent_blocks.lock().unwrap();
        let mut orphaned = Vec::new();
        while recent.back().is_some_and(|b| b.slot > slot) {
            orphaned.push(recent.pop_back().unwrap());
        }
        if !orphaned.is_empty() {
            self.counters.rollbacks.fetch_add(1, Ordering::Relaxed);
            let mut orphans = self.orphans.lock().unwrap();
            orphans.extend(orphaned.iter().cloned());
            let len = orphans.len();
            if len > 100 {
                orphans.drain(..len - 100);
            }
        }
        orphaned
    }

    /// Most recent block points (newest first), used to re-find an
    /// intersection after a reconnect.
    pub fn recent_blocks_points(&self) -> Vec<BlockRef> {
        let recent = self.recent_blocks.lock().unwrap();
        recent.iter().rev().take(10).cloned().collect()
    }

    pub fn snapshot(&self, limit: usize) -> Value {
        // Lock order: trending before events (matches publish).
        let trending = self.trending.lock().unwrap().top(Self::now_unix());
        let events = self.events.lock().unwrap();
        let start = events.len().saturating_sub(limit);
        let slice: Vec<&ChainEvent> = events.iter().skip(start).collect();
        json!({
            "type": "snapshot",
            "network": self.network,
            "tip": &*self.tip.lock().unwrap(),
            "source": *self.source_status.lock().unwrap(),
            "events": slice,
            "trending": trending,
            "buffered": events.len(),
            "retention_hours": self.event_retention_secs / 3600,
        })
    }

    /// Entire in-memory retention window (for browser-side search index).
    pub fn retention_buffer(&self) -> Value {
        let events = self.events.lock().unwrap();
        json!({
            "events": &*events,
            "buffered": events.len(),
            "retention_hours": self.event_retention_secs / 3600,
            "exhausted": true,
        })
    }

    /// One page of the in-memory retention window for progressive client hydrate.
    /// Same cursor semantics as [`Self::events_before`], but never falls through
    /// to disk — `exhausted` means the retention window is fully covered.
    pub fn retention_buffer_page(&self, before_id: u64, limit: usize) -> Value {
        let limit = limit.clamp(1, 5_000);
        let buf = self.events.lock().unwrap();
        let older: Vec<&ChainEvent> = buf.iter().filter(|e| e.id < before_id).collect();
        let start = older.len().saturating_sub(limit);
        let page: Vec<&ChainEvent> = older[start..].to_vec();
        let exhausted = start == 0;
        json!({
            "events": page,
            "buffered": buf.len(),
            "retention_hours": self.event_retention_secs / 3600,
            "exhausted": exhausted,
        })
    }

    /// Search the in-memory retention window. Returns matching events newest-first
    /// in one response (capped) so the client can paginate the DOM locally.
    pub fn search_buffered(&self, query: &str, before: Option<u64>, limit: usize) -> Value {
        let q = query.trim().to_ascii_lowercase();
        // Allow a large page so the browser can hold all matches in JS and
        // only render a small DOM window at a time.
        let limit = limit.clamp(1, 5_000);
        if q.is_empty() {
            return json!({
                "events": [],
                "scanned": 0,
                "matched": 0,
                "total": 0,
                "exhausted": true,
                "truncated": false,
                "retention_hours": self.event_retention_secs / 3600,
            });
        }
        let before_id = before.unwrap_or(u64::MAX);
        let meta = self.meta_ref();
        let meta_ref = meta.as_ref().map(|e| e.as_ref() as &dyn KeywordMeta);
        let buf = self.events.lock().unwrap();
        let scanned = buf.len();
        let mut hits: Vec<&ChainEvent> = Vec::new();
        let mut total: usize = 0;
        // Newest first.
        for ev in buf.iter().rev() {
            if ev.id >= before_id {
                continue;
            }
            if event_matches_query(ev, &q, meta_ref) {
                total += 1;
                if hits.len() < limit {
                    hits.push(ev);
                }
            }
        }
        let truncated = total > hits.len();
        let matched = hits.len();
        json!({
            "events": hits,
            "scanned": scanned,
            "matched": matched,
            "total": total,
            "exhausted": !truncated,
            "truncated": truncated,
            "retention_hours": self.event_retention_secs / 3600,
        })
    }

    /// Page of events older than `before_id` (oldest→newest). Serves the
    /// in-memory retention window first (fast path for trending searches),
    /// then falls through to disk for anything older.
    pub fn events_before(&self, before_id: u64, limit: usize) -> Value {
        let limit = limit.clamp(1, 500);

        {
            let buf = self.events.lock().unwrap();
            let older: Vec<&ChainEvent> = buf.iter().filter(|e| e.id < before_id).collect();
            if !older.is_empty() {
                let start = older.len().saturating_sub(limit);
                let page: Vec<&ChainEvent> = older[start..].to_vec();
                let reached_oldest_in_memory = start == 0;
                if !reached_oldest_in_memory {
                    return json!({ "events": page, "exhausted": false });
                }
                // Returned everything in memory older than before_id. If there
                // is no disk history, we're done; otherwise the next page may
                // come from JSONL once before_id moves past the memory floor.
                if self.persister.is_none() {
                    return json!({ "events": page, "exhausted": true });
                }
                return json!({ "events": page, "exhausted": false });
            }
        }

        if let Some(p) = &self.persister {
            let (events, exhausted) = p.events_before(before_id, limit);
            return json!({ "events": events, "exhausted": exhausted });
        }
        json!({ "events": [], "exhausted": true })
    }

    pub fn stats(&self) -> Value {
        let buffered = self.events.lock().unwrap().len();
        json!({
            "type": "stats",
            "blocks": self.counters.blocks.load(Ordering::Relaxed),
            "txs": self.counters.txs.load(Ordering::Relaxed),
            "events": self.counters.events.load(Ordering::Relaxed),
            "rollbacks": self.counters.rollbacks.load(Ordering::Relaxed),
            "buffered": buffered,
            "retention_hours": self.event_retention_secs / 3600,
        })
    }
}

/// Substring match against the same fields the browser search haystack uses.
fn event_matches_query(ev: &ChainEvent, q: &str, meta: Option<&dyn KeywordMeta>) -> bool {
    if q.is_empty() {
        return false;
    }
    if contains_ci(&ev.title, q)
        || contains_ci(&ev.kind, q)
        || contains_ci(&ev.category, q)
    {
        return true;
    }
    if ev.tx_hash.as_deref().is_some_and(|h| contains_ci(h, q)) {
        return true;
    }
    if ev.block_hash.as_deref().is_some_and(|h| contains_ci(h, q)) {
        return true;
    }
    // Walk JSON strings in-place — far cheaper than serializing every event.
    if value_contains_ci(&ev.data, q) {
        return true;
    }
    // Resolved CIP-26 / pool labels (may not appear verbatim in data JSON).
    if let Some(meta) = meta {
        for key in ["assets", "want"] {
            if let Some(items) = ev
                .data
                .get(key)
                .and_then(|a| a.get("items"))
                .and_then(Value::as_array)
            {
                for a in items {
                    if let Some(unit) = a.get("unit").and_then(Value::as_str) {
                        if meta
                            .asset_label(unit)
                            .is_some_and(|l| contains_ci(&l, q))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        for key in ["pool", "fromPool", "issuerPool"] {
            if let Some(id) = ev.data.get(key).and_then(Value::as_str) {
                if meta.pool_label(id).is_some_and(|l| contains_ci(&l, q)) {
                    return true;
                }
            }
        }
    }
    false
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    // needle is already lowercase ASCII.
    hay.to_ascii_lowercase().contains(needle)
}

fn tx_entry_timestamp(entry: &Value) -> i64 {
    entry
        .get("block")
        .and_then(|b| b.get("timestamp"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn value_contains_ci(v: &Value, q: &str) -> bool {
    match v {
        Value::String(s) => contains_ci(s, q),
        Value::Array(arr) => arr.iter().any(|x| value_contains_ci(x, q)),
        Value::Object(map) => {
            map.iter()
                .any(|(k, x)| contains_ci(k, q) || value_contains_ci(x, q))
        }
        Value::Number(n) => n.to_string().contains(q),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bech32::{Bech32, Hrp};

    fn base_addr(payment_hex: &str, stake_hex: &str) -> String {
        let mut bytes = vec![0x01]; // CIP-19 type 0 (key+key), mainnet
        bytes.extend(hex::decode(payment_hex).unwrap());
        bytes.extend(hex::decode(stake_hex).unwrap());
        bech32::encode::<Bech32>(Hrp::parse("addr").unwrap(), &bytes).unwrap()
    }

    fn token_transfer(hash: &str) -> ChainEvent {
        ChainEvent {
            id: 0,
            parent_id: None,
            kind: "token_transfer".into(),
            category: "token".into(),
            slot: 1,
            height: Some(1),
            block_hash: None,
            tx_hash: Some(hash.into()),
            timestamp: 1,
            title: "Token Transfer".into(),
            summary: String::new(),
            data: json!({}),
        }
    }

    fn block_ctx() -> Value {
        // Recent timestamp so the cached txs stay inside the retention window.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        json!({ "hash": "b", "height": 1, "slot": 1, "timestamp": now })
    }

    #[test]
    fn drops_internal_keeps_external_token_transfers() {
        let state = AppState::new("mainnet", 24, 0, None);
        let stake = "cc".repeat(28);
        let alice = base_addr(&"aa".repeat(28), &stake);
        let bob = base_addr(&"bb".repeat(28), &stake);
        let policy = "dd".repeat(28);
        let name = "544f4b"; // TOK

        // Parent UTxO: Alice holds one TOK.
        let parent = json!({
            "outputs": [{
                "address": alice,
                "value": { "ada": { "lovelace": 5_000_000u64 }, (policy.clone()): { (name): 1 } }
            }]
        });
        state.cache_tx("parent".into(), parent, block_ctx());

        // Internal: Alice spends her UTxO, TOK returns to Alice (change / consolidation).
        let internal = json!({
            "inputs": [{ "transaction": { "id": "parent" }, "index": 0 }],
            "outputs": [
                { "address": alice, "value": { "ada": { "lovelace": 2_000_000u64 }, (policy.clone()): { (name): 1 } } },
                { "address": alice, "value": { "ada": { "lovelace": 2_800_000u64 } } }
            ]
        });
        state.cache_tx("internal".into(), internal, block_ctx());

        // External: Alice sends the TOK to Bob, ADA change back to herself.
        let external = json!({
            "inputs": [{ "transaction": { "id": "parent" }, "index": 0 }],
            "outputs": [
                { "address": bob, "value": { "ada": { "lovelace": 1_500_000u64 }, (policy.clone()): { (name): 1 } } },
                { "address": alice, "value": { "ada": { "lovelace": 3_300_000u64 } } }
            ]
        });
        state.cache_tx("external".into(), external, block_ctx());

        let mut events = vec![
            token_transfer("internal"),
            token_transfer("external"),
            // Non-transfer events are never touched.
            ChainEvent {
                kind: "transaction".into(),
                tx_hash: Some("external".into()),
                ..token_transfer("external")
            },
        ];
        state.drop_internal_token_transfers(&mut events);

        let kept: Vec<_> = events
            .iter()
            .filter(|e| e.kind == "token_transfer")
            .filter_map(|e| e.tx_hash.clone())
            .collect();
        assert_eq!(kept, vec!["external".to_string()]);
        assert_eq!(events.len(), 2); // external transfer + the transaction event
    }

    #[test]
    fn keeps_transfer_when_parent_unresolved() {
        // Without the spent parent cached, ownership can't be proven internal,
        // so a genuine-looking multi-wallet tx is kept (conservative default).
        let state = AppState::new("mainnet", 24, 0, None);
        let stake = "cc".repeat(28);
        let alice = base_addr(&"aa".repeat(28), &stake);
        let bob = base_addr(&"bb".repeat(28), &stake);
        let policy = "dd".repeat(28);
        let name = "544f4b";
        let tx = json!({
            "inputs": [{ "transaction": { "id": "uncached" }, "index": 0 }],
            "outputs": [
                { "address": bob, "value": { "ada": { "lovelace": 1_500_000u64 }, (policy.clone()): { (name): 1 } } },
                { "address": alice, "value": { "ada": { "lovelace": 2_000_000u64 } } }
            ]
        });
        state.cache_tx("tx".into(), tx, block_ctx());
        let mut events = vec![token_transfer("tx")];
        state.drop_internal_token_transfers(&mut events);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn mixed_tx_keeps_only_the_asset_that_moves() {
        // Alice sends TOK to Bob and keeps TOK2 as change: the event survives
        // but its asset list is narrowed to only the asset that changed hands.
        let state = AppState::new("mainnet", 24, 0, None);
        let stake = "cc".repeat(28);
        let alice = base_addr(&"aa".repeat(28), &stake);
        let bob = base_addr(&"bb".repeat(28), &stake);
        let policy = "dd".repeat(28);
        let tok = "544f4b"; // TOK  → Bob
        let tok2 = "544f4b32"; // TOK2 → change back to Alice

        let parent = json!({
            "outputs": [{
                "address": alice,
                "value": { "ada": { "lovelace": 5_000_000u64 },
                    (policy.clone()): { (tok): 1, (tok2): 1 } }
            }]
        });
        state.cache_tx("parent2".into(), parent, block_ctx());

        let tx = json!({
            "inputs": [{ "transaction": { "id": "parent2" }, "index": 0 }],
            "outputs": [
                { "address": bob, "value": { "ada": { "lovelace": 1_500_000u64 }, (policy.clone()): { (tok): 1 } } },
                { "address": alice, "value": { "ada": { "lovelace": 3_000_000u64 }, (policy.clone()): { (tok2): 1 } } }
            ]
        });
        state.cache_tx("mixed".into(), tx, block_ctx());

        let mut events = vec![token_transfer("mixed")];
        state.drop_internal_token_transfers(&mut events);
        assert_eq!(events.len(), 1);
        let items = events[0].data["assets"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "only the moved asset should remain");
        assert_eq!(items[0]["nameHex"], tok);
    }
}
