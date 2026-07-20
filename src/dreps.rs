//! Durable DRep metadata cache (CIP-119 givenName).
//!
//! Loads `dreps.json` from DATA_DIR when present (boot stays non-blocking).
//! Missing / forced refreshes scrape Blockfrost `GET /governance/dreps` in the
//! background (Blockfrost only — no third-party indexers). Prefer RYO's
//! undocumented `unpaged: true` header (one request, every active DRep,
//! metadata included; `retired=false&expired=false`); fall back to paginated
//! list + per-id `/metadata` when needed.
//! The scrape HTTP client has **no timeouts** (request/connect/idle) — it waits
//! until RYO returns or the process is killed; failed attempts retry forever.
//! A daily UTC-midnight job re-scrapes the same way. Misses are also filled by
//! `GET /governance/dreps/{id}/metadata`, registration anchors, and live
//! `/api/drep` lookups.

use anyhow::{anyhow, Context, Result};
use bech32::{Bech32, Hrp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

/// CIP-129 header: DRep (key-type 2) + key-hash credential (2).
const CIP129_DREP_KEY: u8 = 0x22;
/// CIP-129 header: DRep (key-type 2) + script-hash credential (3).
const CIP129_DREP_SCRIPT: u8 = 0x23;

const CACHE_FILE: &str = "dreps.json";
const PAGE: usize = 100;
/// Only currently registered, non-expired DReps (Blockfrost query filters).
const DREP_LIST_FILTERS: &str = "retired=false&expired=false";
const META_CONCURRENCY: usize = 4;
const ANCHOR_CONCURRENCY: usize = 12;
/// Cap between *failed-attempt* retries only (not an in-flight request deadline).
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
    /// Only one bulk scrape at a time (boot + daily refresh must not stack).
    scrape_lock: AsyncMutex<()>,
}

impl DrepCache {
    pub fn len(&self) -> usize {
        self.by_id.lock().unwrap().len()
    }

    pub fn get(&self, drep_id: &str) -> Option<DrepEntry> {
        let map = self.by_id.lock().unwrap();
        // CIP-105 legacy and CIP-129 forms share a credential hash but differ
        // as bech32 strings — try every alias so either key hits.
        for id in drep_id_aliases(drep_id) {
            if let Some(e) = map.get(&id) {
                return Some(e.clone());
            }
        }
        None
    }

