//! Metadata enrichment: in-memory Cardano token registry (CIP-26), stake pool
//! ticker cache, and DRep name cache loaded from disk at boot. Pool/DRep
//! Blockfrost scrapes run in the background when caches are empty (or refresh
//! flags are set); live lookups only run for assets/pools/dreps missing from
//! those durable caches. Pool and DRep caches are also re-scraped daily at
//! 00:00 UTC. Governance action titles (CIP-108) are fetched once on first
//! sight and stored in `gov-actions.json`.

use crate::config::Config;
use crate::dreps::{self, DrepCache, DrepEntry};
use crate::gov_actions::{self, GovActionCache};
use crate::pools::{PoolCache, PoolEntry};
use crate::registry::TokenRegistry;
use crate::trending::KeywordMeta;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Enricher {
    http: reqwest::Client,
    blockfrost_url: Option<String>,
    project_id: Option<String>,
    ogmios_url: String,
    registry_url: String,
    /// Full CIP-26 registry - subject → name/ticker/decimals.
    registry: TokenRegistry,
    /// Durable pool ticker cache - pool1… → ticker/name.
    pool_cache: PoolCache,
    /// Durable DRep name cache - drep1… → CIP-119 givenName.
    drep_cache: DrepCache,
    /// Durable gov-action title cache - `{tx}#{index}` → CIP-108 title.
    gov_action_cache: GovActionCache,
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

        // Disk only — Blockfrost scrapes run in a background task from main.
        let pool_cache = PoolCache::load(cache_dir);
        let drep_cache = DrepCache::load(cache_dir);
        let gov_action_cache = GovActionCache::load(cache_dir);

        Enricher {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("http client"),
            blockfrost_url: config.blockfrost_url.clone(),
            project_id: config.blockfrost_project_id.clone(),
            ogmios_url: config.ogmios_url.clone(),
            registry_url: config.token_registry_url.clone(),
            registry,
            pool_cache,
            drep_cache,
            gov_action_cache,
            assets: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry_len(&self) -> usize {
        self.registry.len()
    }

    pub fn pool_cache_len(&self) -> usize {
        self.pool_cache.len()
    }

    pub fn drep_cache_len(&self) -> usize {
        self.drep_cache.len()
    }

    pub fn gov_action_cache_len(&self) -> usize {
        self.gov_action_cache.titled_len()
    }

    /// Run pool/DRep Blockfrost scrapes when the on-disk cache was empty (or a
    /// refresh flag is set). Intended to be `tokio::spawn`ed so boot is not blocked.
    /// Returns whether the DRep cache gained entries (caller may re-stamp events).
    pub async fn run_initial_scrapes(&self, force_pools: bool, force_dreps: bool) -> bool {
        if self.blockfrost_url.is_none() {
            tracing::info!("pool/drep initial scrape skipped (no BLOCKFROST_URL)");
            return false;
        }
        let need_pools = force_pools || self.pool_cache.len() == 0;
        let need_dreps = force_dreps || self.drep_cache.len() == 0;
        if !need_pools && !need_dreps {
            return false;
        }
        if force_pools {
            tracing::info!("pool cache: POOL_CACHE_REFRESH set - background re-scrape");
        } else if need_pools {
            tracing::info!("pool cache: empty - background scrape starting");
        }
        if force_dreps {
            tracing::info!("drep cache: DREP_CACHE_REFRESH set - background re-scrape");
        } else if need_dreps {
            tracing::info!("drep cache: empty - background scrape starting");
        }

        let pool_fut = async {
            if !need_pools {
                return;
            }
            let Some(base) = self.blockfrost_url.as_deref() else {
                return;
            };
            let pid = self.project_id.as_deref();
            match self.pool_cache.refresh(&self.http, base, pid).await {
                Ok(0) => tracing::warn!("pool cache initial scrape returned 0 entries"),
                Ok(n) => tracing::info!("pool cache initial scrape done ({n} pools)"),
                Err(e) => tracing::warn!("pool cache initial scrape failed: {e:#}"),
            }
        };
        let drep_fut = async {
            if !need_dreps {
                return false;
            }
            let Some(base) = self.blockfrost_url.as_deref() else {
                return false;
            };
            let pid = self.project_id.as_deref();
            match self.drep_cache.refresh(&self.http, base, pid).await {
                Ok(0) => {
                    tracing::warn!("drep cache initial scrape returned 0 names");
                    false
                }
                Ok(n) => {
                    tracing::info!("drep cache initial scrape done ({n} dreps)");
                    true
                }
                Err(e) => {
                    tracing::warn!("drep cache initial scrape failed: {e:#}");
                    false
                }
            }
        };
        let (_, dreps_ready) = tokio::join!(pool_fut, drep_fut);
        dreps_ready
    }

    /// Re-scrape pool + DRep registration metadata from Blockfrost every day
    /// at 00:00 UTC so ticker/name changes land without a restart.
    pub async fn refresh_meta_caches_loop(self: Arc<Self>) {
        if self.blockfrost_url.is_none() {
            tracing::info!("pool/drep daily refresh skipped (no BLOCKFROST_URL)");
            return;
        }
        loop {
            let wait = duration_until_next_utc_midnight();
            tracing::info!(
                "next pool/drep cache refresh in ~{} (00:00 UTC)",
                humantime::format_duration(wait)
            );
            tokio::time::sleep(wait).await;
            self.refresh_pool_and_drep_caches().await;
            // Stay past midnight so the next wait targets tomorrow.
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn refresh_pool_and_drep_caches(&self) {
        let Some(base) = self.blockfrost_url.as_deref() else {
            return;
        };
        let pid = self.project_id.as_deref();
        tracing::info!("refreshing pool and drep caches from Blockfrost…");
        match self.pool_cache.refresh(&self.http, base, pid).await {
            Ok(0) => tracing::warn!("pool cache refresh returned 0 entries - keeping previous"),
            Ok(n) => tracing::info!("pool cache refreshed ({n} pools)"),
            Err(e) => tracing::warn!("pool cache refresh failed: {e:#}"),
        }
        match self.drep_cache.refresh(&self.http, base, pid).await {
            Ok(0) => tracing::warn!("drep cache refresh returned 0 names - keeping previous"),
            Ok(n) => tracing::info!("drep cache refreshed ({n} dreps)"),
            Err(e) => tracing::warn!("drep cache refresh failed: {e:#}"),
        }
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

    /// Stamp CIP-119 givenNames onto DRep id fields when the durable cache has them.
    pub fn stamp_event_dreps(&self, event: &mut crate::model::ChainEvent) {
        let Some(obj) = event.data.as_object_mut() else {
            return;
        };
        for (id_key, name_key) in [
            ("drep", "drepName"),
            ("fromDrep", "fromDrepName"),
            ("voter", "voterName"),
        ] {
            let Some(id) = obj.get(id_key).and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            if !dreps::is_lookup_drep_id(&id) {
                continue;
            }
            if let Some(name) = self
                .drep_cache
                .get(&id)
                .and_then(|e| e.name)
                .filter(|s| !s.is_empty())
            {
                obj.insert(name_key.into(), json!(name));
            }
        }
    }

    /// Stamp CIP-108 titles onto gov proposal / vote events when cached.
    pub fn stamp_event_gov_actions(&self, event: &mut crate::model::ChainEvent) {
        let (tx, index) = match event.kind.as_str() {
            "gov_proposal" => {
                let Some(tx) = event.tx_hash.clone() else {
                    return;
                };
                let index = event
                    .data
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                (tx, index)
            }
            "gov_vote" => {
                let Some(tx) = event
                    .data
                    .get("proposalTx")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    return;
                };
                let index = event
                    .data
                    .get("proposalIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                (tx, index)
            }
            _ => return,
        };
        let key = gov_actions::cache_key(&tx, index);
        let Some(title) = self.gov_action_cache.title(&key) else {
            return;
        };
        if let Some(obj) = event.data.as_object_mut() {
            obj.insert("proposalTitle".into(), json!(title));
        }
    }

    /// Resolve titles for any new gov actions referenced by these events
    /// (Blockfrost once per action; later hits are cache-only).
    pub async fn ensure_gov_action_titles(&self, events: &[crate::model::ChainEvent]) {
        let refs = gov_actions::collect_refs(events, &self.gov_action_cache);
        if refs.is_empty() {
            return;
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| self.http.clone());
        gov_actions::ensure_titles(
            &http,
            self.blockfrost_url.as_deref(),
            self.project_id.as_deref(),
            Some(self.ogmios_url.as_str()),
            &self.gov_action_cache,
            refs,
        )
        .await;
    }

    /// Compact drep-id → name map for the browser (loaded once per page).
    pub fn dreps_json(&self) -> Value {
        self.drep_cache.to_names_json()
    }

    /// Compact `{tx}#{index}` → title map for the browser.
    pub fn gov_actions_json(&self) -> Value {
        self.gov_action_cache.to_titles_json()
    }

    /// Fill the durable cache from registration/update anchor URLs in events.
    pub async fn warm_dreps_from_events(&self, events: &[crate::model::ChainEvent]) {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| self.http.clone());
        dreps::warm_from_events(&http, &self.drep_cache, events).await;
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

    /// DRep CIP-119 givenName (and optional image / metadata url).
    pub async fn drep(&self, drep_id: &str) -> Value {
        let id = drep_id.trim();
        if !dreps::is_lookup_drep_id(id) {
            return json!({ "error": "bad drep id" });
        }
        if let Some(hit) = self.drep_cache.get(id) {
            return hit.to_json(id);
        }
        let meta = self
            .drep_from_blockfrost(id)
            .await
            .unwrap_or_else(|| json!({ "drep": id }));
        let entry = DrepEntry {
            name: meta
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            url: meta
                .get("url")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            image: meta
                .get("image")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        self.drep_cache.remember(id, entry);
        meta
    }

    /// CIP-108 governance action title for `{tx_hash}` + cert index.
    pub async fn gov_action(&self, tx_hash: &str, index: u64) -> Value {
        let tx = tx_hash.trim().to_lowercase();
        if tx.len() != 64 || !tx.chars().all(|c| c.is_ascii_hexdigit()) {
            return json!({ "error": "bad tx hash" });
        }
        let key = gov_actions::cache_key(&tx, index);
        if let Some(hit) = self.gov_action_cache.get(&key) {
            return hit.to_json(&tx, index);
        }
        self.ensure_gov_action_titles(&[crate::model::ChainEvent {
            id: 0,
            kind: "gov_vote".into(),
            category: "governance".into(),
            slot: 0,
            height: None,
            block_hash: None,
            tx_hash: None,
            timestamp: 0,
            title: String::new(),
            summary: String::new(),
            data: json!({ "proposalTx": tx, "proposalIndex": index }),
        }])
        .await;
        self.gov_action_cache
            .get(&key)
            .unwrap_or_default()
            .to_json(&tx, index)
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

    async fn drep_from_blockfrost(&self, drep_id: &str) -> Option<Value> {
        let req = self.bf(&format!("/governance/dreps/{drep_id}/metadata"))?;
        let v: Value = tokio::time::timeout(Duration::from_secs(8), req.send())
            .await
            .ok()?
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        if let Some(entry) = DrepEntry::from_blockfrost_meta(&v) {
            return Some(entry.to_json(drep_id));
        }
        // If BF only returned the anchor URL, fetch CIP-119 ourselves.
        let url = v.get("url").and_then(Value::as_str)?;
        let entry = dreps::fetch_anchor_entry(&self.http, url).await?;
        Some(entry.to_json(drep_id))
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

fn duration_until_next_utc_midnight() -> Duration {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_into_day = now % 86_400;
    let wait = if secs_into_day == 0 {
        86_400
    } else {
        86_400 - secs_into_day
    };
    Duration::from_secs(wait)
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
