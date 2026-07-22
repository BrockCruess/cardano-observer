use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub ogmios_url: String,
    /// Base URL of the enrichment API (Blockfrost-compatible). Populated from
    /// OBSERVER_BACKEND_URL when USE_OBSERVER_BACKEND is set, else BLOCKFROST_URL.
    pub blockfrost_url: Option<String>,
    pub blockfrost_project_id: Option<String>,
    /// True when the self-hosted cardano-observer-backend is selected.
    pub use_observer_backend: bool,
    /// Base URL for ADA Handle resolution (KoraLabs or CF resolver).
    /// `None` disables Handle enrichment on event cards.
    pub ada_handle_url: Option<String>,
    /// Which Handle API shape to speak (`auto` picks from the URL).
    pub ada_handle_api: HandleApiKind,
    /// GitHub zip (or mirror) of the CIP-26 mappings tree - fetched only when
    /// the on-disk cache is missing.
    pub token_registry_zip: String,
    /// Force re-download of the registry zip even if the cache file exists.
    pub token_registry_refresh: bool,
    /// Force re-scrape of Blockfrost pool list even if pools.json exists.
    pub pool_cache_refresh: bool,
    /// Force re-scrape of Blockfrost DRep list even if dreps.json exists.
    pub drep_cache_refresh: bool,
    /// CIP-14 scam fingerprint list URL (raw text, `#` comments ignored).
    pub scam_token_list_url: String,
    /// Force re-download of the scam list even if the cache file exists.
    pub scam_token_list_refresh: bool,
    pub network: Network,
    pub bind: String,
    /// How many hours of events to keep in the in-memory ring (trending + fast search).
    pub event_retention_hours: u64,
    pub tx_cache: usize,
    pub demo: bool,
    /// Directory for persisted events/txs; None disables persistence.
    pub data_dir: Option<String>,
    /// On restart, resume chain-sync from the last persisted block if it is
    /// at most this many hours old (0 disables backfill - start at the tip).
    pub backfill_hours: u64,
}

/// Which Handle HTTP API to call behind `ADA_HANDLE_URL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleApiKind {
    /// Infer from URL (port 9095 / "adahandle" → CF, else KoraLabs).
    Auto,
    /// KoraLabs Handles Public API (`/holders/{address}`).
    Kora,
    /// Cardano Foundation `cf-adahandle-resolver`.
    Cf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Preprod,
    Preview,
}

impl Network {
    pub fn as_str(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Preprod => "preprod",
            Network::Preview => "preview",
        }
    }

    /// Network id bit used in stake address headers (1 = mainnet, 0 = testnets)
    pub fn id_bit(&self) -> u8 {
        match self {
            Network::Mainnet => 1,
            _ => 0,
        }
    }

    pub fn stake_hrp(&self) -> &'static str {
        match self {
            Network::Mainnet => "stake",
            _ => "stake_test",
        }
    }
}

fn non_empty(key: &str) -> Option<String> {
    env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Self {
        let network = match non_empty("NETWORK").as_deref() {
            Some("preprod") => Network::Preprod,
            Some("preview") => Network::Preview,
            _ => Network::Mainnet,
        };
        // Backend selection: USE_OBSERVER_BACKEND=true points the enrichment
        // API at a self-hosted cardano-observer-backend instead of Blockfrost.
        // Both speak the same HTTP API, so the rest of the app is agnostic.
        let use_observer_backend = matches!(
            non_empty("USE_OBSERVER_BACKEND").as_deref(),
            Some("true") | Some("1") | Some("yes")
        );
        let (blockfrost_url, blockfrost_project_id) = if use_observer_backend {
            let url = non_empty("OBSERVER_BACKEND_URL")
                .unwrap_or_else(|| "http://127.0.0.1:3300".into());
            (Some(url.trim_end_matches('/').to_string()), None)
        } else {
            (
                non_empty("BLOCKFROST_URL").map(|u| u.trim_end_matches('/').to_string()),
                non_empty("BLOCKFROST_PROJECT_ID"),
            )
        };
        Config {
            ogmios_url: non_empty("OGMIOS_URL").unwrap_or_else(|| "ws://127.0.0.1:1337".into()),
            blockfrost_url,
            blockfrost_project_id,
            use_observer_backend,
            ada_handle_url: match non_empty("ADA_HANDLE_URL").as_deref() {
                Some("none") | Some("off") | Some("false") => None,
                Some(url) => Some(url.trim_end_matches('/').to_string()),
                // Default: free public Handles API for the configured network.
                None => Some(crate::handles::default_public_url(network)),
            },
            ada_handle_api: match non_empty("ADA_HANDLE_API").as_deref() {
                Some("kora") | Some("koralabs") | Some("handle.me") => HandleApiKind::Kora,
                Some("cf") | Some("cardano-foundation") | Some("resolver") => HandleApiKind::Cf,
                _ => HandleApiKind::Auto,
            },
            token_registry_zip: non_empty("TOKEN_REGISTRY_ZIP").unwrap_or_else(|| {
                crate::registry::DEFAULT_REGISTRY_ZIP.to_string()
            }),
            token_registry_refresh: matches!(
                non_empty("TOKEN_REGISTRY_REFRESH").as_deref(),
                Some("true") | Some("1") | Some("yes")
            ),
            pool_cache_refresh: matches!(
                non_empty("POOL_CACHE_REFRESH").as_deref(),
                Some("true") | Some("1") | Some("yes")
            ),
            drep_cache_refresh: matches!(
                non_empty("DREP_CACHE_REFRESH").as_deref(),
                Some("true") | Some("1") | Some("yes")
            ),
            scam_token_list_url: non_empty("SCAM_TOKEN_LIST_URL").unwrap_or_else(|| {
                crate::scam_tokens::DEFAULT_SCAM_TOKEN_LIST_URL.to_string()
            }),
            scam_token_list_refresh: matches!(
                non_empty("SCAM_TOKEN_LIST_REFRESH").as_deref(),
                Some("true") | Some("1") | Some("yes")
            ),
            network,
            bind: non_empty("BIND").unwrap_or_else(|| "0.0.0.0:9070".into()),
            event_retention_hours: non_empty("EVENT_RETENTION_HOURS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(24)
                .max(1),
            // Soft ceiling for in-memory tx bodies (0 = unlimited within retention).
            // Prefer leaving at 0 so txs stay available for the full EVENT_RETENTION_HOURS window.
            tx_cache: non_empty("TX_CACHE").and_then(|v| v.parse().ok()).unwrap_or(0),
            demo: matches!(non_empty("DEMO").as_deref(), Some("true") | Some("1") | Some("yes")),
            data_dir: match non_empty("DATA_DIR").as_deref() {
                Some("none") | Some("off") | Some("false") => None,
                Some(dir) => Some(dir.to_string()),
                None => Some("./data".to_string()),
            },
            backfill_hours: non_empty("BACKFILL_HOURS").and_then(|v| v.parse().ok()).unwrap_or(24),
        }
    }
}
