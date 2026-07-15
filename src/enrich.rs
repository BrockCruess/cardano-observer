//! Metadata enrichment: in-memory Cardano token registry (CIP-26) and stake
//! pool ticker cache loaded from disk at boot. Live Blockfrost calls only run
//! for assets/pools missing from those durable caches.

use crate::config::Config;
use crate::pools::{PoolCache, PoolEntry};
use crate::registry::TokenRegistry;
use crate::trending::KeywordMeta;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

pub struct Enricher {
    http: reqwest::Client,
    blockfrost_url: Option<String>,
    project_id: Option<String>,
    registry_url: String,
    /// Full CIP-26 registry - subject → name/ticker/decimals.
    registry: TokenRegistry,
    /// Durable pool ticker cache - pool1… → ticker/name.
    pool_cache: PoolCache,
    assets: Mutex<HashMap<String, Value>>,
}

const CACHE_CAP: usize = 20_000;

impl Enricher {
    pub async fn new(config: &Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("http client");

        let cache_dir = config.data_dir.as_ref().map(Path::new);
        let registry = match TokenRegistry::load(
            &http,
            cache_dir,
            &config.token_registry_zip,
            config.token_registry_refresh,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("token registry load failed ({e:#}) - live lookups only");
                TokenRegistry::empty()
            }
        };

