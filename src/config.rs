use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub ogmios_url: String,
    pub blockfrost_url: Option<String>,
    pub blockfrost_project_id: Option<String>,
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
        Config {
            ogmios_url: non_empty("OGMIOS_URL").unwrap_or_else(|| "ws://127.0.0.1:1337".into()),
            blockfrost_url: non_empty("BLOCKFROST_URL").map(|u| u.trim_end_matches('/').to_string()),
            blockfrost_project_id: non_empty("BLOCKFROST_PROJECT_ID"),
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
            network,
            bind: non_empty("BIND").unwrap_or_else(|| "0.0.0.0:9070".into()),
            event_retention_hours: non_empty("EVENT_RETENTION_HOURS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(24)
                .max(1),
            tx_cache: non_empty("TX_CACHE").and_then(|v| v.parse().ok()).unwrap_or(4000),
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
