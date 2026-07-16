//! Shared dApp event detection.
//!
//! Per-dApp scanners live in sibling modules. This module only aggregates
//! their hits into `category: "dapp"` events; `data.dapp` names the app.

mod iagon;

use crate::model::ChainEvent;
use serde_json::Value;

pub struct DappHit {
    pub kind: &'static str,
    pub title: String,
    pub data: Value,
}

/// Registry of dApp scanners consulted on every block.
pub struct DappRegistry {
    iagon: iagon::Scanner,
}

impl DappRegistry {
    pub fn new() -> Self {
        Self {
            iagon: iagon::Scanner::new(),
        }
    }

    /// Run every registered dApp scanner over the block's transactions.
    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        self.iagon.scan_block(txs)
    }
}

impl Default for DappRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn hit_to_event(
    hit: DappHit,
    slot: u64,
    height: u64,
    block_hash: &str,
    tx_hash: &str,
    timestamp: i64,
) -> ChainEvent {
    ChainEvent {
        id: 0,
        parent_id: None,
        kind: hit.kind.into(),
        category: "dapp".into(),
        slot,
        height: Some(height),
        block_hash: Some(block_hash.to_string()),
        tx_hash: Some(tx_hash.to_string()),
        timestamp,
        title: hit.title,
        summary: String::new(),
        data: hit.data,
    }
}
