//! Durable DRep metadata cache (CIP-119 givenName).
//!
//! Loads `dreps.json` from DATA_DIR when present. On first boot (or when
//! `DREP_CACHE_REFRESH` is set) scrapes Blockfrost `GET /governance/dreps`
//! which embeds CIP-119 metadata (`metadata.json_metadata.body.givenName`).
//! While the process is running, a daily UTC-midnight job re-scrapes and
//! overwrites the cache. Misses are filled by `GET /governance/dreps/{id}/metadata`,
//! registration anchor URLs, and live `/api/drep` lookups.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const CACHE_FILE: &str = "dreps.json";
const PAGE: usize = 100;
const META_CONCURRENCY: usize = 16;
const ANCHOR_CONCURRENCY: usize = 12;

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

    /// Replace the in-memory + on-disk cache with a fresh Blockfrost scrape.
    pub async fn refresh(
        &self,
        http: &reqwest::Client,
        blockfrost_url: &str,
        project_id: Option<&str>,
    ) -> Result<usize> {
        let scrape_http = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| http.clone());
        let map = scrape_dreps(&scrape_http, blockfrost_url, project_id).await?;
        let n = map.len();
        if n == 0 {
            return Ok(0);
        }
        {
            let mut guard = self.by_id.lock().unwrap();
            *guard = map;
            if let Some(path) = &self.path {
                if let Err(e) = save_cache(path, &guard) {
                    tracing::warn!("could not write drep cache: {e:#}");
                }
            }
        }
        Ok(n)
    }

    /// Load `dreps.json` if present; otherwise scrape Blockfrost once.
    pub async fn load(
        http: &reqwest::Client,
        cache_dir: Option<&Path>,
        blockfrost_url: Option<&str>,
        project_id: Option<&str>,
        force_refresh: bool,
    ) -> Self {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let path = dir.join(CACHE_FILE);

        if !force_refresh {
            match load_cache(&path) {
                Ok(map) if !map.is_empty() => {
                    tracing::info!(
                        "drep cache: loaded {} entries from {}",
                        map.len(),
                        path.display()
                    );
                    return DrepCache {
                        by_id: Mutex::new(map),
                        path: Some(path),
                    };
                }
                Ok(_) => tracing::info!(
                    "drep cache: empty at {} - will scrape / fill on demand",
                    path.display()
                ),
                Err(_) if path.exists() => {
                    tracing::warn!("drep cache unreadable - will scrape / fill on demand")
                }
                Err(_) => tracing::info!(
                    "drep cache: no file at {} - will scrape / fill on demand",
                    path.display()
                ),
            }
        } else {
            tracing::info!("drep cache: DREP_CACHE_REFRESH set - re-scraping");
        }

        let _ = fs::create_dir_all(&dir);

        if let Some(bf_base) = blockfrost_url {
            let scrape_http = reqwest::Client::builder()
                .timeout(Duration::from_secs(45))
                .connect_timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_else(|_| http.clone());
            match scrape_dreps(&scrape_http, bf_base, project_id).await {
                Ok(map) if !map.is_empty() => {
                    tracing::info!("drep cache: scraped {} dreps with names", map.len());
                    if let Err(e) = save_cache(&path, &map) {
                        tracing::warn!("could not write drep cache: {e:#}");
                    } else {
                        tracing::info!("drep cache: wrote durable cache at {}", path.display());
                    }
                    return DrepCache {
                        by_id: Mutex::new(map),
                        path: Some(path),
                    };
                }
                Ok(_) => tracing::warn!("drep cache: Blockfrost scrape returned no names"),
                Err(e) => tracing::warn!("drep cache: Blockfrost scrape failed ({e:#})"),
            }
        } else {
            tracing::warn!("drep cache: no BLOCKFROST_URL - leaving cache empty for lazy fill");
        }

        DrepCache {
            by_id: Mutex::new(HashMap::new()),
            path: Some(path),
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
    let text = serde_json::to_string(map)?;
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

async fn scrape_dreps(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, DrepEntry>> {
    // Primary: GET /governance/dreps with embedded metadata.json_metadata.
    match scrape_list_embedded(http, base, project_id).await {
        Ok(m) if !m.is_empty() => {
            tracing::info!("drep cache: used embedded metadata on /governance/dreps");
            return Ok(m);
        }
        Ok(_) => tracing::warn!("drep cache: list had no names - fetching /metadata per id"),
        Err(e) => tracing::warn!("drep cache: list scrape failed ({e:#}) - trying /metadata"),
    }
    scrape_list_and_metadata(http, base, project_id).await
}

/// Page `GET /governance/dreps` and read `metadata.json_metadata.body.givenName`.
async fn scrape_list_embedded(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, DrepEntry>> {
    let mut map = HashMap::with_capacity(4_096);
    let mut page = 1u32;
    let mut saw_metadata_field = false;
    loop {
        let path = format!("/governance/dreps?count={PAGE}&page={page}");
        let rows: Value = bf_get(http, base, &path, project_id)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .with_context(|| format!("GET {base}{path}"))?
            .error_for_status()
            .with_context(|| format!("governance/dreps HTTP error page {page}"))?
            .json()
            .await
            .context("decode governance/dreps")?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        for row in arr {
            let Some(id) = row.get("drep_id").and_then(Value::as_str) else {
                continue;
            };
            // Field present (even when null) means this Blockfrost build embeds metadata.
            if row.as_object().is_some_and(|o| o.contains_key("metadata")) {
                saw_metadata_field = true;
            }
            if let Some(meta) = row.get("metadata") {
                if let Some(entry) = DrepEntry::from_blockfrost_meta(meta) {
                    map.insert(id.to_string(), entry);
                }
            }
        }
        if arr.len() < PAGE {
            break;
        }
        page += 1;
        if page % 10 == 0 {
            tracing::info!("drep cache: list scrape - {} names so far…", map.len());
        }
        // Older Blockfrost without a `metadata` field — fall through to per-id fetch.
        if page == 2 && !saw_metadata_field {
            break;
        }
    }
    Ok(map)
}

async fn scrape_list_and_metadata(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, DrepEntry>> {
    let ids = list_drep_ids(http, base, project_id).await?;
    if ids.is_empty() {
        return Err(anyhow!("Blockfrost /governance/dreps returned no drep ids"));
    }
    tracing::info!("drep cache: listed {} dreps - fetching metadata…", ids.len());

    let mut map = HashMap::with_capacity(ids.len());
    let mut set = tokio::task::JoinSet::new();
    let mut outstanding = 0usize;
    let mut done = 0usize;

    for id in ids {
        while outstanding >= META_CONCURRENCY {
            let Some(res) = set.join_next().await else { break };
            outstanding -= 1;
            done += 1;
            if let Ok(Some((did, entry))) = res {
                map.insert(did, entry);
            }
            if done % 200 == 0 {
                tracing::info!("drep cache: metadata {} done, {} names…", done, map.len());
            }
        }
        let http = http.clone();
        let base = base.to_string();
        let pid_header = project_id.map(str::to_string);
        set.spawn(async move {
            let path = format!("/governance/dreps/{id}/metadata");
            let mut req = http.get(format!("{base}{path}"));
            if let Some(pid) = pid_header {
                req = req.header("project_id", pid);
            }
            let v: Value = tokio::time::timeout(Duration::from_secs(8), req.send())
                .await
                .ok()?
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()?;
            let entry = DrepEntry::from_blockfrost_meta(&v)?;
            Some((id, entry))
        });
        outstanding += 1;
    }

    while let Some(res) = set.join_next().await {
        done += 1;
        if let Ok(Some((did, entry))) = res {
            map.insert(did, entry);
        }
        if done % 200 == 0 {
            tracing::info!("drep cache: metadata {} done, {} names…", done, map.len());
        }
    }

    Ok(map)
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
        let rows: Value = bf_get(http, base, &path, project_id)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .with_context(|| format!("GET {base}{path}"))?
            .error_for_status()
            .with_context(|| format!("governance/dreps HTTP error page {page}"))?
            .json()
            .await
            .context("decode governance/dreps")?;
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
        if arr.len() < PAGE {
            break;
        }
        page += 1;
        if page % 20 == 0 {
            tracing::info!("drep cache: listed {} drep ids so far…", ids.len());
        }
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
