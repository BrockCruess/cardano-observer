use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single on-chain (or chain-level) event, ready to be rendered as a card.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainEvent {
    /// Monotonic id assigned by the server (clients use it for ordering/dedup)
    pub id: u64,
    /// Machine-readable event kind, e.g. "transaction", "gov_vote"
    pub kind: String,
    /// Color/filter family: block | transaction | token | mint | staking |
    /// pool | governance | metadata | dex | dapp | alert
    pub category: String,
    pub slot: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    /// Unix seconds (derived from the slot via era summaries)
    pub timestamp: i64,
    pub title: String,
    pub summary: String,
    /// Kind-specific structured payload for the card body
    pub data: Value,
}

/// Chain tip snapshot shown in the header.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Tip {
    pub height: u64,
    pub slot: u64,
    pub hash: String,
    pub epoch: u64,
    /// 0.0 .. 1.0 progress through the current epoch
    pub epoch_progress: f64,
    pub timestamp: i64,
}

/// Lightweight reference to a block we have seen, used for orphan detection.
#[derive(Clone, Debug)]
pub struct BlockRef {
    pub hash: String,
    pub slot: u64,
    pub height: u64,
}

/// Slot→wallclock/epoch conversion derived from Ogmios era summaries.
#[derive(Clone, Debug, Default)]
pub struct TimeModel {
    /// Network system start, unix seconds
    pub system_start: i64,
    pub eras: Vec<EraSummary>,
}

#[derive(Clone, Debug)]
pub struct EraSummary {
    pub start_time_s: i64, // relative to system start
    pub start_slot: u64,
    pub start_epoch: u64,
    pub end_slot: Option<u64>,
    pub epoch_length: u64,
    pub slot_length_ms: u64,
}

impl TimeModel {
    pub fn slot_to_unix(&self, slot: u64) -> i64 {
        match self.era_for(slot) {
            Some(e) => {
                self.system_start
                    + e.start_time_s
                    + ((slot - e.start_slot) as i64 * e.slot_length_ms as i64) / 1000
            }
            // Fallback: assume 1s slots from system start (post-Byron behaviour)
            None => self.system_start + slot as i64,
        }
    }

    pub fn slot_to_epoch(&self, slot: u64) -> (u64, f64) {
        match self.era_for(slot) {
            Some(e) if e.epoch_length > 0 => {
                let delta = slot - e.start_slot;
                let epoch = e.start_epoch + delta / e.epoch_length;
                let progress = (delta % e.epoch_length) as f64 / e.epoch_length as f64;
                (epoch, progress)
            }
            _ => (0, 0.0),
        }
    }

    fn era_for(&self, slot: u64) -> Option<&EraSummary> {
        self.eras
            .iter()
            .rev()
            .find(|e| slot >= e.start_slot && e.end_slot.is_none_or(|end| slot < end))
            // A slot beyond the last known era boundary still belongs to the
            // final era in practice (the safe zone just hasn't been extended)
            .or_else(|| self.eras.last())
    }
}
