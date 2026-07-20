use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single on-chain (or chain-level) event, ready to be rendered as a card.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainEvent {
    /// Monotonic id assigned by the server (clients use it for ordering/dedup)
    pub id: u64,
    /// Id of the containing event (block for a transaction, transaction for its
    /// child events), forming a block → transaction → detail hierarchy. `None`
    /// for top-level events (blocks, rollbacks, slot battles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<u64>,
    /// Machine-readable event kind, e.g. "transaction", "gov_vote"
    pub kind: String,
    /// Color/filter family: block | transaction | token | mint | staking |
    /// pool | governance | metadata | finance | dapp | alert
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

/// dApps filed under `finance` alongside the DEX venues, so a protocol that is
/// both (Dano Finance trades *and* lends) has one filter chip rather than two.
/// Names match each scanner's `DAPP` const and `DAPP_APPS` in
/// `static/dapp/mod.js`. Everything else - Iagon (storage), Wayup (NFT
/// marketplace) - stays under `dapp`.
pub const FINANCE_APPS: &[&str] = &[
    "Dano Finance",
    "FluidTokens",
    "Indigo Protocol",
    "Liqwid",
    "Optim Finance",
    "Strike",
    "Surf",
];

/// Category for a dApp hit, by app name.
pub fn category_for_dapp(dapp: &str) -> &'static str {
    if FINANCE_APPS.contains(&dapp) {
        "finance"
    } else {
        "dapp"
    }
}

/// Map the pre-merge categories of events already on disk onto the current
/// scheme: every `dex` event is now `finance`, and so is a `dapp` event from a
/// finance app. Without this, restored history would carry categories the UI no
/// longer has filters for and would silently vanish from the feed.
pub fn normalize_legacy_category(ev: &mut ChainEvent) {
    match ev.category.as_str() {
        "dex" => ev.category = "finance".into(),
        "dapp" => {
            let dapp = ev.data.get("dapp").and_then(Value::as_str).unwrap_or("");
            ev.category = category_for_dapp(dapp).into();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(category: &str, dapp: Option<&str>) -> ChainEvent {
        ChainEvent {
            id: 1,
            parent_id: None,
            kind: "dapp_activity".into(),
            category: category.into(),
            slot: 0,
            height: None,
            block_hash: None,
            tx_hash: None,
            timestamp: 0,
            title: String::new(),
            summary: String::new(),
            data: match dapp {
                Some(d) => json!({ "dapp": d }),
                None => json!({}),
            },
        }
    }

    #[test]
    fn finance_apps_share_the_dex_category() {
        assert_eq!(category_for_dapp("Dano Finance"), "finance");
        assert_eq!(category_for_dapp("Liqwid"), "finance");
        assert_eq!(category_for_dapp("Surf"), "finance");
    }

    #[test]
    fn non_finance_apps_keep_their_own_category() {
        assert_eq!(category_for_dapp("Iagon"), "dapp");
        assert_eq!(category_for_dapp("Wayup"), "dapp");
        assert_eq!(category_for_dapp(""), "dapp");
    }

    /// History written before the merge must land in a category the UI still
    /// filters on, or those events silently disappear from the feed.
    #[test]
    fn legacy_categories_are_remapped_on_load() {
        let mut e = ev("dex", None);
        normalize_legacy_category(&mut e);
        assert_eq!(e.category, "finance");

        let mut e = ev("dapp", Some("Liqwid"));
        normalize_legacy_category(&mut e);
        assert_eq!(e.category, "finance");

        let mut e = ev("dapp", Some("Iagon"));
        normalize_legacy_category(&mut e);
        assert_eq!(e.category, "dapp");
    }

    #[test]
    fn unrelated_categories_are_untouched() {
        for cat in ["transaction", "governance", "token", "finance"] {
            let mut e = ev(cat, None);
            normalize_legacy_category(&mut e);
            assert_eq!(e.category, cat);
        }
    }
}
