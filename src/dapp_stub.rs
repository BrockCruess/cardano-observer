//! Empty dApp registry used when `src/dapp/` is not present at build time.
//!
//! The DEX scanners live in that same optional tree, so this stub also
//! supplies an inert [`dex::DexRegistry`]: the binary still syncs the chain
//! and emits core events, just no `dapp`/`dex` ones.

use crate::model::ChainEvent;
use serde_json::Value;

/// Inert stand-in for `dapp::dex`.
pub mod dex {
    use crate::model::ChainEvent;
    use serde_json::Value;

    pub struct DexHit {
        pub kind: &'static str,
        pub title: String,
        pub data: Value,
    }

    pub struct DexRegistry;

    impl DexRegistry {
        pub fn new() -> Self {
            Self
        }

        pub fn with_restored_txs(_entries: &[(String, Value)]) -> Self {
            Self
        }

        pub fn scan_block(&self, _txs: &[(&str, &Value)]) -> Vec<(String, DexHit)> {
            Vec::new()
        }
    }

    impl Default for DexRegistry {
        fn default() -> Self {
            Self::new()
        }
    }

    /// No caches to refresh without the DEX modules.
    pub async fn refresh_dex_caches(_reg: std::sync::Arc<DexRegistry>) {}

    pub fn hit_to_event(
        hit: DexHit,
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
            category: "finance".into(),
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
}

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
        category: crate::model::category_for_dapp(
            hit.data.get("dapp").and_then(Value::as_str).unwrap_or(""),
        )
        .into(),
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
