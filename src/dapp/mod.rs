//! Shared dApp event detection.
//!
//! Per-dApp scanners live in sibling modules. This module only aggregates
//! their hits into `category: "dapp"` events; `data.dapp` names the app.

mod fluidtokens;
mod iagon;
mod indigo;
mod surf;
mod wayup;

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
    indigo: indigo::Scanner,
    fluidtokens: fluidtokens::Scanner,
    surf: surf::Scanner,
    wayup: wayup::Scanner,
}

impl DappRegistry {
    pub fn new() -> Self {
        Self {
            iagon: iagon::Scanner::new(),
            indigo: indigo::Scanner::new(),
            fluidtokens: fluidtokens::Scanner::new(),
            surf: surf::Scanner::new(),
            wayup: wayup::Scanner::new(),
        }
    }

    /// Like [`new`], then let each scanner rebuild any restart-sensitive state
    /// from restored `{ tx, block }` cache entries (no events emitted).
    pub fn with_restored_txs(entries: Vec<(String, Value)>) -> Self {
        let reg = Self::new();
        reg.iagon.warm_from_tx_entries(&entries);
        reg.indigo.warm_from_tx_entries(&entries);
        reg.fluidtokens.warm_from_tx_entries(&entries);
        reg.surf.warm_from_tx_entries(&entries);
        reg.wayup.warm_from_tx_entries(&entries);
        reg
    }

    /// Run every registered dApp scanner over the block's transactions.
    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        let mut hits = self.iagon.scan_block(txs);
        hits.extend(self.indigo.scan_block(txs));
        hits.extend(self.fluidtokens.scan_block(txs));
        hits.extend(self.surf.scan_block(txs));
        hits.extend(self.wayup.scan_block(txs));
        hits
    }

    /// Shared-script outpoints that must not create light-cone spend edges
    /// (e.g. Iagon rewards batcher, Surf pool UTxOs, Wayup Ask/Bid listings).
    pub fn is_spend_graph_hub(&self, outpoint: &str) -> bool {
        self.iagon.is_spend_graph_hub(outpoint)
            || self.surf.is_spend_graph_hub(outpoint)
            || self.wayup.is_spend_graph_hub(outpoint)
    }

    /// Address form of [`Self::is_spend_graph_hub`] for cached parent outputs.
    pub fn is_spend_graph_hub_address(&self, addr: &str) -> bool {
        self.iagon.is_spend_graph_hub_address(addr)
            || self.surf.is_spend_graph_hub_address(addr)
            || self.wayup.is_spend_graph_hub_address(addr)
    }

    /// Update hub outpoints after a tx is parsed (block order).
    pub fn note_spend_graph_hubs(&self, tx_hash: &str, tx: &Value) {
        self.iagon.note_spend_graph_hubs(tx_hash, tx);
        self.surf.note_spend_graph_hubs(tx_hash, tx);
        self.wayup.note_spend_graph_hubs(tx_hash, tx);
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
