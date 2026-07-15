//! Shared application state: event ring buffer, tx detail cache, broadcast
//! channel to browsers, orphan/slot-battle tracking, and chain tip stats.

use crate::enrich::Enricher;
use crate::model::{BlockRef, ChainEvent, TimeModel, Tip};
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
        let kept: Vec<ChainEvent> = events
            .into_iter()
            .filter(|e| e.timestamp >= cutoff)
            .collect();
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
            if cache.map.len() >= self.tx_cache_size {
                if let Some(old) = cache.order.pop_front() {
                    cache.map.remove(&old);
                }
            }
            cache.order.push_back(hash.clone());
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
    pub fn publish(&self, mut event: ChainEvent) {
        event.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.counters.events.fetch_add(1, Ordering::Relaxed);
        {
            let meta = self.meta_ref();
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
        {
            let mut buf = self.events.lock().unwrap();
            buf.push_back(event);
            let cutoff = Self::retention_cutoff(Self::now_unix(), self.event_retention_secs);
            Self::trim_events(&mut buf, cutoff);
        }
        let _ = self.sender.send(msg);
    }

    pub fn cache_tx(&self, hash: String, tx: Value, block: Value) {
        let entry = json!({ "tx": tx, "block": block });
        if let Some(p) = &self.persister {
            p.append_tx(&hash, &entry);
        }
        let mut cache = self.txs.lock().unwrap();
        if cache.map.len() >= self.tx_cache_size {
            if let Some(old) = cache.order.pop_front() {
                cache.map.remove(&old);
            }
        }
        cache.order.push_back(hash.clone());
        cache.map.insert(hash, entry);
    }

    pub fn get_tx(&self, hash: &str) -> Option<Value> {
        self.txs.lock().unwrap().map.get(hash).cloned()
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
