//! Durable DRep metadata cache (CIP-119 givenName).
//!
//! Loads `dreps.json` from DATA_DIR when present (boot stays non-blocking).
//! Missing / forced refreshes scrape Blockfrost `GET /governance/dreps` in the
//! background (Blockfrost only — no third-party indexers). Pages are fetched
//! with retries and **no request timeout** so a slow RYO cannot abort the
//! scrape; names are merged into the in-memory map (and disk) as each page
//! lands. A daily UTC-midnight job re-scrapes the same way. Misses are also
//! filled by `GET /governance/dreps/{id}/metadata`, registration anchors, and
//! live `/api/drep` lookups.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const CACHE_FILE: &str = "dreps.json";
/// Smaller pages keep each `/governance/dreps` call cheaper on slow RYO builds.
const PAGE: usize = 20;
const META_CONCURRENCY: usize = 4;
const ANCHOR_CONCURRENCY: usize = 12;
const DREP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Cap between page retries so a wedged RYO is retried, not abandoned.
const DREP_RETRY_BACKOFF_MAX: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrepEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl DrepEntry {
    pub fn has_label(&self) -> bool {
        self.name.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn to_json(&self, drep_id: &str) -> Value {
        json!({
            "drep": drep_id,
            "name": self.name,
            "url": self.url,
            "image": self.image,
            "source": "cache",
        })
    }

    /// Extract CIP-119 fields from a Blockfrost metadata object
    /// (`/governance/dreps` embedded `metadata`, or `/dreps/{id}/metadata`).
    pub fn from_blockfrost_meta(v: &Value) -> Option<Self> {
        if v.is_null() {
            return None;
        }
        if v.get("error").is_some() && v.get("json_metadata").is_none() {
            return None;
        }
        let meta = v.get("json_metadata").unwrap_or(v);
        let mut entry = Self::from_cip119(meta)?;
        if entry.url.is_none() {
            entry.url = v
                .get("url")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        Some(entry)
    }

    /// Parse a CIP-119 (or raw CIP-100 body) JSON document.
    pub fn from_cip119(meta: &Value) -> Option<Self> {
        let body = meta.get("body").unwrap_or(meta);
        let name = body
            .get("givenName")
            .or_else(|| body.get("given_name"))
            .or_else(|| body.get("dRepName"))
            .or_else(|| meta.get("givenName"))
            .and_then(text_field)
            .map(|s| s.chars().take(80).collect::<String>());
        let image = body
            .get("image")
            .or_else(|| meta.get("image"))
            .and_then(image_url);
        let url = meta
            .get("url")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let entry = DrepEntry { name, url, image };
        entry.has_label().then_some(entry)
    }
}

pub struct DrepCache {
    by_id: Mutex<HashMap<String, DrepEntry>>,
    path: Option<PathBuf>,
}

impl DrepCache {
    pub fn len(&self) -> usize {
        self.by_id.lock().unwrap().len()
    }

    pub fn get(&self, drep_id: &str) -> Option<DrepEntry> {
        self.by_id.lock().unwrap().get(drep_id).cloned()
    }

    /// Compact dump for the browser (id → name).
    pub fn to_names_json(&self) -> Value {
        let map = self.by_id.lock().unwrap();
        let mut out = serde_json::Map::new();
        for (id, e) in map.iter() {
            if let Some(name) = e.name.as_ref().filter(|s| !s.is_empty()) {
                out.insert(id.clone(), json!({ "name": name }));
            }
        }
        Value::Object(out)
    }

    /// Insert (or replace) and rewrite the on-disk cache when we learned a name.
    pub fn remember(&self, drep_id: &str, entry: DrepEntry) {
        if !entry.has_label() {
            return;
        }
        let mut map = self.by_id.lock().unwrap();
        let changed = match map.get(drep_id) {
            Some(old) => old.name != entry.name,
            None => true,
        };
        if !changed {
            return;
        }
        map.insert(drep_id.to_string(), entry);
        if let Some(path) = &self.path {
            let _ = save_cache(path, &map);
        }
    }

    /// Merge many entries and persist once.
    pub fn remember_many(&self, entries: HashMap<String, DrepEntry>) {
        if entries.is_empty() {
            return;
        }
        let mut map = self.by_id.lock().unwrap();
        let mut changed = false;
        for (id, entry) in entries {
            if !entry.has_label() {
                continue;
            }
            let is_new = match map.get(&id) {
                Some(old) => old.name != entry.name,
                None => true,
            };
            if is_new {
                map.insert(id, entry);
                changed = true;
            }
        }
        if changed {
            if let Some(path) = &self.path {
                let _ = save_cache(path, &map);
            }
        }
    }

    /// Replace/merge the in-memory + on-disk cache with a Blockfrost scrape.
    ///
    /// Uses a client with **no request timeout** and retries each page until it
    /// succeeds. Names are merged into the live map as pages arrive so the app
    /// can use them before the full scrape completes.
    pub async fn refresh(
        &self,
        http: &reqwest::Client,
        blockfrost_url: &str,
        project_id: Option<&str>,
    ) -> Result<usize> {
        let scrape_http = reqwest::Client::builder()
            .connect_timeout(DREP_CONNECT_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| http.clone());
        tracing::info!(
            "drep cache: scraping Blockfrost /governance/dreps (count={PAGE}, no request timeout, retry forever)…"
        );
        let before = self.len();
        scrape_dreps(self, &scrape_http, blockfrost_url, project_id).await?;
        let n = self.len();
        if n == 0 {
            return Ok(0);
        }
        if n > before {
            tracing::info!("drep cache: scrape finished ({n} names, was {before})");
        } else {
            tracing::info!("drep cache: scrape finished ({n} names)");
        }
        Ok(n)
    }

    /// Load `dreps.json` from disk only — never blocks on Blockfrost.
    /// Background scrapes are started by [`crate::enrich::Enricher`].
    pub fn load(cache_dir: Option<&Path>) -> Self {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let path = dir.join(CACHE_FILE);
        let _ = fs::create_dir_all(&dir);

        match load_cache(&path) {
            Ok(map) if !map.is_empty() => {
                tracing::info!(
                    "drep cache: loaded {} entries from {}",
                    map.len(),
                    path.display()
                );
                DrepCache {
                    by_id: Mutex::new(map),
                    path: Some(path),
                }
            }
            Ok(_) => {
                tracing::info!(
                    "drep cache: empty at {} - background scrape / on-demand fill",
                    path.display()
                );
                DrepCache {
                    by_id: Mutex::new(HashMap::new()),
                    path: Some(path),
                }
            }
            Err(_) if path.exists() => {
                tracing::warn!("drep cache unreadable - background scrape / on-demand fill");
                DrepCache {
                    by_id: Mutex::new(HashMap::new()),
                    path: Some(path),
                }
            }
            Err(_) => {
                tracing::info!(
                    "drep cache: no file at {} - background scrape / on-demand fill",
                    path.display()
                );
                DrepCache {
                    by_id: Mutex::new(HashMap::new()),
                    path: Some(path),
                }
            }
        }
    }
}

fn load_cache(path: &Path) -> Result<HashMap<String, DrepEntry>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).context("parse drep cache")
}

