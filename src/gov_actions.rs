//! Durable governance-action title cache (CIP-108 `body.title`).
//!
//! Keys are `{tx_hash}#{cert_index}`. On first sight of a proposal or a vote
//! referencing one we:
//! 1. Ask Ogmios for the on-chain metadata anchor (`queryLedgerState/governanceProposals`)
//! 2. Optionally try Blockfrost `/governance/proposals/{tx}/{index}/metadata`
//! 3. Fetch the CIP-108 JSON from the anchor URL and read `body.title`
//!
//! Only confirmed results are written to `gov-actions.json` (including
//! "looked up, no title"). Network failures are not cached so we retry later.

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const CACHE_FILE: &str = "gov-actions.json";
const FETCH_CONCURRENCY: usize = 8;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GovActionEntry {
    /// CIP-108 title when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Anchor URL from Ogmios / Blockfrost / the proposal event (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl GovActionEntry {
    fn has_title(&self) -> bool {
        self.title.as_ref().is_some_and(|t| !t.is_empty())
    }

    pub fn to_json(&self, tx_hash: &str, index: u64) -> Value {
        json!({
            "tx": tx_hash,
            "index": index,
            "title": self.title,
            "url": self.url,
            "source": "cache",
        })
    }
}

pub struct GovActionCache {
    /// Presence means we already confirmed a lookup (title may still be None).
    by_key: Mutex<HashMap<String, GovActionEntry>>,
    path: Option<PathBuf>,
}

impl GovActionCache {
    pub fn titled_len(&self) -> usize {
        self.by_key
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.has_title())
            .count()
    }

    pub fn is_resolved(&self, key: &str) -> bool {
        self.by_key.lock().unwrap().contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<GovActionEntry> {
        self.by_key.lock().unwrap().get(key).cloned()
    }

    pub fn title(&self, key: &str) -> Option<String> {
        self.get(key)
            .and_then(|e| e.title)
            .filter(|s| !s.is_empty())
    }

    /// Compact dump for the browser (key → title).
    pub fn to_titles_json(&self) -> Value {
        let map = self.by_key.lock().unwrap();
        let mut out = serde_json::Map::new();
        for (k, e) in map.iter() {
            if let Some(title) = e.title.as_ref().filter(|s| !s.is_empty()) {
                out.insert(k.clone(), json!({ "title": title }));
            }
        }
        Value::Object(out)
    }

    /// Record a confirmed lookup result (success or miss) and persist.
    pub fn remember(&self, key: &str, entry: GovActionEntry) {
        let mut map = self.by_key.lock().unwrap();
        if map.contains_key(key) {
            let Some(old) = map.get(key) else { return };
            let better = match (&old.title, &entry.title) {
                (Some(a), Some(b)) if !a.is_empty() && a == b => false,
                (_, Some(b)) if !b.is_empty() => true,
                (None, _) if entry.url.is_some() && old.url != entry.url => true,
                _ => false,
            };
            if !better {
                return;
            }
        }
        map.insert(key.to_string(), entry);
        if let Some(path) = &self.path {
            let _ = save_cache(path, &map);
        }
    }

    pub fn load(cache_dir: Option<&Path>) -> Self {
        let dir = cache_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("cardano-observer"));
        let path = dir.join(CACHE_FILE);
        match load_cache(&path) {
            Ok(mut map) => {
                // Drop hollow entries from failed lookups (pre-fix caches that
                // recorded `{}` on Blockfrost timeouts) so we retry.
                let before = map.len();
                map.retain(|_, e| e.has_title() || e.url.as_ref().is_some_and(|u| !u.is_empty()));
                let dropped = before - map.len();
                if dropped > 0 {
                    tracing::info!(
                        "gov-action cache: dropped {dropped} empty entries (will re-resolve)"
                    );
                    let _ = save_cache(&path, &map);
                }
                let titled = map.values().filter(|e| e.has_title()).count();
                tracing::info!(
                    "gov-action cache: loaded {} entries ({} titled) from {}",
                    map.len(),
                    titled,
                    path.display()
                );
                GovActionCache {
                    by_key: Mutex::new(map),
                    path: Some(path),
                }
            }
            Err(_) => {
                tracing::info!(
                    "gov-action cache: no file at {} - will fill on first sight",
                    path.display()
                );
                GovActionCache {
                    by_key: Mutex::new(HashMap::new()),
                    path: Some(path),
                }
            }
        }
    }
}

pub fn cache_key(tx_hash: &str, index: u64) -> String {
    format!("{}#{index}", tx_hash.trim().to_lowercase())
}