    /// Compact dump for the browser (id → name).
    ///
    /// Emits both CIP-129 and legacy CIP-105 bech32 keys so the UI can resolve
    /// whichever form is on the event card.
    pub fn to_names_json(&self) -> Value {
        let map = self.by_id.lock().unwrap();
        let mut out = serde_json::Map::new();
        for (id, e) in map.iter() {
            let Some(name) = e.name.as_ref().filter(|s| !s.is_empty()) else {
                continue;
            };
            for alt in drep_id_aliases(id) {
                out.entry(alt)
                    .or_insert_with(|| json!({ "name": name }));
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
    /// Dedicated HTTP client with **no request, connect, or pool idle timeout** —
    /// an in-flight scrape waits until RYO responds or the process is killed.
    /// Failed attempts (connection reset, 5xx, decode) retry forever with backoff.
    /// Prefers RYO `unpaged: true` (single full list); falls back to pagination.
    /// Only one scrape runs at a time so we never pile concurrent list calls
    /// onto RYO.
    pub async fn refresh(
        &self,
        http: &reqwest::Client,
        blockfrost_url: &str,
        project_id: Option<&str>,
    ) -> Result<usize> {
        let Ok(_guard) = self.scrape_lock.try_lock() else {
            tracing::info!("drep cache: scrape already in progress - skipping");
            return Ok(self.len());
        };
        // Never reuse Enricher's short-timeout client. Omit timeout/connect_timeout
        // so reqwest waits indefinitely for headers + body.
        let scrape_http = reqwest::Client::builder()
            .pool_idle_timeout(None)
            // Keep NAT/middleboxes from silently dropping an hour-long download.
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!("drep cache: scrape client build failed ({e:#}) - using bare client");
                let _ = http;
                reqwest::Client::builder()
                    .build()
                    .expect("bare drep scrape http client")
            });
        tracing::info!(
            "drep cache: scraping Blockfrost /governance/dreps (unpaged preferred, no timeouts, retry forever)…"
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
                    scrape_lock: AsyncMutex::new(()),
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
                    scrape_lock: AsyncMutex::new(()),
                }
            }
            Err(_) if path.exists() => {
                tracing::warn!("drep cache unreadable - background scrape / on-demand fill");
                DrepCache {
                    by_id: Mutex::new(HashMap::new()),
                    path: Some(path),
                    scrape_lock: AsyncMutex::new(()),
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
                    scrape_lock: AsyncMutex::new(()),
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

/// Map Blockfrost / Ogmios spellings of the special DReps onto the UI labels
/// used by [`crate::parse::drep_display`] (`Always Abstain` / `Always No Confidence`).
pub fn normalize_drep_id(id: &str) -> String {
    match id.trim() {
        "drep_always_abstain"
        | "always_abstain"
        | "alwaysAbstain"
        | "abstain"
        | "Always Abstain" => "Always Abstain".into(),
        "drep_always_no_confidence"
        | "always_no_confidence"
        | "alwaysNoConfidence"
        | "noConfidence"
        | "Always No Confidence" => "Always No Confidence".into(),
        other => other.to_string(),
    }
}

/// Equality after special-label normalization (and CIP-105 ↔ CIP-129 aliases).
pub fn drep_ids_equal(a: &str, b: &str) -> bool {
    let a = normalize_drep_id(a);
    let b = normalize_drep_id(b);
    if a == b {
        return true;
    }
    let aliases_a = drep_id_aliases(&a);
    aliases_a.iter().any(|x| drep_id_aliases(&b).iter().any(|y| x == y))
}

/// CIP-129 bech32 for a 28-byte credential hash (`from` = verificationKey / script…).
pub fn drep_bech32_cip129(cred_hex: &str, from: Option<&str>) -> Option<String> {
    let bytes = hex::decode(cred_hex).ok()?;
    if bytes.len() != 28 {
        return None;
    }
    let header = match from {
        Some("scriptHash") | Some("script") => CIP129_DREP_SCRIPT,
        _ => CIP129_DREP_KEY,
    };
    let mut payload = Vec::with_capacity(29);
    payload.push(header);
    payload.extend_from_slice(&bytes);
    encode_drep_payload(&payload)
}

/// All bech32 spellings of the same DRep credential (self + CIP-105 ↔ CIP-129).
pub fn drep_id_aliases(id: &str) -> Vec<String> {
    let id = id.trim();
    let mut out = vec![id.to_string()];
    let Some(raw) = decode_drep_payload(id) else {
        return out;
    };
    let push = |out: &mut Vec<String>, s: String| {
        if !out.iter().any(|x| x == &s) {
            out.push(s);
        }
    };
    match raw.len() {
        28 => {
            // Legacy CIP-105 → CIP-129 (try both key and script headers).
            for header in [CIP129_DREP_KEY, CIP129_DREP_SCRIPT] {
                let mut payload = Vec::with_capacity(29);
                payload.push(header);
                payload.extend_from_slice(&raw);
                if let Some(b) = encode_drep_payload(&payload) {
                    push(&mut out, b);
                }
            }
        }
        29 if raw[0] == CIP129_DREP_KEY || raw[0] == CIP129_DREP_SCRIPT => {
            // CIP-129 → legacy CIP-105 (hash only).
            if let Some(b) = encode_drep_payload(&raw[1..]) {
                push(&mut out, b);
            }
        }
        _ => {}
    }
    out
}

fn decode_drep_payload(id: &str) -> Option<Vec<u8>> {
    let (hrp, data) = bech32::decode(id).ok()?;
    match hrp.as_str() {
        "drep" | "drep_script" => Some(data),
        _ => None,
    }
}

fn encode_drep_payload(payload: &[u8]) -> Option<String> {
    let hrp = Hrp::parse("drep").ok()?;
    bech32::encode::<Bech32>(hrp, payload).ok()
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
    unpaged: bool,
) -> reqwest::RequestBuilder {
    let mut req = http.get(format!("{base}{path}"));
    if let Some(pid) = project_id {
        req = req.header("project_id", pid);
    }
    // Blockfrost RYO: bypasses count/page and runs the unpaged SQL path.
    if unpaged {
        req = req.header("unpaged", "true");
    }
    req
}

/// List path for active DReps only (`retired=false&expired=false`).
fn dreps_list_path(page: Option<(usize, u32)>) -> String {
    match page {
        None => format!("/governance/dreps?{DREP_LIST_FILTERS}"),
        Some((count, page)) => {
            format!("/governance/dreps?count={count}&page={page}&{DREP_LIST_FILTERS}")
        }
    }
}

fn retry_backoff(attempt: u32) -> Duration {
    // 1s, 2s, 4s, … capped.
    let secs = (1u64 << attempt.min(5)).min(DREP_RETRY_BACKOFF_MAX.as_secs());
    Duration::from_secs(secs)
}

/// GET JSON from Blockfrost. Retries forever on failure; in-flight waits have
/// no deadline (caller must use a no-timeout client).
async fn bf_get_json_retry(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    project_id: Option<&str>,
    label: &str,
    unpaged: bool,
) -> Result<Value> {
    let mut attempt = 0u32;
    loop {
        match bf_get(http, base, path, project_id, unpaged).send().await {
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
        match bf_get(http, base, path, project_id, false).send().await {
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

enum ListScrape {
    /// Embedded `metadata` present (possibly sparse names).
    Embedded,
    /// List has no `metadata` field — caller should fetch per-id `/metadata`.
    NeedPerIdMetadata,
}

async fn scrape_dreps(
    cache: &DrepCache,
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<()> {
    match scrape_list_unpaged(cache, http, base, project_id).await? {
        Some(ListScrape::Embedded) => {
            tracing::info!("drep cache: used unpaged /governance/dreps with embedded metadata");
            return Ok(());
        }
        Some(ListScrape::NeedPerIdMetadata) => {
            tracing::warn!(
                "drep cache: unpaged /governance/dreps has no metadata field - fetching /metadata per id"
            );
            return scrape_list_and_metadata(cache, http, base, project_id).await;
        }
        None => {
            tracing::info!("drep cache: unpaged incomplete or unused - falling back to pagination");
        }
    }

    match scrape_list_embedded(cache, http, base, project_id).await {
        Ok(true) => {
            tracing::info!("drep cache: used paginated /governance/dreps with embedded metadata");
            Ok(())
        }
        Ok(false) => {
            tracing::warn!(
                "drep cache: /governance/dreps has no metadata field - fetching /metadata per id"
            );
            scrape_list_and_metadata(cache, http, base, project_id).await
        }
        Err(e) => Err(e),
    }
}

/// Merge embedded metadata names from list rows into `cache`.
/// Returns `(saw_metadata_field, named_count)`.
fn merge_embedded_rows(cache: &DrepCache, arr: &[Value]) -> (bool, usize) {
    let mut saw_metadata_field = false;
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
    let named = batch.len();
    cache.remember_many(batch);
    (saw_metadata_field, named)
}

/// RYO `unpaged: true` — one `GET /governance/dreps?retired=false&expired=false`.
///
/// Returns `None` when the response looks truncated (header ignored by a
/// paginating proxy / public API) so the caller can paginate instead.
async fn scrape_list_unpaged(
    cache: &DrepCache,
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<Option<ListScrape>> {
    let path = dreps_list_path(None);
    let rows = bf_get_json_retry(
        http,
        base,
        &path,
        project_id,
        "GET /governance/dreps (unpaged, active)",
        true,
    )
    .await?;
    let Some(arr) = rows.as_array() else {
        return Ok(None);
    };
    if arr.is_empty() {
        return Ok(None);
    }

    // Public Blockfrost ignores unknown headers and returns the default page
    // (count=100). If we got ≤ PAGE rows, check whether page 2 exists.
    if arr.len() <= PAGE {
        let probe = dreps_list_path(Some((PAGE, 2)));
        let more = bf_get_json_retry(
            http,
            base,
            &probe,
            project_id,
            "GET /governance/dreps?page=2 (unpaged probe)",
            false,
        )
        .await?;
        if more.as_array().is_some_and(|a| !a.is_empty()) {
            tracing::info!(
                "drep cache: unpaged returned {} rows but page 2 is non-empty - header ignored",
                arr.len()
            );
            return Ok(None);
        }
    }

    let (saw_meta, named) = merge_embedded_rows(cache, arr);
    tracing::info!(
        "drep cache: unpaged list ({} rows, {named} names)",
        arr.len()
    );
    if saw_meta || named > 0 {
        Ok(Some(ListScrape::Embedded))
    } else {
        Ok(Some(ListScrape::NeedPerIdMetadata))
    }
}

/// Page `GET /governance/dreps` (active only) and merge `metadata` names.
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
        let path = dreps_list_path(Some((PAGE, page)));
        let label = format!("GET {path}");
        let rows = bf_get_json_retry(http, base, &path, project_id, &label, false).await?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        let (saw, batch_named) = merge_embedded_rows(cache, arr);
        saw_metadata_field |= saw;
        named += batch_named;
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
        let path = dreps_list_path(Some((PAGE, page)));
        let label = format!("GET {path}");
        let rows = bf_get_json_retry(http, base, &path, project_id, &label, false).await?;
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
    use wiremock::matchers::{header, method, path, query_param};
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

    #[test]
    fn list_path_ends_with_active_filters() {
        assert_eq!(
            dreps_list_path(None),
            "/governance/dreps?retired=false&expired=false"
        );
        assert_eq!(
            dreps_list_path(Some((100, 3))),
            "/governance/dreps?count=100&page=3&retired=false&expired=false"
        );
    }

    #[tokio::test]
    async fn scrape_unpaged_one_shot() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(header("unpaged", "true"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 105)))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        assert_eq!(cache.len(), 0);

        let http = reqwest::Client::new();
        let n = cache.refresh(&http, &server.uri(), None).await.unwrap();
        assert_eq!(n, 105);
        assert_eq!(cache.len(), 105);
        assert_eq!(
            cache
                .get(&format!("drep1test{:0>50}", 0))
                .and_then(|e| e.name),
            Some("DRep 0".into())
        );
        assert_eq!(
            cache
                .get(&format!("drep1test{:0>50}", 104))
                .and_then(|e| e.name),
            Some("DRep 104".into())
        );
        assert!(dir.path().join(CACHE_FILE).exists());
    }

    #[tokio::test]
    async fn scrape_falls_back_to_pagination_when_unpaged_truncated() {
        let server = MockServer::start().await;

        // "Unpaged" returns a single default page — header ignored.
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(header("unpaged", "true"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 100)))
            .expect(1)
            .mount(&server)
            .await;
        // Probe finds page 2 → fall back to pagination.
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("page", "2"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(100, 5)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("count", "100"))
            .and(query_param("page", "1"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 100)))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        let http = reqwest::Client::new();
        let n = cache.refresh(&http, &server.uri(), None).await.unwrap();
        assert_eq!(n, 105);
        assert_eq!(cache.len(), 105);
    }

    #[tokio::test]
    async fn scrape_waits_past_enricher_client_timeout() {
        // Enricher's live client is 10s; scrape must still succeed if RYO is slower.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(header("unpaged", "true"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(12))
                    .set_body_json(page_body(0, 2)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("page", "2"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        // Deliberately pass a short-timeout client — refresh must ignore it.
        let short = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let n = cache.refresh(&short, &server.uri(), None).await.unwrap();
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn scrape_retries_until_unpaged_succeeds() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(header("unpaged", "true"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(header("unpaged", "true"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(0, 3)))
            .mount(&server)
            .await;
        // ≤ PAGE rows → probe page 2 (empty) so unpaged is accepted.
        Mock::given(method("GET"))
            .and(path("/governance/dreps"))
            .and(query_param("page", "2"))
            .and(query_param("retired", "false"))
            .and(query_param("expired", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        let http = reqwest::Client::new();
        let n = cache.refresh(&http, &server.uri(), None).await.unwrap();
        assert_eq!(n, 3);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn cip129_and_legacy_drep_ids_alias() {
        // Synthetic 28-byte credential — no mainnet DRep ids in fixtures.
        let cred_hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c";
        let cip129 = drep_bech32_cip129(cred_hex, Some("verificationKey")).unwrap();
        let legacy = encode_drep_payload(&hex::decode(cred_hex).unwrap()).unwrap();
        let aliases = drep_id_aliases(&cip129);
        assert!(aliases.contains(&cip129));
        assert!(aliases.contains(&legacy));
        let back = drep_id_aliases(&legacy);
        assert!(back.iter().any(|a| a == &cip129));
    }

    #[test]
    fn normalizes_blockfrost_special_dreps() {
        assert_eq!(normalize_drep_id("drep_always_abstain"), "Always Abstain");
        assert_eq!(normalize_drep_id("Always Abstain"), "Always Abstain");
        assert_eq!(
            normalize_drep_id("drep_always_no_confidence"),
            "Always No Confidence"
        );
        assert!(drep_ids_equal(
            "drep_always_abstain",
            "Always Abstain"
        ));
        assert!(!drep_ids_equal(
            "drep_always_abstain",
            "Always No Confidence"
        ));
    }

    #[test]
    fn cache_get_resolves_legacy_via_cip129_key() {
        let cred_hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c";
        let cip129 = drep_bech32_cip129(cred_hex, Some("verificationKey")).unwrap();
        let legacy = encode_drep_payload(&hex::decode(cred_hex).unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cache = DrepCache::load(Some(dir.path()));
        cache.remember(
            &cip129,
            DrepEntry {
                name: Some("Test DRep".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            cache.get(&legacy).and_then(|e| e.name),
            Some("Test DRep".into())
        );
        let dump = cache.to_names_json();
        assert_eq!(dump[&legacy]["name"], "Test DRep");
        assert_eq!(dump[&cip129]["name"], "Test DRep");
    }

    #[test]
    fn drep_bech32_cip129_uses_header() {
        let hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c";
        let b32 = drep_bech32_cip129(hex, Some("verificationKey")).unwrap();
        assert!(b32.starts_with("drep1"));
        let raw = decode_drep_payload(&b32).unwrap();
        assert_eq!(raw.len(), 29);
        assert_eq!(raw[0], CIP129_DREP_KEY);
        assert_eq!(&raw[1..], &hex::decode(hex).unwrap());
    }
}

