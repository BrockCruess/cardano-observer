//! End-to-end API tests against a scratch PostgreSQL instance carrying a
//! minimal db-sync-shaped schema. Skipped unless TEST_DB_URL is set, e.g.:
//!
//! ```bash
//! docker run -d --rm -e POSTGRES_USER=cexplorer -e POSTGRES_PASSWORD=test \
//!   -e POSTGRES_DB=cexplorer -p 55432:5432 postgres:16-alpine
//! TEST_DB_URL=postgres://cexplorer:test@127.0.0.1:55432/cexplorer \
//!   cargo test -p cardano-observer-backend --test api
//! ```

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use bech32::{Bech32, Hrp};
use cardano_observer_backend::config::{Config, Network};
use cardano_observer_backend::{ids, routes, AppState};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn bech(hrp: &str, data: &[u8]) -> String {
    bech32::encode::<Bech32>(Hrp::parse(hrp).unwrap(), data).unwrap()
}

struct Ids {
    pool_a: String,
    pool_b: String,
    pool_c: String,
    stake: String,
    drep_x: String,
    drep_y: String,
    drep_z: String,
}

fn test_ids() -> Ids {
    Ids {
        pool_a: bech("pool", &[0xA1; 28]),
        pool_b: bech("pool", &[0xB2; 28]),
        pool_c: bech("pool", &[0xC3; 28]),
        stake: bech("stake", &{
            let mut v = vec![0xE1];
            v.extend([0x44; 28]);
            v
        }),
        drep_x: bech("drep", &[0x11; 28]),
        drep_y: bech("drep", &[0x22; 28]),
        drep_z: bech("drep", &[0x33; 28]),
    }
}

