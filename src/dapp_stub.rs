//! Empty dApp registry used when `src/dapp/` is not present at build time.

use crate::model::ChainEvent;
use serde_json::Value;

pub struct DappHit {
    pub kind: &'static str,
    pub title: String,
    pub data: Value,
}

/// No-op registry — core chain/DEX parsing still runs.
pub struct DappRegistry;

impl DappRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn with_restored_txs(_entries: Vec<(String, Value)>) -> Self {
        Self
    }

    pub fn scan_block(&self, _txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        Vec::new()
    }

    pub fn is_spend_graph_hub(&self, _outpoint: &str) -> bool {
        false
    }

    pub fn is_spend_graph_hub_address(&self, _addr: &str) -> bool {
        false
    }

    pub fn note_spend_graph_hubs(&self, _tx_hash: &str, _tx: &Value) {}
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