/// Collect unique unresolved (tx, index[, optional anchor]) refs from events.
pub fn collect_refs(
    events: &[crate::model::ChainEvent],
    cache: &GovActionCache,
) -> Vec<(String, u64, Option<String>)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for ev in events {
        let (tx, index, anchor) = match ev.kind.as_str() {
            "gov_proposal" => {
                let Some(tx) = ev.tx_hash.as_deref() else {
                    continue;
                };
                let index = ev
                    .data
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let anchor = ev
                    .data
                    .get("anchorUrl")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                (tx.to_string(), index, anchor)
            }
            "gov_vote" => {
                let Some(tx) = ev
                    .data
                    .get("proposalTx")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let index = ev
                    .data
                    .get("proposalIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                (tx.to_string(), index, None)
            }
            _ => continue,
        };
        let key = cache_key(&tx, index);
        if !seen.insert(key.clone()) || cache.is_resolved(&key) {
            continue;
        }
        out.push((tx, index, anchor));
    }
    out
}

/// Fetch titles for unresolved refs and write confirmed results into the cache.
pub async fn ensure_titles(
    http: &reqwest::Client,
    blockfrost_url: Option<&str>,
    project_id: Option<&str>,
    ogmios_url: Option<&str>,
    cache: &GovActionCache,
    refs: Vec<(String, u64, Option<String>)>,
) {
    if refs.is_empty() {
        return;
    }

    tracing::info!("gov-action cache: resolving {} new proposal title(s)…", refs.len());

    // Prefer Ogmios for anchors — it is already required and returns metadata.url
    // even when Blockfrost is down or hanging.
    let mut ogmios_anchors = HashMap::new();
    if let Some(url) = ogmios_url {
        let want: Vec<(String, u64)> = refs.iter().map(|(t, i, _)| (t.clone(), *i)).collect();
        match fetch_ogmios_anchors(url, &want).await {
            Ok(map) => {
                tracing::info!("gov-action cache: Ogmios returned {} anchor(s)", map.len());
                ogmios_anchors = map;
            }
            Err(e) => tracing::warn!("gov-action cache: Ogmios anchor query failed: {e:#}"),
        }
    }

    let mut set: tokio::task::JoinSet<(String, Option<GovActionEntry>)> =
        tokio::task::JoinSet::new();
    let mut outstanding = 0usize;
    let mut learned = 0usize;
    let mut confirmed = 0usize;

    for (tx, index, event_anchor) in refs {
        while outstanding >= FETCH_CONCURRENCY {
            let Some(res) = set.join_next().await else {
                break;
            };
            outstanding -= 1;
            if let Ok((k, Some(entry))) = res {
                if entry.has_title() {
                    learned += 1;
                }
                confirmed += 1;
                cache.remember(&k, entry);
            }
        }
        let key = cache_key(&tx, index);
        let ogmios_anchor = ogmios_anchors.get(&key).cloned();
        let http = http.clone();
        let bf_base = blockfrost_url.map(str::to_string);
        let pid = project_id.map(str::to_string);
        set.spawn(async move {
            let entry = resolve_one(
                &http,
                bf_base.as_deref(),
                pid.as_deref(),
                &tx,
                index,
                event_anchor.as_deref(),
                ogmios_anchor.as_deref(),
            )
            .await;
            (key, entry)
        });
        outstanding += 1;
    }
    while let Some(res) = set.join_next().await {
        if let Ok((k, Some(entry))) = res {
            if entry.has_title() {
                learned += 1;
            }
            confirmed += 1;
            cache.remember(&k, entry);
        }
    }
    tracing::info!(
        "gov-action cache: confirmed {confirmed} lookup(s), learned {learned} title(s)"
    );
}

/// Returns `Some` only for a confirmed outcome (title found or definitive miss).
/// Returns `None` on transport failure so we retry later.
async fn resolve_one(
    http: &reqwest::Client,
    blockfrost_url: Option<&str>,
    project_id: Option<&str>,
    tx: &str,
    index: u64,
    event_anchor: Option<&str>,
    ogmios_anchor: Option<&str>,
) -> Option<GovActionEntry> {
    // Prefer Ogmios/event anchors — Blockfrost often hangs on RYO while the
    // node already has the metadata URL.
    if let Some(u) = ogmios_anchor.or(event_anchor) {
        match fetch_anchor_title(http, u).await {
            AnchorResult::Ok(entry) => return Some(entry),
            AnchorResult::Miss => {
                return Some(GovActionEntry {
                    title: None,
                    url: Some(u.to_string()),
                });
            }
            AnchorResult::Error => {
                // Fall through to Blockfrost if configured; otherwise retry later.
                if blockfrost_url.is_none() {
                    return None;
                }
            }
        }
    }

    let Some(base) = blockfrost_url else {
        return None;
    };

    match fetch_blockfrost(http, base, project_id, tx, index).await {
        BfResult::Ok(entry) if entry.has_title() => Some(entry),
        BfResult::Ok(entry) => {
            if let Some(u) = entry.url.as_deref().or(ogmios_anchor).or(event_anchor) {
                match fetch_anchor_title(http, u).await {
                    AnchorResult::Ok(from_anchor) => Some(from_anchor),
                    AnchorResult::Miss => Some(GovActionEntry {
                        title: entry.title,
                        url: Some(u.to_string()),
                    }),
                    AnchorResult::Error => None,
                }
            } else {
                Some(entry) // BF responded; no title, no URL
            }
        }
        BfResult::Miss => Some(GovActionEntry::default()),
        BfResult::Error => None,
    }
}

