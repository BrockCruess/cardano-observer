//! Full Cardano token registry (CIP-26) loaded into memory at startup.
//!
//! On first run, downloads the official GitHub mappings zip, strips it to
//! subject → {name, ticker, decimals} (no logos), and writes
//! `token-registry.json`. Later starts always load that file - registry
//! entries are append-only / immutable, so we never re-fetch unless the
//! cache is missing or `TOKEN_REGISTRY_REFRESH=1`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Default zip of https://github.com/cardano-foundation/cardano-token-registry
pub const DEFAULT_REGISTRY_ZIP: &str =
    "https://github.com/cardano-foundation/cardano-token-registry/archive/refs/heads/master.zip";

const SLIM_CACHE_FILE: &str = "token-registry.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegistryEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u32>,
}

pub struct TokenRegistry {
    by_subject: HashMap<String, RegistryEntry>,
}

impl TokenRegistry {
    pub fn empty() -> Self {
        TokenRegistry {
            by_subject: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_subject.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_subject.is_empty()
    }

    /// Look up a unit (policy‖assetName hex). Also tries CIP-68 prefix variants.
    pub fn get(&self, unit: &str) -> Option<&RegistryEntry> {
        for key in lookup_keys(unit) {
            if let Some(hit) = self.by_subject.get(&key) {
                return Some(hit);
            }
        }
        None
    }

    pub fn to_json(&self, unit: &str) -> Option<Value> {
        let e = self.get(unit)?;
        Some(json!({
            "unit": unit,
            "name": e.name,
            "ticker": e.ticker,
            // CIP-26: omitted decimals means 0 (base units = display units).
            "decimals": e.decimals.unwrap_or(0),
            "source": "registry",
        }))
    }

    /// Browser bulk hydrate: subject → {decimals, ticker, name} (only useful rows).
    pub fn to_assets_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (subject, e) in &self.by_subject {
            if e.decimals.is_none() && e.ticker.is_none() && e.name.is_none() {
                continue;
            }
            out.insert(
                subject.clone(),
                json!({
                    "decimals": e.decimals.unwrap_or(0),
                    "ticker": e.ticker,
                    "name": e.name,
                }),
            );
        }
        json!({ "assets": out, "count": out.len() })
    }

    /// Load `token-registry.json` if present; otherwise download + parse the zip
    /// and write the file for every subsequent boot.
    pub async fn load(
        http: &reqwest::Client,
        cache_dir: Option<&Path>,
        zip_url: &str,
        force_refresh: bool,
    ) -> Result<Self> {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let path = dir.join(SLIM_CACHE_FILE);

        if !force_refresh {
            match load_slim_cache(&path) {
                Ok(reg) if !reg.is_empty() => {
                    tracing::info!(
                        "token registry: loaded {} entries from {}",
                        reg.len(),
                        path.display()
                    );
                    return Ok(reg);
                }
                Ok(_) => tracing::warn!("token registry cache empty - downloading"),
                Err(_) if path.exists() => {
                    tracing::warn!("token registry cache unreadable - downloading");
                }
                Err(_) => {
                    tracing::info!("token registry: no cache at {} - downloading once", path.display());
                }
            }
        } else {
            tracing::info!("token registry: TOKEN_REGISTRY_REFRESH set - re-downloading");
        }

        tracing::info!("token registry: downloading {zip_url}");
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let zip_path = dir.join("token-registry.zip");

        let mut resp = http
            .get(zip_url)
            .send()
            .await
            .context("download token registry zip")?
            .error_for_status()
            .context("token registry zip HTTP error")?;
        {
            use std::io::Write;
            let mut out = fs::File::create(&zip_path)
                .with_context(|| format!("create {}", zip_path.display()))?;
            loop {
                let Some(chunk) = resp.chunk().await.context("read zip chunk")? else { break };
                out.write_all(&chunk)?;
            }
            out.flush()?;
        }

        let file = fs::File::open(&zip_path)
            .with_context(|| format!("open {}", zip_path.display()))?;
        let reg = parse_registry_zip(file).context("parse token registry zip")?;
        tracing::info!("token registry: parsed {} entries from zip", reg.len());
        let _ = fs::remove_file(&zip_path);

        if let Err(e) = save_slim_cache(&path, &reg) {
            tracing::warn!("could not write token registry cache: {e:#}");
        } else {
            tracing::info!("token registry: wrote durable cache at {}", path.display());
        }

        Ok(reg)
    }
}

fn lookup_keys(unit: &str) -> Vec<String> {
    let unit = unit.to_ascii_lowercase();
    let mut keys = vec![unit.clone()];
    if unit.len() <= 56 {
        return keys;
    }
    let (policy, name) = unit.split_at(56);
    // CIP-68 labels: (100) reference NFT, (222) user token.
    for prefix in ["000de140", "0014df10"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            keys.push(format!("{policy}{rest}"));
        } else {
            // Also try *adding* the prefix for registries that store the bare name.
            keys.push(format!("{policy}{prefix}{name}"));
        }
    }
    keys
}

fn load_slim_cache(path: &Path) -> Result<TokenRegistry> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let map: HashMap<String, RegistryEntry> =
        serde_json::from_str(&text).context("parse slim registry cache")?;
    Ok(TokenRegistry { by_subject: map })
}

fn save_slim_cache(path: &PathBuf, reg: &TokenRegistry) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&reg.by_subject)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn parse_registry_zip(file: fs::File) -> Result<TokenRegistry> {
    let mut archive = ZipArchive::new(file).context("open zip")?;
    let mut by_subject = HashMap::with_capacity(8_192);
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name().to_string();
        if !name.contains("/mappings/") || !name.ends_with(".json") {
            continue;
        }
        // Cap read size: entries are ≤370KB; we only need the small fields.
        let mut buf = Vec::new();
        file.take(512 * 1024)
            .read_to_end(&mut buf)
            .context("read mapping entry")?;
        let Ok(v) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };
        let subject = v
            .get("subject")
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase())
            .or_else(|| {
                Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
            });
        let Some(subject) = subject else { continue };
        let entry = RegistryEntry {
            name: field_str(&v, "name"),
            ticker: field_str(&v, "ticker"),
            decimals: field_u32(&v, "decimals"),
        };
        if entry.name.is_none() && entry.ticker.is_none() && entry.decimals.is_none() {
            continue;
        }
        by_subject.insert(subject, entry);
    }
    Ok(TokenRegistry { by_subject })
}

fn field_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|f| f.get("value"))
        .and_then(|x| {
            x.as_str()
                .map(str::to_string)
                .or_else(|| x.as_i64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty())
}

fn field_u32(v: &Value, key: &str) -> Option<u32> {
    let x = v.get(key)?.get("value")?;
    if let Some(n) = x.as_u64() {
        return u32::try_from(n).ok();
    }
    if let Some(n) = x.as_i64() {
        return u32::try_from(n).ok();
    }
    x.as_str()?.parse().ok()
}

