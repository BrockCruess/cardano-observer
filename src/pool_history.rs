//! Pool registration history: tells an initial registration apart from a
//! re-registration and works out what actually changed.
//!
//! A `stakePoolRegistration` certificate looks identical whether an operator is
//! registering for the first time or just resubmitting updated parameters, so
//! the certificate alone can't say which it is. On first sight of one we ask the
//! backend for that pool's earlier registrations; when one exists the event is
//! retitled "Pool Registration Update" and carries a `changes` list describing
//! the differences. Registrations are rare, so this runs once per event.
//!
//! Without a backend configured, nothing is looked up and the event stays a
//! plain "Pool Registration".

use crate::config::Network;
use crate::model::ChainEvent;
use crate::parse;
use serde_json::{json, Value};
use std::time::Duration;

/// Margins within this of each other count as unchanged (ratio vs float).
const MARGIN_EPSILON: f64 = 1e-9;

/// Attempts to read a pool's history before giving up. Registrations are rare,
/// so a couple of quick retries are cheaper than mislabelling a re-registration
/// as a first registration because the backend blipped.
const HISTORY_ATTEMPTS: usize = 3;
/// Pause after each failed attempt (the last attempt has none).
const HISTORY_BACKOFF_MS: [u64; 2] = [250, 750];

/// What the backend could tell us about a pool's earlier registrations.
enum History {
    /// The registration that preceded the one being reported.
    Previous(Box<Value>),
    /// The backend answered: this pool has no earlier registration.
    FirstEver,
    /// The backend could not be reached, so nothing can be concluded.
    Unavailable,
}

/// Retitle pool-registration events that follow an earlier registration, and
/// attach the list of changed parameters.
pub async fn stamp_registration_updates(
    http: &reqwest::Client,
    backend_url: Option<&str>,
    network: Network,
    events: &mut [ChainEvent],
) {
    let Some(base) = backend_url else { return };
    for ev in events.iter_mut() {
        if ev.kind != "pool_registration" {
            continue;
        }
        let Some(pool) = ev.data.get("pool").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        // Retry only when the backend itself was unreachable - an answered
        // "no earlier registration" is final and must not be retried, or every
        // genuine first registration would stall.
        let mut found = None;
        for attempt in 0..HISTORY_ATTEMPTS {
            match fetch_previous(http, base, &pool, ev.tx_hash.as_deref()).await {
                History::Previous(p) => {
                    found = Some(*p);
                    break;
                }
                History::FirstEver => break,
                History::Unavailable => match HISTORY_BACKOFF_MS.get(attempt) {
                    Some(ms) => {
                        tracing::debug!("pool history {pool}: unavailable, retrying in {ms}ms");
                        tokio::time::sleep(Duration::from_millis(*ms)).await;
                    }
                    None => tracing::warn!(
                        "pool history {pool}: backend unavailable after {HISTORY_ATTEMPTS} \
                         attempts - shown as a first registration"
                    ),
                },
            }
        }
        let Some(previous) = found else {
            continue; // first registration (or history still unavailable)
        };
        let changes = diff(&ev.data, &previous, network);
        ev.title = "Pool Registration Update".into();
        if let Some(obj) = ev.data.as_object_mut() {
            obj.insert("update".into(), json!(true));
            obj.insert("changes".into(), json!(changes));
        }
    }
}

