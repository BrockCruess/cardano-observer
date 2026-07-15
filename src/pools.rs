//! Durable stake-pool metadata cache (ticker / name / homepage).
//!
//! First boot scrapes Blockfrost `/pools` (+ per-pool `/metadata`) into
//! `pools.json`. Later boots always load that file. Individual misses are
//! fetched live once and appended - we never re-pull a pool already cached.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const CACHE_FILE: &str = "pools.json";
const PAGE: usize = 100;
/// Parallel metadata fetches when `/pools/extended` is unavailable.
const META_CONCURRENCY: usize = 24;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PoolEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

impl PoolEntry {
    pub fn has_label(&self) -> bool {
        self.ticker.as_ref().is_some_and(|s| !s.is_empty())
            || self.name.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn to_json(&self, pool_id: &str) -> Value {
        json!({
            "pool": pool_id,
            "ticker": self.ticker,
            "name": self.name,
            "homepage": self.homepage,
            "source": "cache",
        })
    }

    fn from_blockfrost_meta(v: &Value) -> Option<Self> {
        let ticker = v
            .get("ticker")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let homepage = v
            .get("homepage")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let entry = PoolEntry {
            ticker,
            name,
            homepage,
        };
        entry.has_label().then_some(entry)
    }
}

pub struct PoolCache {
    by_id: Mutex<HashMap<String, PoolEntry>>,
    path: Option<PathBuf>,
}

impl PoolCache {
    pub fn empty() -> Self {
        PoolCache {
            by_id: Mutex::new(HashMap::new()),
            path: None,
        }
    }

    pub fn len(&self) -> usize {
        self.by_id.lock().unwrap().len()
    }

    pub fn get(&self, pool_id: &str) -> Option<PoolEntry> {
        self.by_id.lock().unwrap().get(pool_id).cloned()
    }

    /// Insert (or replace) and rewrite the on-disk cache when we learned something new.
    pub fn remember(&self, pool_id: &str, entry: PoolEntry) {
        if !entry.has_label() {
            return;
        }
        let mut map = self.by_id.lock().unwrap();
        let changed = match map.get(pool_id) {
            Some(old) => old.ticker != entry.ticker || old.name != entry.name,
            None => true,
        };
        if !changed {
            return;
        }
        map.insert(pool_id.to_string(), entry);
        if let Some(path) = &self.path {
            let _ = save_cache(path, &map);
        }
    }

    /// Load `pools.json` if present; otherwise scrape Blockfrost once.
    pub async fn load(
        http: &reqwest::Client,
        cache_dir: Option<&Path>,
        blockfrost_url: Option<&str>,
        project_id: Option<&str>,
        force_refresh: bool,
    ) -> Result<Self> {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let path = dir.join(CACHE_FILE);

        if !force_refresh {
            match load_cache(&path) {
                Ok(map) if !map.is_empty() => {
                    tracing::info!(
                        "pool cache: loaded {} entries from {}",
                        map.len(),
                        path.display()
                    );
                    return Ok(PoolCache {
                        by_id: Mutex::new(map),
                        path: Some(path),
                    });
                }
                Ok(_) => tracing::warn!("pool cache empty - scraping"),
                Err(_) if path.exists() => tracing::warn!("pool cache unreadable - scraping"),
                Err(_) => tracing::info!(
                    "pool cache: no file at {} - scraping once",
                    path.display()
                ),
            }
        } else {
            tracing::info!("pool cache: POOL_CACHE_REFRESH set - re-scraping");
        }

        let Some(bf_base) = blockfrost_url else {
            tracing::warn!("pool cache: no BLOCKFROST_URL - leaving cache empty");
            return Ok(PoolCache {
                by_id: Mutex::new(HashMap::new()),
                path: Some(path),
            });
        };

        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

        let scrape_http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| http.clone());

        let map = scrape_pools(&scrape_http, bf_base, project_id)
            .await
            .context("Blockfrost pool scrape failed")?;

        tracing::info!("pool cache: scraped {} pools with tickers", map.len());
        if let Err(e) = save_cache(&path, &map) {
            tracing::warn!("could not write pool cache: {e:#}");
        } else {
            tracing::info!("pool cache: wrote durable cache at {}", path.display());
        }

        Ok(PoolCache {
            by_id: Mutex::new(map),
            path: Some(path),
        })
    }
}

fn load_cache(path: &Path) -> Result<HashMap<String, PoolEntry>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).context("parse pool cache")
}

fn save_cache(path: &Path, map: &HashMap<String, PoolEntry>) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(map)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
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