fn save_cache(path: &Path, map: &HashMap<String, DrepEntry>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(map)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// True for bech32 DRep ids we can look up (not the special always-* labels).
pub fn is_lookup_drep_id(id: &str) -> bool {
    let id = id.trim();
    (id.starts_with("drep1") || id.starts_with("drep_script1")) && id.len() >= 50 && id.len() <= 120
}

fn text_field(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        let t = s.trim();
        return (!t.is_empty()).then(|| t.to_string());
    }
    if let Some(s) = v.get("@value").and_then(Value::as_str) {
        let t = s.trim();
        return (!t.is_empty()).then(|| t.to_string());
    }
    if let Some(obj) = v.as_object() {
        for (_k, val) in obj {
            if let Some(s) = val.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            if let Some(s) = val.get("@value").and_then(Value::as_str) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn image_url(img: &Value) -> Option<String> {
    img.as_str()
        .or_else(|| img.get("contentUrl").and_then(Value::as_str))
        .or_else(|| img.get("url").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn bf_get(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    project_id: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = http.get(format!("{base}{path}"));
    if let Some(pid) = project_id {
        req = req.header("project_id", pid);
    }
    req
}

fn retry_backoff(attempt: u32) -> Duration {
    // 1s, 2s, 4s, … capped.
    let secs = (1u64 << attempt.min(5)).min(DREP_RETRY_BACKOFF_MAX.as_secs());
    Duration::from_secs(secs)
}

/// GET JSON from Blockfrost with unlimited retries and no request timeout.
/// Used for list pages that must eventually succeed.
async fn bf_get_json_retry(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    project_id: Option<&str>,
    label: &str,
) -> Result<Value> {
    let mut attempt = 0u32;
    loop {
        match bf_get(http, base, path, project_id).send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(ok) => match ok.json::<Value>().await {
                    Ok(v) => return Ok(v),
                    Err(e) => {
                        tracing::warn!(
                            "drep cache: {label} decode failed (attempt {}): {e:#} - retrying",
                            attempt + 1
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "drep cache: {label} HTTP error (attempt {}): {e:#} - retrying",
                        attempt + 1
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    "drep cache: {label} request failed (attempt {}): {e:#} - retrying",
                    attempt + 1
                );
            }
        }
        tokio::time::sleep(retry_backoff(attempt)).await;
        attempt = attempt.saturating_add(1);
    }
}

/// Fetch one DRep's `/metadata`. Retries transport/5xx forever; 404/400 = no name.
async fn fetch_drep_metadata_retry(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    project_id: Option<&str>,
) -> Option<DrepEntry> {
    let mut attempt = 0u32;
    loop {
        match bf_get(http, base, path, project_id).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 404 || status.as_u16() == 400 {
                    return None;
                }
                if !status.is_success() {
                    tracing::warn!(
                        "drep cache: GET {path} HTTP {status} (attempt {}) - retrying",
                        attempt + 1
                    );
                    tokio::time::sleep(retry_backoff(attempt)).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                match resp.json::<Value>().await {
                    Ok(v) => return DrepEntry::from_blockfrost_meta(&v),
                    Err(e) => {
                        tracing::warn!(
                            "drep cache: GET {path} decode failed (attempt {}): {e:#} - retrying",
                            attempt + 1
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "drep cache: GET {path} failed (attempt {}): {e:#} - retrying",
                    attempt + 1
                );
            }
        }
        tokio::time::sleep(retry_backoff(attempt)).await;
        attempt = attempt.saturating_add(1);
    }
}

async fn scrape_dreps(
    cache: &DrepCache,
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<()> {
    // Primary: paginate GET /governance/dreps and read embedded metadata.
    match scrape_list_embedded(cache, http, base, project_id).await {
        Ok(true) => {
            tracing::info!("drep cache: used embedded metadata on /governance/dreps");
            return Ok(());
        }
        Ok(false) => {
            tracing::warn!("drep cache: list had no embedded names - fetching /metadata per id")
        }
        Err(e) => tracing::warn!("drep cache: list scrape failed ({e:#}) - trying /metadata"),
    }
    scrape_list_and_metadata(cache, http, base, project_id).await
}

/// Page `GET /governance/dreps` and merge `metadata` names into `cache`.
///
/// Returns `Ok(true)` when the endpoint embeds a `metadata` field (even if some
/// rows lack names). Returns `Ok(false)` when the build has no `metadata` field
/// so the caller should fall back to per-id `/metadata`.
async fn scrape_list_embedded(
    cache: &DrepCache,
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<bool> {
    let mut page = 1u32;
    let mut saw_metadata_field = false;
    let mut named = 0usize;
    loop {
        let path = format!("/governance/dreps?count={PAGE}&page={page}");
        let label = format!("GET {path}");
        let rows = bf_get_json_retry(http, base, &path, project_id, &label).await?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        let mut batch = HashMap::new();
        for row in arr {
            let Some(id) = row.get("drep_id").and_then(Value::as_str) else {
                continue;
            };
            if row.as_object().is_some_and(|o| o.contains_key("metadata")) {
                saw_metadata_field = true;
            }
            if let Some(meta) = row.get("metadata") {
                if let Some(entry) = DrepEntry::from_blockfrost_meta(meta) {
                    batch.insert(id.to_string(), entry);
                }
            }
        }
        named += batch.len();
        cache.remember_many(batch);
        tracing::info!(
            "drep cache: list page {page} ({} rows, {named} names so far)",
            arr.len()
        );
        if arr.len() < PAGE {
            break;
        }
        page += 1;
        // Older Blockfrost without a `metadata` field — fall through to per-id fetch.
        if page == 2 && !saw_metadata_field {
            return Ok(false);
        }
    }
    Ok(saw_metadata_field || named > 0)
}

async fn scrape_list_and_metadata(
    cache: &DrepCache,
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<()> {
    let ids = list_drep_ids(http, base, project_id).await?;
    if ids.is_empty() {
        return Err(anyhow!("Blockfrost /governance/dreps returned no drep ids"));
    }
    tracing::info!(
        "drep cache: listed {} dreps - fetching /metadata (concurrency {META_CONCURRENCY})…",
        ids.len()
    );

    let mut set: tokio::task::JoinSet<Option<(String, DrepEntry)>> =
        tokio::task::JoinSet::new();
    let mut outstanding = 0usize;
    let mut done = 0usize;
    let mut named = 0usize;

    for id in ids {
        while outstanding >= META_CONCURRENCY {
            let Some(res) = set.join_next().await else {
                break;
            };
            outstanding -= 1;
            done += 1;
            if let Ok(Some((did, entry))) = res {
                named += 1;
                cache.remember(&did, entry);
            }
            if done % 50 == 0 {
                tracing::info!("drep cache: metadata {done} done, {named} names…");
            }
        }
        let http = http.clone();
        let base = base.to_string();
        let pid = project_id.map(str::to_string);
        set.spawn(async move {
            let path = format!("/governance/dreps/{id}/metadata");
            fetch_drep_metadata_retry(&http, &base, &path, pid.as_deref())
                .await
                .map(|entry| (id, entry))
        });
        outstanding += 1;
    }

    while let Some(res) = set.join_next().await {
        done += 1;
        if let Ok(Some((did, entry))) = res {
            named += 1;
            cache.remember(&did, entry);
        }
        if done % 50 == 0 {
            tracing::info!("drep cache: metadata {done} done, {named} names…");
        }
    }

    Ok(())
}

async fn list_drep_ids(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(4_096);
    let mut page = 1u32;
    loop {
        let path = format!("/governance/dreps?count={PAGE}&page={page}");
        let label = format!("GET {path}");
        let rows = bf_get_json_retry(http, base, &path, project_id, &label).await?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        for row in arr {
            if let Some(id) = row.get("drep_id").and_then(Value::as_str) {
                ids.push(id.to_string());
            } else if let Some(id) = row.as_str() {
                ids.push(id.to_string());
            }
        }
        tracing::info!("drep cache: listed {} drep ids (page {page})", ids.len());
        if arr.len() < PAGE {
            break;
        }
        page += 1;
    }
    Ok(ids)
}

/// Resolve `ipfs://…` / `ipfs/…` to an HTTPS gateway URL.
pub fn resolve_anchor_url(url: &str) -> Option<String> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    if let Some(rest) = u.strip_prefix("ipfs://") {
        let path = rest.trim_start_matches("ipfs/");
        return Some(format!("https://ipfs.io/ipfs/{path}"));
    }
    if u.starts_with("http://") || u.starts_with("https://") {
        return Some(u.to_string());
    }
    None
}

/// Fetch CIP-119 JSON from a registration/update anchor URL.
pub async fn fetch_anchor_entry(http: &reqwest::Client, url: &str) -> Option<DrepEntry> {
    let resolved = resolve_anchor_url(url)?;
    let v: Value = tokio::time::timeout(Duration::from_secs(8), http.get(&resolved).send())
        .await
        .ok()?
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let mut entry = DrepEntry::from_cip119(&v)?;
    if entry.url.is_none() {
        entry.url = Some(url.to_string());
    }
    Some(entry)
}

/// Warm misses from registration/update anchor URLs in the event window.
pub async fn warm_from_events(
    http: &reqwest::Client,
    cache: &DrepCache,
    events: &[crate::model::ChainEvent],
) {
    let mut jobs: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    for ev in events {
        if !matches!(ev.kind.as_str(), "drep_registration" | "drep_update") {
            continue;
        }
        let d = &ev.data;
        let Some(id) = d.get("drep").and_then(Value::as_str).filter(|s| is_lookup_drep_id(s))
        else {
            continue;
        };
        if cache.get(id).is_some_and(|e| e.has_label()) {
            continue;
        }
        let Some(url) = d
            .get("anchorUrl")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        jobs.push((id.to_string(), url.to_string()));
    }
    if jobs.is_empty() {
        return;
    }
    tracing::info!(
        "drep cache: warming {} names from registration anchors…",
        jobs.len()
    );

    let mut set = tokio::task::JoinSet::new();
    let mut outstanding = 0usize;
    let mut got = HashMap::new();

    for (id, url) in jobs {
        while outstanding >= ANCHOR_CONCURRENCY {
            let Some(res) = set.join_next().await else { break };
            outstanding -= 1;
            if let Ok(Some((did, entry))) = res {
                got.insert(did, entry);
            }
        }
        let http = http.clone();
        set.spawn(async move {
            let entry = fetch_anchor_entry(&http, &url).await?;
            Some((id, entry))
        });
        outstanding += 1;
    }
    while let Some(res) = set.join_next().await {
        if let Ok(Some((did, entry))) = res {
            got.insert(did, entry);
        }
    }

    let n = got.len();
    cache.remember_many(got);
    if n > 0 {
        tracing::info!("drep cache: learned {n} names from anchors");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn named_row(i: usize) -> Value {
        // Long enough to pass is_lookup_drep_id length checks if needed later.
        let id = format!("drep1test{:0>50}", i);
        json!({
            "drep_id": id,
            "metadata": {
                "json_metadata": {
                    "body": { "givenName": format!("DRep {i}") }
                }
            }
        })
    }

    fn page_body(start: usize, n: usize) -> Value {
        Value::Array((start..start + n).map(named_row).collect())
    }

    #[test]
    fn parses_embedded_blockfrost_metadata() {
        let v = json!({
            "url": "https://example.com/drep.json",
            "json_metadata": {
                "body": { "givenName": "Ada Lovelace" }
            }
        });
        let e = DrepEntry::from_blockfrost_meta(&v).expect("parse");
        assert_eq!(e.name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(e.url.as_deref(), Some("https://example.com/drep.json"));
    }

    #[tokio::test]
    async fn scrape_paginates_and_updates_memory() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("count", "20"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 20)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("count", "20"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(20, 20)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("count", "20"))
            .and(query_param("page", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(40, 5)))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        assert_eq!(cache.len(), 0);

        let http = reqwest::Client::new();
        let n = cache.refresh(&http, &server.uri(), None).await.unwrap();
        assert_eq!(n, 45);
        assert_eq!(cache.len(), 45);
        assert_eq!(
            cache
                .get(&format!("drep1test{:0>50}", 0))
                .and_then(|e| e.name),
            Some("DRep 0".into())
        );
        assert_eq!(
            cache
                .get(&format!("drep1test{:0>50}", 44))
                .and_then(|e| e.name),
            Some("DRep 44".into())
        );
        // Durable file written.
        assert!(dir.path().join(CACHE_FILE).exists());
    }

    #[tokio::test]
    async fn scrape_retries_until_page_succeeds() {
        let server = MockServer::start().await;

        // First two attempts for page 1 fail; third succeeds with a short page.
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 3)))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        let http = reqwest::Client::new();
        let n = cache.refresh(&http, &server.uri(), None).await.unwrap();
        assert_eq!(n, 3);
        assert_eq!(cache.len(), 3);
    }
}

