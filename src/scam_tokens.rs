//! Known scam-token fingerprint list (CIP-14 `asset1…`).
//!
//! Upstream is a plain-text list (comments + fingerprints). At boot we fetch it,
//! parse into a set, and persist `DATA_DIR/scam-tokens.json` — same pattern as
//! `token-registry.json` / `pools.json`. Daily 00:00 UTC refresh re-fetches and
//! rewrites the JSON. Lines starting with `#` are ignored.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Default source list (one CIP-14 fingerprint per line).
pub const DEFAULT_SCAM_TOKEN_LIST_URL: &str =
    "https://raw.githubusercontent.com/BrockCruess/Cardano-Scam-Token-Registry/refs/heads/main/scam-token-list";

const CACHE_FILE: &str = "scam-tokens.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScamTokenCache {
    /// Absolute fingerprint count (mirrors `fingerprints.len()`).
    count: usize,
    /// Source URL last used to build this cache.
    source: String,
    /// Sorted CIP-14 fingerprints.
    fingerprints: Vec<String>,
}

pub struct ScamTokenList {
    fingerprints: Mutex<HashSet<String>>,
    path: Option<PathBuf>,
    url: String,
}

impl ScamTokenList {
    pub fn empty(cache_dir: Option<&Path>, url: &str) -> Self {
        Self {
            fingerprints: Mutex::new(HashSet::new()),
            path: cache_dir.map(|d| d.join(CACHE_FILE)),
            url: url.to_string(),
        }
    }

    pub fn len(&self) -> usize {
        self.fingerprints.lock().unwrap().len()
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        let fp = fingerprint.trim();
        if fp.is_empty() {
            return false;
        }
        self.fingerprints.lock().unwrap().contains(fp)
    }

    /// Load `scam-tokens.json` when present; otherwise fetch `url` and persist.
    pub async fn load(
        http: &reqwest::Client,
        cache_dir: Option<&Path>,
        url: &str,
        force_refresh: bool,
    ) -> Result<Self> {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(CACHE_FILE);

        if !force_refresh {
            match load_cache(&path) {
                Ok(set) if !set.is_empty() => {
                    tracing::info!(
                        "scam-token list: loaded {} fingerprints from {}",
                        set.len(),
                        path.display()
                    );
                    return Ok(Self {
                        fingerprints: Mutex::new(set),
                        path: Some(path),
                        url: url.to_string(),
                    });
                }
                Ok(_) => tracing::warn!("scam-token cache empty - downloading"),
                Err(_) if path.exists() => {
                    tracing::warn!("scam-token cache unreadable - downloading");
                }
                Err(_) => {
                    tracing::info!(
                        "scam-token list: no cache at {} - downloading once",
                        path.display()
                    );
                }
            }
        } else {
            tracing::info!("scam-token list: SCAM_TOKEN_LIST_REFRESH set - re-downloading");
        }

        let list = Self {
            fingerprints: Mutex::new(HashSet::new()),
            path: Some(path),
            url: url.to_string(),
        };
        list.refresh(http).await?;
        Ok(list)
    }

    /// Re-fetch the remote list, replace in-memory + JSON cache.
    pub async fn refresh(&self, http: &reqwest::Client) -> Result<usize> {
        let text = tokio::time::timeout(Duration::from_secs(30), http.get(&self.url).send())
            .await
            .context("scam-token list request timed out")?
            .context("scam-token list request failed")?
            .error_for_status()
            .context("scam-token list HTTP error")?
            .text()
            .await
            .context("scam-token list body")?;
        let set = parse_list(&text);
        let n = set.len();
        if n == 0 {
            anyhow::bail!("scam-token list parsed empty (aborting overwrite)");
        }
        if let Some(path) = &self.path {
            if let Err(e) = save_cache(path, &self.url, &set) {
                tracing::warn!("could not write scam-token cache: {e:#}");
            } else {
                tracing::info!(
                    "scam-token list: wrote {n} fingerprints → {}",
                    path.display()
                );
            }
        } else {
            tracing::info!("scam-token list: loaded {n} fingerprints (no disk cache)");
        }
        *self.fingerprints.lock().unwrap() = set;
        Ok(n)
    }
}

fn load_cache(path: &Path) -> Result<HashSet<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cache: ScamTokenCache = serde_json::from_str(&text).context("parse scam-token cache")?;
    Ok(cache.fingerprints.into_iter().collect())
}

fn save_cache(path: &Path, source: &str, set: &HashSet<String>) -> Result<()> {
    let mut fingerprints: Vec<String> = set.iter().cloned().collect();
    fingerprints.sort();
    let cache = ScamTokenCache {
        count: fingerprints.len(),
        source: source.to_string(),
        fingerprints,
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&cache)?;
    fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename {}", path.display()))?;
    Ok(())
}

fn parse_list(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // CIP-14 fingerprints; ignore accidental junk.
        if t.starts_with("asset1") && t.len() >= 20 && t.len() <= 120 {
            out.insert(t.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_keeps_assets() {
        let text = "\
# comment
#asset1ignoredbecausehashed
asset1nl72gzsyzsdfmgu8fe7ljkuen5sfd59tn788nq
  asset1q03s6t98y73rfumn6hgt7zdnge3x8rym7hpvra  
not-an-asset
";
        let set = parse_list(text);
        assert_eq!(set.len(), 2);
        assert!(set.contains("asset1nl72gzsyzsdfmgu8fe7ljkuen5sfd59tn788nq"));
        assert!(!set.contains("asset1ignoredbecausehashed"));
    }

    #[test]
    fn cache_roundtrip_pretty_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);
        let set: HashSet<String> = [
            "asset1nl72gzsyzsdfmgu8fe7ljkuen5sfd59tn788nq",
            "asset1q03s6t98y73rfumn6hgt7zdnge3x8rym7hpvra",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        save_cache(&path, DEFAULT_SCAM_TOKEN_LIST_URL, &set).unwrap();
        let loaded = load_cache(&path).unwrap();
        assert_eq!(loaded, set);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"count\": 2"));
        assert!(text.contains("\"fingerprints\""));
    }
}
