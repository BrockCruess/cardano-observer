use anyhow::{Context, Result};
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    /// PostgreSQL connection URL for the cardano-db-sync database.
    pub db_url: String,
    pub db_max_connections: u32,
    /// Server-side statement timeout in ms (0 disables).
    pub db_statement_timeout_ms: u64,
    /// Address and port the API listens on.
    pub bind: String,
    pub network: Network,
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

    /// Bech32 human-readable part of stake addresses on this network.
    pub fn stake_hrp(&self) -> &'static str {
        match self {
            Network::Mainnet => "stake",
            _ => "stake_test",
        }
    }
}

fn non_empty(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let network = match non_empty("NETWORK").as_deref() {
            Some("preprod") => Network::Preprod,
            Some("preview") => Network::Preview,
            _ => Network::Mainnet,
        };
        Ok(Config {
            db_url: non_empty("DBSYNC_URL").context(
                "DBSYNC_URL is required (e.g. postgres://dbsync@localhost:5432/cexplorer)",
            )?,
            db_max_connections: non_empty("DBSYNC_MAX_CONNECTIONS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(8)
                .max(1),
            db_statement_timeout_ms: non_empty("DBSYNC_STATEMENT_TIMEOUT_MS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(60_000),
            bind: non_empty("BACKEND_BIND").unwrap_or_else(|| "0.0.0.0:3300".into()),
            network,
        })
    }

    /// Connection URL with any password replaced, safe for log lines.
    pub fn redacted_db_url(&self) -> String {
        match url_password_span(&self.db_url) {
            Some((start, end)) => format!(
                "{}***{}",
                &self.db_url[..start],
                &self.db_url[end..]
            ),
            None => self.db_url.clone(),
        }
    }
}

/// Byte range of the password inside `scheme://user:password@host/...`, if any.
fn url_password_span(url: &str) -> Option<(usize, usize)> {
    let scheme_end = url.find("://")? + 3;
    let at = scheme_end + url[scheme_end..].find('@')?;
    let colon = scheme_end + url[scheme_end..at].find(':')?;
    Some((colon + 1, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password() {
        let c = Config {
            db_url: "postgres://user:secret@localhost:5432/db".into(),
            db_statement_timeout_ms: 60_000,
            db_max_connections: 8,
            bind: "0.0.0.0:3300".into(),
            network: Network::Mainnet,
        };
        assert_eq!(c.redacted_db_url(), "postgres://user:***@localhost:5432/db");
    }

    #[test]
    fn no_password_untouched() {
        let c = Config {
            db_url: "postgres://user@localhost/db".into(),
            db_statement_timeout_ms: 60_000,
            db_max_connections: 8,
            bind: "0.0.0.0:3300".into(),
            network: Network::Mainnet,
        };
        assert_eq!(c.redacted_db_url(), "postgres://user@localhost/db");
    }
}