/// The pool's most recent registration *before* `current_tx`.
///
/// A lagging backend is harmless here: the row we want is the *earlier*
/// certificate, which was indexed long ago. Whether or not the one being
/// reported has landed yet, the newest row that isn't `current_tx` is the
/// answer.
async fn fetch_previous(
    http: &reqwest::Client,
    base: &str,
    pool: &str,
    current_tx: Option<&str>,
) -> History {
    // `/registrations` carries each certificate's declared parameters; the
    // standard `/updates` only reports that a registration happened. List
    // endpoints default to ascending, so ask for newest-first explicitly.
    //
    // Two rows is always enough: the newest is either the certificate we are
    // reporting (when db-sync has already indexed it) or the one before it, so
    // the row we want is never deeper than the second.
    let url = format!("{base}/pools/{pool}/registrations?count=2&order=desc");
    let resp = match tokio::time::timeout(Duration::from_secs(5), http.get(&url).send()).await {
        Ok(Ok(r)) => r,
        _ => return History::Unavailable,
    };
    // An unknown pool means the backend has no registration for it at all,
    // which is an answer (a brand-new pool), not a failure.
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return History::FirstEver;
    }
    if !resp.status().is_success() {
        return History::Unavailable;
    }
    let Ok(rows) = resp.json::<Value>().await else {
        return History::Unavailable;
    };
    let Some(arr) = rows.as_array() else {
        return History::Unavailable;
    };
    match arr
        .iter()
        // The registration we are reporting may already be indexed - skip it.
        .find(|r| r.get("tx_hash").and_then(Value::as_str) != current_tx)
    {
        Some(prev) => History::Previous(Box::new(prev.clone())),
        None => History::FirstEver,
    }
}

/// Compare the freshly seen certificate against the previous registration.
/// Numeric parameters carry `from`/`to`; everything else is just flagged as new.
fn diff(new: &Value, prev: &Value, network: Network) -> Vec<Value> {
    let mut out = Vec::new();

    // Pledge / cost: lovelace, compared numerically.
    for (key, prev_key) in [("pledge", "pledge"), ("cost", "cost")] {
        let a = prev.get(prev_key).and_then(as_i128);
        let b = new.get(key).and_then(as_i128);
        if let (Some(a), Some(b)) = (a, b) {
            if a != b {
                out.push(json!({ "key": key, "from": a as f64, "to": b as f64 }));
            }
        }
    }

    // Margin: a ratio ("2/100") on-chain, a fraction in the backend.
    let prev_margin = prev.get("margin").and_then(Value::as_f64);
    let new_margin = new.get("margin").and_then(margin_fraction);
    if let (Some(a), Some(b)) = (prev_margin, new_margin) {
        if (a - b).abs() > MARGIN_EPSILON {
            out.push(json!({ "key": "margin", "from": a, "to": b }));
        }
    }

    // Metadata: either the URL or the hash moving means new metadata.
    let meta_changed = differs_str(new.get("metadataUrl"), prev.get("metadata_url"))
        || differs_str(new.get("metadataHash"), prev.get("metadata_hash"));
    if meta_changed {
        out.push(json!({ "key": "metadata" }));
    }

    // Reward account: bech32 on both sides.
    if differs_str(new.get("rewardAccount"), prev.get("reward_account")) {
        out.push(json!({ "key": "rewardAccount" }));
    }

    // VRF key: hex on both sides.
    if differs_str(new.get("vrf"), prev.get("vrf_key")) {
        out.push(json!({ "key": "vrf" }));
    }

    // Owners: raw key hashes on-chain, stake addresses in the backend.
    let new_owners = normalized_owners(new.get("owners"), network);
    let prev_owners = sorted_strings(prev.get("owners"));
    if !new_owners.is_empty() && new_owners != prev_owners {
        out.push(json!({ "key": "owners" }));
    }

    // Relays: normalise both shapes to "host:port" before comparing.
    let new_relays = normalized_relays(new.get("relays"));
    let prev_relays = normalized_relays(prev.get("relays"));
    if !new_relays.is_empty() && new_relays != prev_relays {
        out.push(json!({ "key": "relays" }));
    }

    out
}

fn as_i128(v: &Value) -> Option<i128> {
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
        .or_else(|| v.as_str().and_then(|s| s.parse::<i128>().ok()))
}

/// `"2/100"` → `0.02`; a bare number passes through.
fn margin_fraction(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    let s = v.as_str()?;
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
            (b != 0.0).then_some(a / b)
        }
        None => s.parse::<f64>().ok(),
    }
}