async fn scrape_pools(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, PoolEntry>> {
    // Prefer `/pools/extended` (metadata in one page). Some RYO builds hang or
    // 500 on it - fall back to `/pools` + per-id `/metadata`.
    match scrape_extended(http, base, project_id).await {
        Ok(m) if !m.is_empty() => {
            tracing::info!("pool cache: used Blockfrost /pools/extended");
            return Ok(m);
        }
        Ok(_) => tracing::warn!("pool cache: /pools/extended returned no tickers - falling back"),
        Err(e) => tracing::warn!("pool cache: /pools/extended failed ({e:#}) - falling back"),
    }
    scrape_list_and_metadata(http, base, project_id).await
}

async fn scrape_extended(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, PoolEntry>> {
    let mut map = HashMap::with_capacity(8_192);
    let mut page = 1u32;
    loop {
        let path = format!("/pools/extended?count={PAGE}&page={page}");
        let rows: Value = bf_get(http, base, &path, project_id)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .with_context(|| format!("GET {base}{path}"))?
            .error_for_status()
            .with_context(|| format!("pools/extended HTTP error page {page}"))?
            .json()
            .await
            .context("decode pools/extended")?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        for row in arr {
            let Some(id) = row.get("pool_id").and_then(Value::as_str) else {
                continue;
            };
            let meta = row.get("metadata").unwrap_or(row);
            if let Some(entry) = PoolEntry::from_blockfrost_meta(meta) {
                map.insert(id.to_string(), entry);
            }
        }
        if arr.len() < PAGE {
            break;
        }
        page += 1;
        if page % 10 == 0 {
            tracing::info!("pool cache: extended scrape - {} tickers so far…", map.len());
        }
    }
    Ok(map)
}

async fn scrape_list_and_metadata(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<HashMap<String, PoolEntry>> {
    let ids = list_pool_ids(http, base, project_id).await?;
    if ids.is_empty() {
        return Err(anyhow!("Blockfrost /pools returned no pool ids"));
    }
    tracing::info!("pool cache: listed {} pools - fetching metadata…", ids.len());

    let mut map = HashMap::with_capacity(ids.len());
    let mut set = tokio::task::JoinSet::new();
    let mut outstanding = 0usize;
    let mut done = 0usize;

    for id in ids {
        while outstanding >= META_CONCURRENCY {
            let Some(res) = set.join_next().await else { break };
            outstanding -= 1;
            done += 1;
            if let Ok(Some((pid, entry))) = res {
                map.insert(pid, entry);
            }
            if done % 500 == 0 {
                tracing::info!(
                    "pool cache: metadata {} done, {} tickers…",
                    done,
                    map.len()
                );
            }
        }
        let http = http.clone();
        let base = base.to_string();
        let pid_header = project_id.map(str::to_string);
        set.spawn(async move {
            let path = format!("/pools/{id}/metadata");
            let mut req = http.get(format!("{base}{path}"));
            if let Some(pid) = pid_header {
                req = req.header("project_id", pid);
            }
            let v: Value = tokio::time::timeout(Duration::from_secs(15), req.send())
                .await
                .ok()?
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()?;
            let entry = PoolEntry::from_blockfrost_meta(&v)?;
            Some((id, entry))
        });
        outstanding += 1;
    }

    loop {
        let Some(res) = set.join_next().await else { break };
        done += 1;
        if let Ok(Some((pid, entry))) = res {
            map.insert(pid, entry);
        }
        if done % 500 == 0 {
            tracing::info!(
                "pool cache: metadata {} done, {} tickers…",
                done,
                map.len()
            );
        }
    }

    Ok(map)
}

async fn list_pool_ids(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(8_192);
    let mut page = 1u32;
    loop {
        let path = format!("/pools?count={PAGE}&page={page}");
        let rows: Value = bf_get(http, base, &path, project_id)
            .send()
            .await
            .with_context(|| format!("GET {base}{path}"))?
            .error_for_status()
            .with_context(|| format!("pools HTTP error page {page}"))?
            .json()
            .await
            .context("decode pools")?;
        let Some(arr) = rows.as_array() else {
            break;
        };
        if arr.is_empty() {
            break;
        }
        for row in arr {
            if let Some(id) = row.as_str() {
                ids.push(id.to_string());
            }
        }
        if arr.len() < PAGE {
            break;
        }
        page += 1;
        if page % 20 == 0 {
            tracing::info!("pool cache: listed {} pool ids so far…", ids.len());
        }
    }
    Ok(ids)
}
