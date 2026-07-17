//! ADA Handle resolution via KoraLabs Handles Public API (or the Cardano
//! Foundation `cf-adahandle-resolver`). Used to replace truncated stake
//! addresses on event cards with `$handle` when the account has one.

use crate::config::{HandleApiKind, Network};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const CACHE_CAP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleApi {
    /// KoraLabs / handles-public-api: `GET /holders/{address}` → `default_handle`.
    Kora,
    /// CF ada-handle-resolver: `GET /api/v1/ada-handles/by-stake-address/{address}` → `[handles…]`.
    Cf,
}

impl HandleApi {
    pub fn from_config(kind: HandleApiKind, base_url: &str) -> Self {
        match kind {
            HandleApiKind::Kora => HandleApi::Kora,
            HandleApiKind::Cf => HandleApi::Cf,
            HandleApiKind::Auto => {
                let u = base_url.to_ascii_lowercase();
                if u.contains(":9095") || u.contains("adahandle") || u.contains("ada-handle") {
                    HandleApi::Cf
                } else {
                    HandleApi::Kora
                }
            }
        }
    }
}

pub fn default_public_url(network: Network) -> String {
    match network {
        Network::Mainnet => "https://api.handle.me".into(),
        Network::Preprod => "https://preprod.api.handle.me".into(),
        Network::Preview => "https://preview.api.handle.me".into(),
    }
}

/// Cached Handle lookups. `None` base URL disables resolution entirely.
pub struct HandleCache {
    http: reqwest::Client,
    base: Option<String>,
    api: HandleApi,
    /// address → preferred handle (without `$`); empty string = negative cache.
    cache: Mutex<HashMap<String, String>>,
}

impl HandleCache {
    pub fn new(base: Option<String>, api_kind: HandleApiKind) -> Self {
        let api = base
            .as_deref()
            .map(|u| HandleApi::from_config(api_kind, u))
            .unwrap_or(HandleApi::Kora);
        if let Some(ref url) = base {
            tracing::info!("ADA Handle resolver: {} ({:?})", url, api);
        } else {
            tracing::info!("ADA Handle resolver: disabled");
        }
        HandleCache {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .expect("handle http client"),
            base,
            api,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_cached(&self, address: &str) -> Option<Option<String>> {
        let c = self.cache.lock().unwrap();
        c.get(address).map(|h| {
            if h.is_empty() {
                None
            } else {
                Some(h.clone())
            }
        })
    }

    fn remember(&self, address: &str, handle: Option<&str>) {
        let mut c = self.cache.lock().unwrap();
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        c.insert(address.to_string(), handle.unwrap_or("").to_string());
    }

    /// Resolve preferred Handle for a stake (or other) address.
    /// Returns `{ address, handle }` where `handle` is without `$`, or null when unknown.
    pub async fn resolve(&self, address: &str) -> Value {
        let addr = address.trim();
        if !is_lookup_address(addr) {
            return json!({ "error": "bad address" });
        }
        if self.base.is_none() {
            return json!({ "address": addr, "handle": Value::Null, "disabled": true });
        }
        if let Some(cached) = self.get_cached(addr) {
            return json!({ "address": addr, "handle": cached });
        }
        let handle = match self.fetch(addr).await {
            Ok(h) => h,
            Err(e) => {
                tracing::debug!("handle lookup {addr}: {e:#}");
                None
            }
        };
        self.remember(addr, handle.as_deref());
        json!({ "address": addr, "handle": handle })
    }

    async fn fetch(&self, addr: &str) -> anyhow::Result<Option<String>> {
        let base = self.base.as_deref().unwrap();
        match self.api {
            HandleApi::Kora => self.fetch_kora(base, addr).await,
            HandleApi::Cf => self.fetch_cf(base, addr).await,
        }
    }

    async fn fetch_kora(&self, base: &str, addr: &str) -> anyhow::Result<Option<String>> {
        // Bech32 addresses are URL-path safe.
        let url = format!("{base}/holders/{addr}");
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            anyhow::bail!("HTTP {status}");
        }
        let v: Value = resp.json().await?;
        let handle = v
            .get("default_handle")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(normalize_handle);
        Ok(handle)
    }

    async fn fetch_cf(&self, base: &str, addr: &str) -> anyhow::Result<Option<String>> {
        let url = format!("{base}/api/v1/ada-handles/by-stake-address/{addr}");
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            anyhow::bail!("HTTP {status}");
        }
        let v: Value = resp.json().await?;
        let handles: Vec<String> = match v {
            Value::Array(items) => items
                .into_iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .filter(|s| !s.trim().is_empty())
                .map(|s| normalize_handle(&s))
                .collect(),
            _ => Vec::new(),
        };
        Ok(pick_preferred(&handles))
    }
}

fn is_lookup_address(addr: &str) -> bool {
    let a = addr.trim();
    if a.len() < 15 || a.len() > 128 {
        return false;
    }
    // Stake addresses are the common case on event cards; also accept payment
    // addrs in case we expand later.
    a.starts_with("stake1")
        || a.starts_with("stake_test1")
        || a.starts_with("addr1")
        || a.starts_with("addr_test1")
}

fn normalize_handle(s: &str) -> String {
    s.trim().trim_start_matches('$').to_string()
}

/// CF resolver has no preferred-handle field — mirror KoraLabs' simple ranking:
/// shortest name, then ascending alpha.
fn pick_preferred(handles: &[String]) -> Option<String> {
    handles
        .iter()
        .filter(|h| !h.is_empty())
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_preferred_shortest_then_alpha() {
        let handles = ["zebra", "ab", "aa", "abc"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(pick_preferred(&handles).as_deref(), Some("aa"));
    }

    #[test]
    fn normalize_strips_dollar() {
        assert_eq!(normalize_handle("$cool"), "cool");
        assert_eq!(normalize_handle(" cool "), "cool");
    }

    #[test]
    fn auto_api_detects_cf_port() {
        assert_eq!(
            HandleApi::from_config(HandleApiKind::Auto, "http://127.0.0.1:9095"),
            HandleApi::Cf
        );
        assert_eq!(
            HandleApi::from_config(HandleApiKind::Auto, "https://api.handle.me"),
            HandleApi::Kora
        );
    }
}