/// True when both sides are present and different (a missing side is not a change).
fn differs_str(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.and_then(Value::as_str), b.and_then(Value::as_str)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

fn sorted_strings(v: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = v
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// Pool owners arrive as bare stake key hashes; the backend stores bech32
/// stake addresses, so encode before comparing.
fn normalized_owners(v: Option<&Value>, network: Network) -> Vec<String> {
    let mut out: Vec<String> = v
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|o| {
                    if o.starts_with("stake") {
                        o.to_string()
                    } else {
                        parse::stake_address(o, None, network)
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Reduce either relay shape (on-chain or backend) to sorted `host:port` keys.
fn normalized_relays(v: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = v
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| {
                    let host = ["ipv4", "ipv6", "hostname", "dns", "srv", "dnsName"]
                        .iter()
                        .find_map(|k| r.get(*k).and_then(Value::as_str))
                        .unwrap_or("");
                    let port = r
                        .get("port")
                        .and_then(|p| p.as_u64().or_else(|| p.as_str()?.parse().ok()))
                        .map(|p| p.to_string())
                        .unwrap_or_default();
                    format!("{host}:{port}")
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_cert() -> Value {
        json!({
            "pool": "pool1abc",
            "pledge": 20_000_000_000u64,
            "cost": 170_000_000u64,
            "margin": "0/1",
            "metadataUrl": "https://brockpool.com/meta.json",
            "metadataHash": "602d4970a19902efa1b5cb6fa86464f4a6034ea7e365db34e2aefba4f8832752",
            "owners": ["60941e55dedd2c66815e0000e60ea5d47719de066308dc943a66f095"],
            "relays": [{ "hostname": "brockpool.ddns.net", "port": 6000, "type": "hostname" }],
            "rewardAccount": "stake1u9sfg8j4mmwjce5ptcqqpesw5h28wxw7qe3s3hy58fn0p9gzqsm42",
            "vrf": "a2d3c275f518a126e9d5d471ee4958b3fbe35bc92ff8a27d8ff4a8603ddd32ef"
        })
    }

    fn prev_from(new: &Value, network: Network) -> Value {
        // A backend row that matches the certificate exactly.
        json!({
            "pledge": new["pledge"].as_u64().unwrap().to_string(),
            "cost": new["cost"].as_u64().unwrap().to_string(),
            "margin": margin_fraction(&new["margin"]).unwrap(),
            "metadata_url": new["metadataUrl"],
            "metadata_hash": new["metadataHash"],
            "reward_account": new["rewardAccount"],
            "vrf_key": new["vrf"],
            "owners": normalized_owners(new.get("owners"), network),
            "relays": [{ "dns": "brockpool.ddns.net", "port": 6000 }],
        })
    }

    #[test]
    fn identical_resubmission_reports_no_changes() {
        let n = new_cert();
        let p = prev_from(&n, Network::Mainnet);
        assert!(diff(&n, &p, Network::Mainnet).is_empty());
    }

    #[test]
    fn numeric_changes_carry_from_and_to() {
        let n = new_cert();
        let mut p = prev_from(&n, Network::Mainnet);
        p["pledge"] = json!("30000000000");
        p["cost"] = json!("340000000");
        p["margin"] = json!(0.02);
        let d = diff(&n, &p, Network::Mainnet);
        let keys: Vec<&str> = d.iter().filter_map(|c| c["key"].as_str()).collect();
        assert_eq!(keys, vec!["pledge", "cost", "margin"]);
        assert_eq!(d[0]["from"], 30_000_000_000f64);
        assert_eq!(d[0]["to"], 20_000_000_000f64);
        assert_eq!(d[2]["from"], 0.02);
        assert_eq!(d[2]["to"], 0.0);
    }

    #[test]
    fn structural_changes_are_flagged_only() {
        let n = new_cert();
        let mut p = prev_from(&n, Network::Mainnet);
        p["metadata_url"] = json!("https://old.example/meta.json");
        p["reward_account"] = json!("stake1uxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
        p["vrf_key"] = json!("ffff");
        p["owners"] = json!(["stake1uother"]);
        p["relays"] = json!([{ "dns": "old-relay.example", "port": 3001 }]);
        let d = diff(&n, &p, Network::Mainnet);
        let keys: Vec<&str> = d.iter().filter_map(|c| c["key"].as_str()).collect();
        assert_eq!(
            keys,
            vec!["metadata", "rewardAccount", "vrf", "owners", "relays"]
        );
        // Flag-only entries carry no from/to.
        assert!(d.iter().all(|c| c.get("from").is_none()));
    }

    #[test]
    fn owner_key_hash_matches_encoded_stake_address() {
        let n = new_cert();
        let encoded = normalized_owners(n.get("owners"), Network::Mainnet);
        assert_eq!(encoded.len(), 1);
        assert!(encoded[0].starts_with("stake1"), "got {}", encoded[0]);
        // Same owner expressed as an address must not count as a change.
        let mut p = prev_from(&n, Network::Mainnet);
        p["owners"] = json!(encoded);
        assert!(diff(&n, &p, Network::Mainnet)
            .iter()
            .all(|c| c["key"] != "owners"));
    }

    #[test]
    fn margin_ratio_forms_compare_equal() {
        assert_eq!(margin_fraction(&json!("0/1")), Some(0.0));
        assert_eq!(margin_fraction(&json!("2/100")), Some(0.02));
        assert_eq!(margin_fraction(&json!(0.02)), Some(0.02));
    }

    /* ── history lookup ─────────────────────────────────────────────────── */

    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn registration_event(tx: &str) -> ChainEvent {
        ChainEvent {
            id: 0,
            parent_id: None,
            kind: "pool_registration".into(),
            category: "pool".into(),
            slot: 1,
            height: Some(1),
            block_hash: None,
            tx_hash: Some(tx.into()),
            timestamp: 1,
            title: "Pool Registration".into(),
            summary: String::new(),
            data: new_cert(),
        }
    }

    /// A backend row for the previous certificate, with a bigger pledge.
    fn prev_row(tx: &str) -> Value {
        let mut p = prev_from(&new_cert(), Network::Mainnet);
        p["tx_hash"] = json!(tx);
        p["pledge"] = json!("30000000000");
        p
    }

    async fn run(server: &MockServer, event_tx: &str) -> ChainEvent {
        let mut events = vec![registration_event(event_tx)];
        stamp_registration_updates(
            &reqwest::Client::new(),
            Some(&server.uri()),
            Network::Mainnet,
            &mut events,
        )
        .await;
        events.remove(0)
    }

    #[tokio::test]
    async fn retitles_and_diffs_when_a_previous_registration_exists() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([prev_row("older")])))
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(ev.title, "Pool Registration Update");
        assert_eq!(ev.data["update"], json!(true));
        let keys: Vec<&str> = ev.data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["key"].as_str())
            .collect();
        assert_eq!(keys, vec!["pledge"]);
    }

    #[tokio::test]
    async fn a_lagging_backend_still_finds_the_previous_registration() {
        // db-sync hasn't indexed the new certificate yet, so the newest row it
        // returns *is* the previous registration - the tx-hash skip is a no-op.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([prev_row("older")])))
            .mount(&server)
            .await;

        let ev = run(&server, "not-yet-indexed").await;
        assert_eq!(ev.title, "Pool Registration Update");
    }

    #[tokio::test]
    async fn skips_the_certificate_being_reported_when_already_indexed() {
        let server = MockServer::start().await;
        let mut own = prev_from(&new_cert(), Network::Mainnet);
        own["tx_hash"] = json!("current");
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([own, prev_row("older")])),
            )
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(ev.title, "Pool Registration Update");
        // Diffed against the older row, not against itself.
        assert_eq!(ev.data["changes"][0]["key"], "pledge");
    }

    #[tokio::test]
    async fn first_ever_registration_is_left_alone() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(ev.title, "Pool Registration");
        assert!(ev.data.get("update").is_none());
    }

    #[tokio::test]
    async fn unknown_pool_404_counts_as_a_first_registration() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(ev.title, "Pool Registration");
    }

    #[tokio::test]
    async fn a_transient_backend_error_is_retried() {
        let server = MockServer::start().await;
        // First call fails, the retry succeeds.
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([prev_row("older")])))
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(
            ev.title, "Pool Registration Update",
            "a blip must not mislabel a re-registration"
        );
    }

    #[tokio::test]
    async fn gives_up_after_the_attempt_budget() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/pools/.*/registrations$"))
            .respond_with(ResponseTemplate::new(503))
            .expect(HISTORY_ATTEMPTS as u64)
            .mount(&server)
            .await;

        let ev = run(&server, "current").await;
        assert_eq!(ev.title, "Pool Registration");
    }
}