enum BfResult {
    Ok(GovActionEntry),
    Miss,
    Error,
}

async fn fetch_blockfrost(
    http: &reqwest::Client,
    base: &str,
    project_id: Option<&str>,
    tx: &str,
    index: u64,
) -> BfResult {
    let path = format!("/governance/proposals/{tx}/{index}/metadata");
    let mut req = http.get(format!("{base}{path}"));
    if let Some(pid) = project_id {
        req = req.header("project_id", pid);
    }
    let resp = match tokio::time::timeout(Duration::from_secs(5), req.send()).await {
        Ok(Ok(r)) => r,
        _ => return BfResult::Error,
    };
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return BfResult::Miss;
    }
    if !resp.status().is_success() {
        return BfResult::Error;
    }
    match resp.json::<Value>().await {
        Ok(v) => BfResult::Ok(entry_from_blockfrost(&v)),
        Err(_) => BfResult::Error,
    }
}

fn entry_from_blockfrost(v: &Value) -> GovActionEntry {
    let title = extract_title(v.get("json_metadata").unwrap_or(v));
    let url = v
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    GovActionEntry { title, url }
}

fn extract_title(meta: &Value) -> Option<String> {
    let raw = meta
        .get("body")
        .and_then(|b| b.get("title"))
        .and_then(Value::as_str)
        .or_else(|| meta.get("title").and_then(Value::as_str))
        .filter(|s| !s.is_empty())?;
    let trimmed: String = raw.chars().take(120).collect();
    let trimmed = trimmed.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

enum AnchorResult {
    Ok(GovActionEntry),
    Miss,
    Error,
}

async fn fetch_anchor_title(http: &reqwest::Client, url: &str) -> AnchorResult {
    let Some(resolved) = resolve_anchor_url(url) else {
        return AnchorResult::Miss;
    };
    let resp = match tokio::time::timeout(Duration::from_secs(10), http.get(&resolved).send()).await
    {
        Ok(Ok(r)) => r,
        _ => return AnchorResult::Error,
    };
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return AnchorResult::Miss;
    }
    if !resp.status().is_success() {
        return AnchorResult::Error;
    }
    let v = match resp.json::<Value>().await {
        Ok(v) => v,
        Err(_) => return AnchorResult::Miss,
    };
    let title = extract_title(&v);
    if title.is_some() {
        AnchorResult::Ok(GovActionEntry {
            title,
            url: Some(url.to_string()),
        })
    } else {
        AnchorResult::Miss
    }
}

