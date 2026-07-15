//! Shared application state: event ring buffer, tx detail cache, broadcast
//! channel to browsers, orphan/slot-battle tracking, and chain tip stats.

use crate::model::{BlockRef, ChainEvent, TimeModel, Tip};
use crate::persist::Persister;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct AppState {
    pub network: &'static str,
    event_buffer: usize,
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
        event_buffer: usize,
        tx_cache_size: usize,
        persister: Option<Arc<Persister>>,
    ) -> Self {
        let (sender, _) = broadcast::channel(4096);
        AppState {
            network,
            event_buffer,
            tx_cache_size,
            next_id: AtomicU64::new(1),
            persister,
            events: Mutex::new(VecDeque::with_capacity(event_buffer)),
            txs: Mutex::new(TxCache { order: VecDeque::new(), map: HashMap::new() }),
            tip: Mutex::new(Tip::default()),
            time_model: Mutex::new(TimeModel::default()),
            recent_blocks: Mutex::new(VecDeque::with_capacity(720)),
            orphans: Mutex::new(Vec::new()),
            sender,
            source_status: Mutex::new("connecting"),
            counters: Counters::default(),
        }
    }

    /// Restore state saved by a previous run. Call before serving clients;
    /// nothing is re-broadcast or re-persisted.
    pub fn restore(&self, events: Vec<ChainEvent>, txs: Vec<(String, Value)>) {
        if events.is_empty() && txs.is_empty() {
            return;
        }
        tracing::info!("restored {} events and {} cached txs from disk", events.len(), txs.len());
        let max_id = events.iter().map(|e| e.id).max().unwrap_or(0);
        self.next_id.store(max_id + 1, Ordering::Relaxed);
        {
            let mut buf = self.events.lock().unwrap();
            for event in events {
                if buf.len() >= self.event_buffer {
                    buf.pop_front();
                }
                buf.push_back(event);
            }
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
        let msg = json!({ "type": "event", "event": &event }).to_string();
        if let Some(p) = &self.persister {
            p.append_event(&event);
        }
        {
            let mut buf = self.events.lock().unwrap();
            if buf.len() >= self.event_buffer {
                buf.pop_front();
            }
            buf.push_back(event);
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
        let events = self.events.lock().unwrap();
        let start = events.len().saturating_sub(limit);
        let slice: Vec<&ChainEvent> = events.iter().skip(start).collect();
        json!({
            "type": "snapshot",
            "network": self.network,
            "tip": &*self.tip.lock().unwrap(),
            "source": *self.source_status.lock().unwrap(),
            "events": slice,
        })
    }

    /// Page of events older than `before_id` (oldest→newest). Prefers disk
    /// history; falls back to the in-memory ring when persistence is off.
    pub fn events_before(&self, before_id: u64, limit: usize) -> Value {
        let limit = limit.clamp(1, 500);
        if let Some(p) = &self.persister {
            let (events, exhausted) = p.events_before(before_id, limit);
            return json!({ "events": events, "exhausted": exhausted });
        }
        let buf = self.events.lock().unwrap();
        let older: Vec<&ChainEvent> = buf.iter().filter(|e| e.id < before_id).collect();
        let start = older.len().saturating_sub(limit);
        let page = &older[start..];
        let exhausted = start == 0;
        json!({ "events": page, "exhausted": exhausted })
    }

    pub fn stats(&self) -> Value {
        json!({
            "type": "stats",
            "blocks": self.counters.blocks.load(Ordering::Relaxed),
            "txs": self.counters.txs.load(Ordering::Relaxed),
            "events": self.counters.events.load(Ordering::Relaxed),
            "rollbacks": self.counters.rollbacks.load(Ordering::Relaxed),
        })
    }
}