fn seed_sql(ids: &Ids) -> String {
    let Ids {
        pool_a,
        pool_b,
        pool_c,
        stake,
        drep_x,
        drep_y,
        drep_z,
    } = ids;
    let a1 = "a1".repeat(28);
    let b2 = "b2".repeat(28);
    let c3 = "c3".repeat(28);
    let x11 = "11".repeat(28);
    let y22 = "22".repeat(28);
    let z33 = "33".repeat(28);
    let stake_raw = format!("e1{}", "44".repeat(28));
    format!(
        r#"
INSERT INTO epoch (id, no) VALUES (1, 479), (2, 500);
INSERT INTO epoch_param (id, epoch_no, drep_activity) VALUES (1, 500, 20);

INSERT INTO pool_hash (id, hash_raw, view) VALUES
  (1, decode('{a1}', 'hex'), '{pool_a}'),
  (2, decode('{b2}', 'hex'), '{pool_b}'),
  (3, decode('{c3}', 'hex'), '{pool_c}');

INSERT INTO slot_leader (id, pool_hash_id, description) VALUES
  (1, NULL, 'Genesis slot leader'),
  (2, 1, 'Pool A');

INSERT INTO block (id, hash, epoch_no, slot_no, epoch_slot_no, block_no, previous_id,
    slot_leader_id, size, time, tx_count, vrf_key, op_cert, op_cert_counter) VALUES
  (1, decode('{oldblock}', 'hex'), 479, 100, 10, 100, NULL, 1, 512,
   '2025-12-01 00:00:00', 1, 'vrf_old', NULL, NULL),
  (2, decode('{newblock}', 'hex'), 500, 1000000, 5000, 200, 1, 2, 1024,
   '2026-01-01 00:00:00', 6, 'vrf_vk1test', decode('{opcert}', 'hex'), 12);

INSERT INTO tx (id, hash, block_id, block_index, out_sum, fee, deposit, size,
    invalid_before, invalid_hereafter, valid_contract, treasury_donation) VALUES
  (1, decode('{tx1}', 'hex'), 1, 0, 0, 0, 0, 100, NULL, NULL, TRUE, 0),
  (2, decode('{tx2}', 'hex'), 2, 0, 0, 170000, 0, 300, NULL, NULL, TRUE, 0),
  (3, decode('{tx3}', 'hex'), 2, 1, 0, 171000, 0, 300, NULL, NULL, TRUE, 0),
  (4, decode('{tx4}', 'hex'), 2, 2, 0, 172000, 500000000, 300, NULL, NULL, TRUE, 0),
  (5, decode('{tx5}', 'hex'), 2, 3, 0, 173000, -500000000, 300, NULL, NULL, TRUE, 0),
  (6, decode('{tx6}', 'hex'), 2, 4, 9000000, 180000, 2000000, 400, NULL, NULL, TRUE, 0),
  (7, decode('{tx7}', 'hex'), 2, 5, 8000000, 200000, 0, 512, NULL, 123456789, TRUE, 0);

INSERT INTO pool_update (id, hash_id, cert_index, vrf_key_hash, pledge, active_epoch_no,
    meta_id, margin, fixed_cost, registered_tx_id) VALUES
  (1, 1, 0, NULL, 1000000000, 501, 1, 0.03, 340000000, 2),
  (2, 2, 1, NULL, 2000000000, 501, NULL, 0.05, 340000000, 2),
  (3, 3, 2, NULL, 3000000000, 501, 2, 0.05, 340000000, 2);

INSERT INTO pool_retire (id, hash_id, cert_index, announced_tx_id, retiring_epoch) VALUES
  (1, 2, 0, 3, 490),
  (2, 3, 1, 3, 999);

INSERT INTO pool_metadata_ref (id, pool_id, url, hash, registered_tx_id) VALUES
  (1, 1, 'https://example.com/pool.json', decode('{poolmeta}', 'hex'), 2),
  (2, 3, 'https://example.com/broken.json', decode('{brokenmeta}', 'hex'), 2);

INSERT INTO off_chain_pool_fetch_error (id, pool_id, fetch_time, pmr_id, fetch_error, retry_count)
  VALUES (1, 3, '2026-01-01 00:00:00', 2,
    'Hash mismatch when fetching metadata from https://example.com/broken.json', 3);

INSERT INTO off_chain_pool_data (id, pool_id, ticker_name, hash, json, bytes, pmr_id) VALUES
  (1, 1, 'TEST', decode('{poolmeta}', 'hex'),
   '{{"name":"Test Pool","description":"A test pool","homepage":"https://example.com"}}',
   decode('00', 'hex'), 1);

INSERT INTO stake_address (id, hash_raw, view) VALUES
  (1, decode('{stake_raw}', 'hex'), '{stake}');

INSERT INTO stake_registration (id, addr_id, cert_index, tx_id) VALUES (1, 1, 0, 6);

INSERT INTO delegation (id, addr_id, cert_index, pool_hash_id, active_epoch_no, tx_id) VALUES
  (1, 1, 0, 1, 501, 6);

INSERT INTO drep_hash (id, raw, view, has_script) VALUES
  (1, decode('{x11}', 'hex'), '{drep_x}', FALSE),
  (2, decode('{y22}', 'hex'), '{drep_y}', FALSE),
  (3, decode('{z33}', 'hex'), '{drep_z}', FALSE),
  (4, NULL, 'drep_always_abstain', FALSE);

INSERT INTO voting_anchor (id, url, data_hash, type) VALUES
  (1, 'https://example.com/drep.jsonld', decode('{anchor1}', 'hex'), 'other'),
  (2, 'https://example.com/proposal.jsonld', decode('{anchor2}', 'hex'), 'gov_action');

INSERT INTO off_chain_vote_data (id, voting_anchor_id, hash, json, bytes) VALUES
  (1, 1, NULL, '{{"body":{{"givenName":"Test DRep"}}}}', decode('00', 'hex')),
  (2, 2, NULL, '{{"body":{{"title":"Test Proposal"}}}}', decode('00', 'hex'));

INSERT INTO drep_registration (id, tx_id, cert_index, deposit, drep_hash_id, voting_anchor_id) VALUES
  (1, 4, 0, 500000000, 1, 1),
  (2, 4, 1, 500000000, 2, NULL),
  (3, 5, 0, -500000000, 2, NULL),
  (4, 1, 0, 500000000, 3, NULL);

INSERT INTO delegation_vote (id, addr_id, cert_index, drep_hash_id, tx_id) VALUES
  (1, 1, 1, 1, 6);

INSERT INTO voting_procedure (id, tx_id, index, drep_voter, vote) VALUES
  (1, 4, 0, 1, 'Yes');

INSERT INTO drep_distr (id, hash_id, amount, epoch_no) VALUES (1, 1, 7000000, 500);

INSERT INTO gov_action_proposal (id, tx_id, index, deposit, voting_anchor_id, type) VALUES
  (1, 4, 0, 100000000000, 2, 'InfoAction');

INSERT INTO tx_out (id, tx_id, index, address, value, stake_address_id, data_hash,
    inline_datum_id, reference_script_id, consumed_by_tx_id) VALUES
  (1, 6, 0, 'addr1qsrc', 9000000, 1, NULL, NULL, NULL, 7),
  (2, 7, 0, 'addr1qtest0', 5000000, 1, NULL, NULL, NULL, NULL),
  (3, 7, 1, 'addr1qtest1', 3000000, 1, NULL, NULL, NULL, NULL);

INSERT INTO tx_in (id, tx_in_id, tx_out_id, tx_out_index) VALUES (1, 7, 6, 0);

INSERT INTO multi_asset (id, policy, name) VALUES
  (1, decode('{policy}', 'hex'), decode('544f4b', 'hex'));

INSERT INTO ma_tx_out (id, ident, quantity, tx_out_id) VALUES (1, 1, 42, 2);

INSERT INTO reward (addr_id, type, amount, earned_epoch, spendable_epoch) VALUES
  (1, 'member', 1000000, 498, 500),
  (1, 'refund', 100000, 498, 500);

INSERT INTO reward_rest (addr_id, type, amount, earned_epoch, spendable_epoch) VALUES
  (1, 'proposal_refund', 50000, 499, 500);

INSERT INTO withdrawal (id, addr_id, amount, tx_id) VALUES (1, 1, 300000, 6);
INSERT INTO treasury (id, addr_id, amount, tx_id) VALUES (1, 1, 10000, 2);
INSERT INTO reserve (id, addr_id, amount, tx_id) VALUES (1, 1, 20000, 2);
"#,
        oldblock = "f1".repeat(32),
        newblock = "f2".repeat(32),
        opcert = "cc".repeat(32),
        tx1 = "01".repeat(32),
        tx2 = "02".repeat(32),
        tx3 = "03".repeat(32),
        tx4 = "04".repeat(32),
        tx5 = "05".repeat(32),
        tx6 = "06".repeat(32),
        tx7 = "07".repeat(32),
        poolmeta = "d1".repeat(32),
        brokenmeta = "d2".repeat(32),
        anchor1 = "e2".repeat(32),
        anchor2 = "e3".repeat(32),
        policy = "aa".repeat(28),
    )
}

