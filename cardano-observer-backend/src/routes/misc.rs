//! Service identity, health probes, and the latest-block summary.

use crate::error::ApiError;
use crate::rows::RowExt;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub async fn root(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "network": state.config.network.as_str(),
    }))
}

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let ok = tokio::time::timeout(
        Duration::from_secs(5),
        sqlx::query("SELECT 1").execute(&state.db),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);
    Json(json!({ "is_healthy": ok }))
}

pub async fn health_clock() -> Json<Value> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Json(json!({ "server_time": ms }))
}

pub async fn not_found() -> ApiError {
    ApiError::NotFound
}

const LATEST_BLOCK_SQL: &str = r#"
SELECT EXTRACT(EPOCH FROM b.time)::BIGINT AS time,
  b.block_no::INTEGER AS height,
  encode(b.hash, 'hex') AS hash,
  b.slot_no::BIGINT AS slot,
  b.epoch_no::INTEGER AS epoch,
  b.epoch_slot_no::BIGINT AS epoch_slot,
  COALESCE(ph.view, sl.description) AS slot_leader,
  b.size::INTEGER AS size,
  b.tx_count::INTEGER AS tx_count,
  (SELECT SUM(t.out_sum) FROM tx t WHERE t.block_id = b.id)::TEXT AS output,
  (SELECT SUM(t.fee) FROM tx t WHERE t.block_id = b.id)::TEXT AS fees,
  b.vrf_key AS block_vrf,
  encode(b.op_cert, 'hex') AS op_cert,
  b.op_cert_counter::TEXT AS op_cert_counter,
  (SELECT encode(p.hash, 'hex') FROM block p WHERE p.id = b.previous_id) AS previous_block
FROM block b
  JOIN slot_leader sl ON sl.id = b.slot_leader_id
  LEFT JOIN pool_hash ph ON ph.id = sl.pool_hash_id
ORDER BY b.id DESC
LIMIT 1
"#;

pub async fn blocks_latest(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(LATEST_BLOCK_SQL)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({
        "time": row.int8("time"),
        "height": row.int4("height"),
        "hash": row.s("hash"),
        "slot": row.int8("slot"),
        "epoch": row.int4("epoch"),
        "epoch_slot": row.int8("epoch_slot"),
        "slot_leader": row.s("slot_leader"),
        "size": row.int4("size"),
        "tx_count": row.int4("tx_count"),
        "output": row.s("output"),
        "fees": row.s("fees"),
        "block_vrf": row.s("block_vrf"),
        "op_cert": row.s("op_cert"),
        "op_cert_counter": row.s("op_cert_counter"),
        "previous_block": row.s("previous_block"),
        "next_block": Value::Null,
        "confirmations": 0,
    })))
}