        let pool_cache = match PoolCache::load(
            &http,
            cache_dir,
            config.blockfrost_url.as_deref(),
            config.blockfrost_project_id.as_deref(),
            config.pool_cache_refresh,
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("pool cache load failed ({e:#}) - live lookups only");
                PoolCache::empty()
            }
        };

        Enricher {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("http client"),
            blockfrost_url: config.blockfrost_url.clone(),
            project_id: config.blockfrost_project_id.clone(),
            registry_url: config.token_registry_url.clone(),
            registry,
            pool_cache,
            assets: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry_len(&self) -> usize {
        self.registry.len()
    }

    pub fn pool_cache_len(&self) -> usize {
        self.pool_cache.len()
    }

    /// Sync CIP-26 ticker/name for trending keyword extraction.
    pub fn asset_label(&self, unit: &str) -> Option<String> {
        let e = self.registry.get(unit)?;
        e.ticker
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| e.name.clone().filter(|s| !s.is_empty()))
    }

    /// Stamp CIP-26 decimals/ticker/name onto every `{unit, qty}` asset object in
    /// an event so the UI can format quantities without a per-token round-trip.
    pub fn stamp_event_assets(&self, event: &mut crate::model::ChainEvent) {
        stamp_json_assets(&mut event.data, &self.registry);
    }

    /// Drop swap/order DEX events whose tokens are not in CIP-26 (no liquidity
    /// signal worth showing). LP events are kept — share tokens are rarely registered.
    pub fn keep_dex_event(&self, event: &crate::model::ChainEvent) -> bool {
        if !matches!(
            event.kind.as_str(),
            "dex_order" | "dex_fill" | "dex_cancel"
        ) {
            return true;
        }
        let d = &event.data;
        let side = d.get("side").and_then(Value::as_str).unwrap_or("");
        let want_items = d
            .get("want")
            .and_then(|w| w.get("items"))
            .and_then(Value::as_array);
        let asset_items = d
            .get("assets")
            .and_then(|w| w.get("items"))
            .and_then(Value::as_array);
        let has_want = d.get("wantAda").is_some()
            || want_items.map(|a| !a.is_empty()).unwrap_or(false);
        let has_paid_tok = asset_items.map(|a| !a.is_empty()).unwrap_or(false);

        // Incomplete ask — hide rather than show a one-sided card.
        match side {
            "buy" | "sell" if !has_want => return false,
            "swap" if !has_paid_tok || !has_want => return false,
            _ => {}
        }

        for items in [asset_items, want_items].into_iter().flatten() {
            for a in items {
                let Some(unit) = a.get("unit").and_then(Value::as_str) else {
                    continue;
                };
                if !unit.is_empty() && self.registry.get(unit).is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// Compact registry dump for the browser (unit → decimals/ticker/name).
    pub fn registry_assets_json(&self) -> Value {
        self.registry.to_assets_json()
    }

    /// Sync pool ticker/name for trending keyword extraction.
    pub fn pool_label(&self, pool_id: &str) -> Option<String> {
        let e = self.pool_cache.get(pool_id)?;
        e.ticker
            .filter(|s| !s.is_empty())
            .or_else(|| e.name.filter(|s| !s.is_empty()))
    }

    fn bf(&self, path: &str) -> Option<reqwest::RequestBuilder> {
        let base = self.blockfrost_url.as_ref()?;
        let mut req = self.http.get(format!("{base}{path}"));
        if let Some(pid) = &self.project_id {
            req = req.header("project_id", pid);
        }
        Some(req)
    }

    /// Asset metadata for a unit (policy hex + asset-name hex).
    pub async fn asset(&self, unit: &str) -> Value {
        if !unit.chars().all(|c| c.is_ascii_hexdigit()) || unit.len() < 56 || unit.len() > 120 {
            return json!({ "error": "bad unit" });
        }
        // Prefer the in-memory CIP-26 registry - no network round-trip.
        if let Some(hit) = self.registry.to_json(unit) {
            return hit;
        }
        if let Some(hit) = self.assets.lock().unwrap().get(unit) {
            return hit.clone();
        }
        // Unregistered / unknown tokens: Blockfrost, then registry HTTP.
        let mut meta = self.asset_from_blockfrost(unit).await;
        if !meta_has_decimals(&meta) {
            if let Some(reg) = self.asset_from_registry_http(unit).await {
                meta = Some(merge_asset_meta(meta, reg));
            }
        }
        // Still unknown after registry + Blockfrost + HTTP: CIP-26 default is 0.
        let mut meta = meta.unwrap_or_else(|| json!({ "unit": unit }));
        if !meta_has_decimals(&Some(meta.clone())) {
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("decimals".into(), json!(0));
                obj.insert("decimalsDefaulted".into(), json!(true));
            }
        }
        let mut cache = self.assets.lock().unwrap();
        if cache.len() >= CACHE_CAP {
            cache.clear();
        }
        cache.insert(unit.to_string(), meta.clone());
        meta
    }

    async fn asset_from_blockfrost(&self, unit: &str) -> Option<Value> {
        let req = self.bf(&format!("/assets/{unit}"))?;
        let v: Value = tokio::time::timeout(Duration::from_millis(1500), req.send())
            .await
            .ok()?
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let m = v.get("metadata").filter(|m| !m.is_null());
        let onchain = v.get("onchain_metadata").filter(|m| !m.is_null());
        Some(json!({
            "unit": unit,
            "name": m.and_then(|m| m.get("name")).or_else(|| onchain.and_then(|m| m.get("name"))),
            "ticker": m.and_then(|m| m.get("ticker")),
            "decimals": m.and_then(|m| m.get("decimals")).filter(|d| !d.is_null()),
            "logo": m.and_then(|m| m.get("logo")),
            "image": onchain.and_then(|m| m.get("image")),
            "fingerprint": v.get("fingerprint"),
            "quantity": v.get("quantity"),
            "mintTxCount": v.get("mint_or_burn_count"),
        }))
    }

    async fn asset_from_registry_http(&self, unit: &str) -> Option<Value> {
        let url = format!("{}/metadata/{unit}", self.registry_url);
        let v: Value = tokio::time::timeout(Duration::from_millis(1500), self.http.get(url).send())
            .await
            .ok()?
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let field = |k: &str| v.get(k).and_then(|f| f.get("value")).cloned();
        Some(json!({
            "unit": unit,
            "name": field("name"),
            "ticker": field("ticker"),
            "decimals": field("decimals"),
            "logo": field("logo"),
        }))
    }

    /// Stake pool ticker/name/homepage.
    pub async fn pool(&self, pool_id: &str) -> Value {
        if !pool_id.starts_with("pool1") || pool_id.len() > 64 {
            return json!({ "error": "bad pool id" });
        }
        // Durable cache first - never re-fetch a pool we already know.
        if let Some(hit) = self.pool_cache.get(pool_id) {
            return hit.to_json(pool_id);
        }
        let meta = self
            .pool_from_blockfrost(pool_id)
            .await
            .unwrap_or_else(|| json!({ "pool": pool_id }));
        let entry = PoolEntry {
            ticker: meta
                .get("ticker")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            name: meta
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            homepage: meta
                .get("homepage")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        self.pool_cache.remember(pool_id, entry);
        meta
    }

}

impl KeywordMeta for Enricher {
    fn asset_label(&self, unit: &str) -> Option<String> {
        Enricher::asset_label(self, unit)
    }

    fn pool_label(&self, pool_id: &str) -> Option<String> {
        Enricher::pool_label(self, pool_id)
    }
}

impl Enricher {
    async fn pool_from_blockfrost(&self, pool_id: &str) -> Option<Value> {
        let req = self.bf(&format!("/pools/{pool_id}/metadata"))?;
        let v: Value = req.send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
        let ticker = v.get("ticker").and_then(Value::as_str).filter(|s| !s.is_empty());
        let name = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty());
        Some(json!({
            "pool": pool_id,
            "ticker": ticker,
            "name": name,
            "homepage": v.get("homepage").and_then(Value::as_str),
        }))
    }

    /// Batch account delegation snapshot (pool + drep) for stake addresses.
    pub async fn account_delegations(&self, stakes: &[String]) -> HashMap<String, Value> {
        if stakes.is_empty() {
            return HashMap::new();
        }
        self.accounts_from_blockfrost(stakes)
            .await
            .unwrap_or_default()
    }

    async fn accounts_from_blockfrost(&self, stakes: &[String]) -> Option<HashMap<String, Value>> {
        if self.blockfrost_url.is_none() {
            return None;
        }
        let mut out = HashMap::new();
        // Parallelism capped - RYO nodes dislike huge bursts.
        let mut set = tokio::task::JoinSet::new();
        for stake in stakes.iter().cloned().take(32) {
            let http = self.http.clone();
            let base = self.blockfrost_url.clone()?;
            let pid = self.project_id.clone();
            set.spawn(async move {
                let mut req = http.get(format!("{base}/accounts/{stake}"));
                if let Some(pid) = pid {
                    req = req.header("project_id", pid);
                }
                let v: Value = tokio::time::timeout(Duration::from_millis(900), req.send())
                    .await
                    .ok()?
                    .ok()?
                    .error_for_status()
                    .ok()?
                    .json()
                    .await
                    .ok()?;
                let pool = v
                    .get("pool_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let drep = v
                    .get("drep_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                Some((stake, json!({ "pool": pool, "drep": drep })))
            });
        }
        loop {
            let Some(res) = set.join_next().await else { break };
            if let Ok(Some((stake, row))) = res {
                out.insert(stake, row);
            }
        }
        Some(out)
    }

    /// Previous pool from Blockfrost delegation history, skipping the current tx.
    pub async fn previous_pool_from_history(
        &self,
        stake: &str,
        new_pool: &str,
        current_tx: Option<&str>,
    ) -> Option<String> {
        let req = self.bf(&format!(
            "/accounts/{stake}/delegations?count=5&order=desc"
        ))?;
        let v: Value = tokio::time::timeout(Duration::from_millis(900), req.send())
            .await
            .ok()?
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        for row in v.as_array()? {
            let tx = row.get("tx_hash").and_then(Value::as_str);
            if current_tx.is_some() && tx == current_tx {
                continue;
            }
            let pool = row.get("pool_id").and_then(Value::as_str)?;
            if pool != new_pool {
                return Some(pool.to_string());
            }
        }
        None
    }

    /// Fallback tx lookup when a hash has left the in-memory ring buffer.
    pub async fn tx_fallback(&self, hash: &str) -> Option<Value> {
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) || hash.len() != 64 {
            return None;
        }
        let tx: Value = self.bf(&format!("/txs/{hash}"))?.send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
        let utxos: Option<Value> = match self.bf(&format!("/txs/{hash}/utxos")) {
            Some(req) => req.send().await.ok()?.error_for_status().ok()?.json().await.ok(),
            None => None,
        };
        Some(json!({ "blockfrost": { "tx": tx, "utxos": utxos } }))
    }
}

fn meta_has_decimals(meta: &Option<Value>) -> bool {
    meta.as_ref()
        .and_then(|m| m.get("decimals"))
        .map(|d| !d.is_null())
        .unwrap_or(false)
}

/// Walk event JSON and attach registry decimals/ticker onto asset-like objects.
fn stamp_json_assets(v: &mut Value, reg: &TokenRegistry) {
    match v {
        Value::Object(map) => {
            let unit = map.get("unit").and_then(Value::as_str).map(str::to_string);
            if let Some(unit) = unit {
                if map.contains_key("qty") {
                    if let Some(e) = reg.get(&unit) {
                        // Omitted CIP-26 decimals ⇒ 0.
                        map.insert("decimals".into(), json!(e.decimals.unwrap_or(0)));
                        if let Some(t) = e.ticker.as_ref().filter(|s| !s.is_empty()) {
                            map.insert("ticker".into(), json!(t));
                        }
                        let name_empty = map
                            .get("name")
                            .and_then(Value::as_str)
                            .map(|s| s.is_empty())
                            .unwrap_or(true);
                        if name_empty {
                            if let Some(n) = e.name.as_ref().filter(|s| !s.is_empty()) {
                                map.insert("name".into(), json!(n));
                            }
                        }
                    } else if !map.contains_key("decimals") {
                        // Not in CIP-26 (e.g. SONGMARKETCAP): Cardano default is 0.
                        // Blockfrost may refine later via /api/asset.
                        map.insert("decimals".into(), json!(0));
                    }
                }
            }
            for child in map.values_mut() {
                stamp_json_assets(child, reg);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                stamp_json_assets(child, reg);
            }
        }
        _ => {}
    }
}

/// Prefer fields already present on `base`; fill gaps from `extra`.
fn merge_asset_meta(base: Option<Value>, extra: Value) -> Value {
    let Some(mut base) = base else {
        return extra;
    };
    let obj = base.as_object_mut();
    let Some(obj) = obj else {
        return extra;
    };
    if let Some(ex) = extra.as_object() {
        for (k, v) in ex {
            let empty = match obj.get(k) {
                None => true,
                Some(cur) => cur.is_null(),
            };
            if empty && !v.is_null() {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    base
}