async fn setup() -> Option<Router> {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let Ok(url) = std::env::var("TEST_DB_URL") else {
        eprintln!("TEST_DB_URL not set - skipping live API tests");
        return None;
    };
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to TEST_DB_URL");
    sqlx::raw_sql(include_str!("fixtures/schema.sql"))
        .execute(&db)
        .await
        .expect("apply schema");
    sqlx::raw_sql(&seed_sql(&test_ids()))
        .execute(&db)
        .await
        .expect("seed data");
    let state = AppState {
        db,
        config: Arc::new(Config {
            db_url: url,
            db_statement_timeout_ms: 60_000,
            db_max_connections: 4,
            bind: String::new(),
            network: Network::Mainnet,
        }),
    };
    Some(routes::router(state))
}

async fn get(app: &Router, path: &str, headers: &[(&str, &str)]) -> (u16, Value) {
    let mut req = Request::builder().uri(path).method("GET");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

#[tokio::test]
async fn api_endpoints() {
    let Some(app) = setup().await else { return };
    let ids = test_ids();
    let tx6 = "06".repeat(32);
    let tx7 = "07".repeat(32);

    // Service identity and health.
    let (status, body) = get(&app, "/", &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["name"], "cardano-observer-backend");
    assert_eq!(body["network"], "mainnet");
    let (status, body) = get(&app, "/health", &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["is_healthy"], true);
    let (status, body) = get(&app, "/nope", &[]).await;
    assert_eq!(status, 404);
    assert_eq!(body["status_code"], 404);

    // Latest block.
    let (status, body) = get(&app, "/blocks/latest", &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["height"], 200);
    assert_eq!(body["hash"], "f2".repeat(32));
    assert_eq!(body["slot_leader"], ids.pool_a);
    assert_eq!(body["epoch"], 500);

    // Pool list: retired pool B excluded, future-retirement pool C included.
    let (status, body) = get(&app, "/pools", &[]).await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        serde_json::json!([ids.pool_a.clone(), ids.pool_c.clone()])
    );
    let (_, body) = get(&app, "/pools?count=1&page=2", &[]).await;
    assert_eq!(body, serde_json::json!([ids.pool_c.clone()]));
    let (_, body) = get(&app, "/pools", &[("unpaged", "true")]).await;
    assert_eq!(body.as_array().unwrap().len(), 2);

    // Extended pool list with embedded metadata.
    let (status, body) = get(&app, "/pools/extended", &[]).await;
    assert_eq!(status, 200);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let a = &rows[0];
    assert_eq!(a["pool_id"], ids.pool_a);
    assert_eq!(a["hex"], "a1".repeat(28));
    assert_eq!(a["margin_cost"], 0.03);
    assert_eq!(a["fixed_cost"], "340000000");
    assert_eq!(a["declared_pledge"], "1000000000");
    assert_eq!(a["metadata"]["ticker"], "TEST");
    assert_eq!(a["metadata"]["name"], "Test Pool");
    assert_eq!(a["metadata"]["homepage"], "https://example.com");
    assert!(a["metadata"].get("error").is_none());
    assert!(a["metadata"].get("fetch_error").is_none());
    // The bulk list carries ticker/name/homepage only; the off-chain fetch
    // error stays on the single-pool endpoint (asserted below).
    let c = &rows[1];
    assert_eq!(c["pool_id"], ids.pool_c);
    assert_eq!(c["metadata"]["ticker"], Value::Null);
    assert!(c["metadata"].get("error").is_none());

    // Per-pool metadata: bech32 and hex forms, empty-metadata pool, 404, 400.
    let (status, body) = get(&app, &format!("/pools/{}/metadata", ids.pool_a), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["pool_id"], ids.pool_a);
    assert_eq!(body["ticker"], "TEST");
    assert_eq!(body["name"], "Test Pool");
    assert_eq!(body["description"], "A test pool");
    assert_eq!(body["url"], "https://example.com/pool.json");
    assert_eq!(body["hash"], "d1".repeat(32));
    let (status, hex_body) =
        get(&app, &format!("/pools/{}/metadata", "a1".repeat(28)), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(hex_body["pool_id"], ids.pool_a);
    let (status, body) = get(&app, &format!("/pools/{}/metadata", ids.pool_b), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body, serde_json::json!({}));
    let (status, body) = get(&app, &format!("/pools/{}/metadata", ids.pool_c), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["url"], "https://example.com/broken.json");
    assert_eq!(body["ticker"], Value::Null);
    assert_eq!(body["error"]["code"], "HASH_MISMATCH");
    let unknown_pool = bech("pool", &[0x77; 28]);
    let (status, _) = get(&app, &format!("/pools/{unknown_pool}/metadata"), &[]).await;
    assert_eq!(status, 404);
    let (status, _) = get(&app, "/pools/garbage/metadata", &[]).await;
    assert_eq!(status, 400);

    // DRep list: all rows, then the active-only filter the observer uses.
    let (status, body) = get(&app, "/governance/dreps", &[]).await;
    assert_eq!(status, 200);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let (x_cip129, x_hex) = ids::drep_view_to_cip129(&ids.drep_x, false);
    assert_eq!(rows[0]["drep_id"], x_cip129);
    assert_eq!(rows[0]["hex"], x_hex.unwrap());
    assert_eq!(rows[0]["amount"], "7000000");
    assert_eq!(rows[0]["retired"], false);
    assert_eq!(rows[0]["expired"], false);
    assert_eq!(rows[0]["last_active_epoch"], 500);
    assert_eq!(
        rows[0]["metadata"]["json_metadata"]["body"]["givenName"],
        "Test DRep"
    );
    assert_eq!(rows[0]["metadata"]["url"], "https://example.com/drep.jsonld");
    assert_eq!(rows[1]["retired"], true);
    assert_eq!(rows[2]["expired"], true);
    assert_eq!(rows[3]["drep_id"], "drep_always_abstain");
    assert_eq!(rows[3]["metadata"], Value::Null);

    let (status, body) = get(
        &app,
        "/governance/dreps?retired=false&expired=false",
        &[("unpaged", "true")],
    )
    .await;
    assert_eq!(status, 200);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["drep_id"], x_cip129);
    assert_eq!(rows[1]["drep_id"], "drep_always_abstain");
    let (status, _) = get(&app, "/governance/dreps?retired=sideways", &[]).await;
    assert_eq!(status, 400);

    // DRep metadata in both id forms.
    let (status, body) = get(
        &app,
        &format!("/governance/dreps/{}/metadata", ids.drep_x),
        &[],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["drep_id"], ids.drep_x);
    assert_eq!(body["json_metadata"]["body"]["givenName"], "Test DRep");
    assert_eq!(body["url"], "https://example.com/drep.jsonld");
    assert_eq!(body["hash"], "e2".repeat(32));
    let (status, body) = get(
        &app,
        &format!("/governance/dreps/{x_cip129}/metadata"),
        &[],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["drep_id"], x_cip129);
    assert_eq!(body["json_metadata"]["body"]["givenName"], "Test DRep");
    let (status, _) = get(
        &app,
        &format!("/governance/dreps/{}/metadata", ids.drep_y),
        &[],
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = get(&app, "/governance/dreps/junk/metadata", &[]).await;
    assert_eq!(status, 400);

    // Governance action metadata.
    let tx4 = "04".repeat(32);
    let (status, body) = get(
        &app,
        &format!("/governance/proposals/{tx4}/0/metadata"),
        &[],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["tx_hash"], tx4);
    assert_eq!(body["cert_index"], 0);
    assert_eq!(body["json_metadata"]["body"]["title"], "Test Proposal");
    assert_eq!(body["url"], "https://example.com/proposal.jsonld");
    assert_eq!(body["id"], ids::gov_action_id(&tx4, 0).unwrap());
    let (status, _) = get(
        &app,
        &format!("/governance/proposals/{tx4}/1/metadata"),
        &[],
    )
    .await;
    assert_eq!(status, 404);

    // Account summary.
    let (status, body) = get(&app, &format!("/accounts/{}", ids.stake), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["stake_address"], ids.stake);
    assert_eq!(body["active"], true);
    assert_eq!(body["registered"], true);
    assert_eq!(body["active_epoch"], 500);
    assert_eq!(body["pool_id"], ids.pool_a);
    assert_eq!(body["drep_id"], x_cip129);
    assert_eq!(body["controlled_amount"], "8850000");
    assert_eq!(body["rewards_sum"], "1150000");
    assert_eq!(body["withdrawals_sum"], "300000");
    assert_eq!(body["withdrawable_amount"], "850000");
    assert_eq!(body["reserves_sum"], "20000");
    assert_eq!(body["treasury_sum"], "10000");
    let unknown_stake = bech("stake", &{
        let mut v = vec![0xE1];
        v.extend([0x55; 28]);
        v
    });
    let (status, _) = get(&app, &format!("/accounts/{unknown_stake}"), &[]).await;
    assert_eq!(status, 404);
    let (status, _) = get(&app, "/accounts/garbage", &[]).await;
    assert_eq!(status, 400);

    // Delegation history.
    let (status, body) = get(
        &app,
        &format!("/accounts/{}/delegations?count=5&order=desc", ids.stake),
        &[],
    )
    .await;
    assert_eq!(status, 200);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["pool_id"], ids.pool_a);
    assert_eq!(rows[0]["tx_hash"], tx6);
    assert_eq!(rows[0]["active_epoch"], 501);
    assert_eq!(rows[0]["amount"], "9000000");
    assert_eq!(rows[0]["tx_slot"], 1000000);
    assert_eq!(rows[0]["block_height"], 200);

    // Transaction summary.
    let (status, body) = get(&app, &format!("/txs/{tx7}"), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["hash"], tx7);
    assert_eq!(body["block"], "f2".repeat(32));
    assert_eq!(body["block_height"], 200);
    assert_eq!(body["slot"], 1000000);
    assert_eq!(body["index"], 5);
    assert_eq!(body["fees"], "200000");
    assert_eq!(body["invalid_hereafter"], "123456789");
    assert_eq!(body["utxo_count"], 3);
    assert_eq!(body["valid_contract"], true);
    let amounts = body["output_amount"].as_array().unwrap();
    assert_eq!(amounts[0]["unit"], "lovelace");
    assert_eq!(amounts[0]["quantity"], "8000000");
    assert_eq!(
        amounts[1]["unit"],
        format!("{}544f4b", "aa".repeat(28))
    );
    assert_eq!(amounts[1]["quantity"], "42");
    let (_, body) = get(&app, &format!("/txs/{tx6}"), &[]).await;
    assert_eq!(body["delegation_count"], 1);
    assert_eq!(body["stake_cert_count"], 1);
    assert_eq!(body["withdrawal_count"], 1);
    let (status, _) = get(&app, &format!("/txs/{}", "99".repeat(32)), &[]).await;
    assert_eq!(status, 404);
    let (status, _) = get(&app, "/txs/nothex", &[]).await;
    assert_eq!(status, 400);

    // Transaction utxos.
    let (status, body) = get(&app, &format!("/txs/{tx7}/utxos"), &[]).await;
    assert_eq!(status, 200);
    assert_eq!(body["hash"], tx7);
    let inputs = body["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["address"], "addr1qsrc");
    assert_eq!(inputs[0]["tx_hash"], tx6);
    assert_eq!(inputs[0]["output_index"], 0);
    assert_eq!(inputs[0]["collateral"], false);
    assert_eq!(inputs[0]["amount"][0]["quantity"], "9000000");
    let outputs = body["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["address"], "addr1qtest0");
    assert_eq!(outputs[0]["amount"][0]["quantity"], "5000000");
    assert_eq!(outputs[0]["amount"][1]["quantity"], "42");
    assert_eq!(outputs[1]["amount"][0]["quantity"], "3000000");
    let (_, body) = get(&app, &format!("/txs/{tx6}/utxos"), &[]).await;
    assert_eq!(body["outputs"][0]["consumed_by_tx"], tx7);
}