fn resolve_anchor_url(url: &str) -> Option<String> {
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

/// Fetch a vote's pinned CIP-100 / CIP-136 JSON and return display fields.
///
/// Used by the tx-detail modal. Network / parse failures return `None` so the
/// UI can show a soft miss rather than an error page.
pub async fn fetch_vote_rationale(http: &reqwest::Client, url: &str) -> Option<Value> {
    let resolved = resolve_anchor_url(url)?;
    let resp = tokio::time::timeout(Duration::from_secs(12), http.get(&resolved).send())
        .await
        .ok()?
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // Cap body so a huge IPFS blob can't blow up the modal request.
    let bytes = tokio::time::timeout(Duration::from_secs(12), resp.bytes())
        .await
        .ok()?
        .ok()?;
    if bytes.len() > 512 * 1024 {
        return None;
    }
    let doc: Value = serde_json::from_slice(&bytes).ok()?;
    Some(extract_vote_rationale(&doc, url, &resolved))
}

fn extract_vote_rationale(doc: &Value, url: &str, resolved: &str) -> Value {
    let body = doc.get("body").unwrap_or(doc);
    let mut out = serde_json::Map::new();
    out.insert("url".into(), json!(url));
    out.insert("resolvedUrl".into(), json!(resolved));

    for key in [
        "comment",
        "summary",
        "rationaleStatement",
        "precedentDiscussion",
        "counterargumentDiscussion",
        "conclusion",
    ] {
        if let Some(s) = jsonld_str(body.get(key)) {
            out.insert(key.into(), json!(s));
        }
    }

    if let Some(authors) = doc.get("authors").and_then(Value::as_array) {
        let names: Vec<String> = authors
            .iter()
            .filter_map(|a| jsonld_str(a.get("name")))
            .collect();
        if !names.is_empty() {
            out.insert("authors".into(), json!(names));
        }
    }

    if let Some(refs) = body.get("references").and_then(Value::as_array) {
        let items: Vec<Value> = refs
            .iter()
            .filter_map(|r| {
                let label = jsonld_str(r.get("label")).unwrap_or_default();
                let uri = jsonld_str(r.get("uri"))?;
                let ty = r
                    .get("@type")
                    .or_else(|| r.get("type"))
                    .and_then(|t| jsonld_str(Some(t)));
                Some(json!({
                    "label": if label.is_empty() { Value::Null } else { json!(label) },
                    "uri": uri,
                    "type": ty,
                }))
            })
            .collect();
        if !items.is_empty() {
            out.insert("references".into(), json!(items));
        }
    }

    if let Some(iv) = body.get("internalVote") {
        out.insert("internalVote".into(), iv.clone());
    }

    Value::Object(out)
}

/// Unwrap plain strings and JSON-LD `{ "@value": "…" }` nodes.
fn jsonld_str(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let s = v
        .as_str()
        .or_else(|| v.get("@value").and_then(Value::as_str))?
        .trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Ask Ogmios for metadata anchors of the given proposals.
async fn fetch_ogmios_anchors(
    ogmios_url: &str,
    refs: &[(String, u64)],
) -> Result<HashMap<String, String>> {
    if refs.is_empty() {
        return Ok(HashMap::new());
    }
    let proposals: Vec<Value> = refs
        .iter()
        .map(|(tx, index)| {
            json!({
                "transaction": { "id": tx },
                "index": index,
            })
        })
        .collect();

    let (mut ws, _) = tokio_tungstenite::connect_async(ogmios_url)
        .await
        .context("ogmios connect for gov-action anchors")?;

    let req = json!({
        "jsonrpc": "2.0",
        "method": "queryLedgerState/governanceProposals",
        "params": { "proposals": proposals },
        "id": "gov-anchors",
    });
    ws.send(Message::Text(req.to_string().into()))
        .await
        .context("send governanceProposals")?;

    let raw = tokio::time::timeout(Duration::from_secs(20), ws.next())
        .await
        .context("timeout waiting for governanceProposals")?
        .ok_or_else(|| anyhow!("ogmios closed during governanceProposals"))??;
    let text = match raw {
        Message::Text(t) => t.to_string(),
        other => return Err(anyhow!("unexpected ogmios message: {other:?}")),
    };
    let msg: Value = serde_json::from_str(&text).context("decode governanceProposals")?;
    if let Some(err) = msg.get("error") {
        return Err(anyhow!("ogmios governanceProposals error: {err}"));
    }
    let Some(arr) = msg.get("result").and_then(Value::as_array) else {
        return Ok(HashMap::new());
    };

    let mut out = HashMap::new();
    for row in arr {
        let tx = row
            .pointer("/proposal/transaction/id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let index = row
            .pointer("/proposal/index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let url = row
            .pointer("/metadata/url")
            .or_else(|| row.pointer("/proposal/metadata/url"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        if let Some(url) = url {
            if !tx.is_empty() {
                out.insert(cache_key(tx, index), url.to_string());
            }
        }
    }
    Ok(out)
}

fn load_cache(path: &Path) -> Result<HashMap<String, GovActionEntry>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).context("parse gov-action cache")
}

fn save_cache(path: &Path, map: &HashMap<String, GovActionEntry>) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(map)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cip100_comment_and_cip136_fields() {
        let doc = json!({
            "authors": [{ "name": "Ada" }],
            "body": {
                "comment": "plain comment",
                "summary": { "@value": "short summary" },
                "rationaleStatement": "longer **text**",
                "references": [{
                    "@type": "Other",
                    "label": "PDF",
                    "uri": "https://example.com/a.pdf"
                }]
            }
        });
        let out = extract_vote_rationale(&doc, "ipfs://QmTest", "https://ipfs.io/ipfs/QmTest");
        assert_eq!(out["comment"], "plain comment");
        assert_eq!(out["summary"], "short summary");
        assert_eq!(out["rationaleStatement"], "longer **text**");
        assert_eq!(out["authors"][0], "Ada");
        assert_eq!(out["references"][0]["uri"], "https://example.com/a.pdf");
        assert_eq!(out["url"], "ipfs://QmTest");
    }
}
