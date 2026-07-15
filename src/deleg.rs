//! Tracks the last-seen pool / DRep for each stake credential so delegation
//! events can show `from → to`. Seeded from persisted history; live updates
//! cost nothing. Cache misses are filled via Blockfrost before the events
//! are published.

use crate::enrich::Enricher;
use crate::model::ChainEvent;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

const TRACKER_CAP: usize = 50_000;

#[derive(Clone, Default)]
struct StakeDeleg {
    pool: Option<String>,
    drep: Option<String>,
}

#[derive(Default)]
pub struct DelegationTracker {
    map: Mutex<HashMap<String, StakeDeleg>>,
}

impl DelegationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay history oldest→newest so the map holds the latest known state.
    pub fn seed_from_events(&self, events: &[ChainEvent]) {
        for ev in events {
            let d = &ev.data;
            let Some(stake) = d.get("stake").and_then(Value::as_str) else {
                continue;
            };
            match ev.kind.as_str() {
                "delegation" => {
                    if let Some(pool) = d.get("pool").and_then(Value::as_str) {
                        self.set_pool(stake, pool);
                    }
                }
                "vote_delegation" => {
                    if let Some(drep) = d.get("drep").and_then(Value::as_str) {
                        self.set_drep(stake, drep);
                    }
                }
                _ => {}
            }
        }
    }

    /// Return previous pool (if any) and record the new one.
    pub fn swap_pool(&self, stake: &str, new_pool: &str) -> Option<String> {
        let mut map = self.map.lock().unwrap();
        self.evict_if_needed(&mut map);
        let entry = map.entry(stake.to_string()).or_default();
        let prev = entry.pool.clone();
        entry.pool = Some(new_pool.to_string());
        prev.filter(|p| p != new_pool)
    }

    /// Return previous DRep (if any) and record the new one.
    pub fn swap_drep(&self, stake: &str, new_drep: &str) -> Option<String> {
        let mut map = self.map.lock().unwrap();
        self.evict_if_needed(&mut map);
        let entry = map.entry(stake.to_string()).or_default();
        let prev = entry.drep.clone();
        entry.drep = Some(new_drep.to_string());
        prev.filter(|d| d != new_drep)
    }

    fn set_pool(&self, stake: &str, pool: &str) {
        let mut map = self.map.lock().unwrap();
        self.evict_if_needed(&mut map);
        map.entry(stake.to_string()).or_default().pool = Some(pool.to_string());
    }

    fn set_drep(&self, stake: &str, drep: &str) {
        let mut map = self.map.lock().unwrap();
        self.evict_if_needed(&mut map);
        map.entry(stake.to_string()).or_default().drep = Some(drep.to_string());
    }

    fn evict_if_needed(&self, map: &mut HashMap<String, StakeDeleg>) {
        if map.len() >= TRACKER_CAP {
            // Drop an arbitrary ~10% - order doesn't matter for a soft cache.
            let drop_n = TRACKER_CAP / 10;
            let keys: Vec<String> = map.keys().take(drop_n).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }
    }
}

/// For delegation events still missing `fromPool` / `fromDrep`, batch-query
/// account state and fill when the live tip still shows the *previous* target
/// (i.e. the new cert hasn't been reflected yet). Cheap no-op when the tip
/// already matches the new target. Pool misses also try Blockfrost delegation
/// history (skipping the current tx) as a second shot.
pub async fn fill_missing_froms(enricher: &Enricher, events: &mut [ChainEvent]) {
    let mut need_pool: Vec<(usize, String, String, Option<String>)> = Vec::new(); // idx, stake, new, tx
    let mut need_drep: Vec<(usize, String, String)> = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let d = &ev.data;
        let Some(stake) = d.get("stake").and_then(Value::as_str) else {
            continue;
        };
        match ev.kind.as_str() {
            "delegation" => {
                if d.get("fromPool").is_none() {
                    if let Some(pool) = d.get("pool").and_then(Value::as_str) {
                        need_pool.push((
                            i,
                            stake.to_string(),
                            pool.to_string(),
                            ev.tx_hash.clone(),
                        ));
                    }
                }
            }
            "vote_delegation" => {
                if d.get("fromDrep").is_none() {
                    if let Some(drep) = d.get("drep").and_then(Value::as_str) {
                        need_drep.push((i, stake.to_string(), drep.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    if need_pool.is_empty() && need_drep.is_empty() {
        return;
    }

    let mut stakes: Vec<String> = need_pool
        .iter()
        .map(|(_, s, _, _)| s.clone())
        .chain(need_drep.iter().map(|(_, s, _)| s.clone()))
        .collect();
    stakes.sort();
    stakes.dedup();

    let info = enricher.account_delegations(&stakes).await;
    let mut still_need_hist: Vec<(usize, String, String, Option<String>)> = Vec::new();
    for (i, stake, new_pool, tx) in need_pool {
        if let Some(row) = info.get(&stake) {
            if let Some(prev) = row.get("pool").and_then(Value::as_str) {
                if prev != new_pool {
                    if let Some(obj) = events[i].data.as_object_mut() {
                        obj.insert("fromPool".into(), json!(prev));
                    }
                    continue;
                }
            }
        }
        still_need_hist.push((i, stake, new_pool, tx));
    }
    for (i, stake, new_drep) in need_drep {
        let Some(row) = info.get(&stake) else { continue };
        let Some(prev) = row.get("drep").and_then(Value::as_str) else { continue };
        if prev != new_drep {
            if let Some(obj) = events[i].data.as_object_mut() {
                obj.insert("fromDrep".into(), json!(prev));
            }
        }
    }

    // Tip already shows the new pool - use BF delegation history if available.
    for (i, stake, new_pool, tx) in still_need_hist {
        if let Some(prev) = enricher
            .previous_pool_from_history(&stake, &new_pool, tx.as_deref())
            .await
        {
            if let Some(obj) = events[i].data.as_object_mut() {
                obj.insert("fromPool".into(), json!(prev));
            }
        }
    }
}

