//! Stake account endpoints: the account summary (registration state, balances,
//! current pool and DRep delegation) and the delegation history list.

use crate::error::ApiError;
use crate::ids;
use crate::pagination::{Page, PageParams};
use crate::rows::RowExt;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};

/// Resolves a stake address to its database id, validating the bech32 form
/// first. Unknown (but well-formed) addresses map to a 404.
async fn resolve_stake_address(
    state: &AppState,
    stake_address: &str,
) -> Result<i64, ApiError> {
    if !ids::is_valid_stake_address(stake_address, state.config.network.stake_hrp()) {
        return Err(ApiError::bad_request(
            "Invalid or malformed stake address format.",
        ));
    }
    let row = sqlx::query("SELECT id FROM stake_address WHERE view = $1")
        .bind(stake_address)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    row.int8("id").ok_or(ApiError::NotFound)
}

const ACCOUNT_SQL: &str = r#"
WITH now_epoch AS (
  SELECT b.epoch_no FROM block b ORDER BY b.id DESC LIMIT 1
),
regs AS (
  SELECT
    COALESCE((SELECT MAX(tx_id) FROM stake_registration WHERE addr_id = $1), 0) AS last_reg,
    COALESCE((SELECT MAX(tx_id) FROM stake_deregistration WHERE addr_id = $1), 0) AS last_dereg
),
-- Current pool delegation: the newest delegation certificate, valid only while
-- the account's latest registration is newer than any deregistration.
pool AS (
  SELECT ph.view AS pool_id
  FROM delegation d
    JOIN pool_hash ph ON ph.id = d.pool_hash_id
  WHERE d.addr_id = $1
    AND d.id = (SELECT MAX(id) FROM delegation WHERE addr_id = $1)
    AND (SELECT last_reg FROM regs) > (SELECT last_dereg FROM regs)
    AND d.tx_id > (SELECT last_dereg FROM regs)
),
-- Current DRep delegation: the newest vote-delegation certificate, valid while
-- the account stays registered and the DRep itself is still registered.
vote AS (
  SELECT dh.view AS drep_id, dh.has_script
  FROM delegation_vote dv
    JOIN drep_hash dh ON dh.id = dv.drep_hash_id
  WHERE dv.addr_id = $1
    AND dv.id = (SELECT MAX(id) FROM delegation_vote WHERE addr_id = $1)
    AND dv.tx_id >= (SELECT last_dereg FROM regs)
    AND COALESCE((
        SELECT ROW(dr.tx_id, dr.cert_index)
        FROM drep_registration dr
        WHERE dr.drep_hash_id = dv.drep_hash_id AND dr.deposit > 0
        ORDER BY dr.tx_id DESC, dr.cert_index DESC
        LIMIT 1
      ), ROW(1::bigint, 1::integer))
      > COALESCE((
        SELECT ROW(dr.tx_id, dr.cert_index)
        FROM drep_registration dr
        WHERE dr.drep_hash_id = dv.drep_hash_id AND dr.deposit < 0
        ORDER BY dr.tx_id DESC, dr.cert_index DESC
        LIMIT 1
      ), ROW(-1::bigint, -1::integer))
    AND dv.tx_id >= (
      SELECT COALESCE(MAX(dr.tx_id), -1)
      FROM drep_registration dr
      WHERE dr.drep_hash_id = dv.drep_hash_id AND dr.deposit > 0
    )
),
sums AS (
  SELECT
    (
      SELECT COALESCE(SUM(txo.value), 0)
      FROM tx_out txo
        LEFT JOIN tx_in ti ON ti.tx_out_id = txo.tx_id
          AND ti.tx_out_index = txo.index
      WHERE txo.stake_address_id = $1
        AND ti.id IS NULL
    ) AS utxo_sum,
    (
      SELECT COALESCE(SUM(amount), 0) FROM reward
      WHERE addr_id = $1 AND type <> 'refund'
        AND spendable_epoch <= (SELECT epoch_no FROM now_epoch)
    ) AS rewards,
    (
      SELECT COALESCE(SUM(amount), 0) FROM reward_rest
      WHERE addr_id = $1
        AND spendable_epoch <= (SELECT epoch_no FROM now_epoch)
    ) AS instant_rewards,
    (
      SELECT COALESCE(SUM(amount), 0) FROM reward
      WHERE addr_id = $1 AND type = 'refund'
        AND spendable_epoch <= (SELECT epoch_no FROM now_epoch)
    ) AS refunds,
    (SELECT COALESCE(SUM(amount), 0) FROM withdrawal WHERE addr_id = $1) AS withdrawals,
    (SELECT COALESCE(SUM(amount), 0) FROM reserve WHERE addr_id = $1) AS reserves,
    (SELECT COALESCE(SUM(amount), 0) FROM treasury WHERE addr_id = $1) AS treasury
)
SELECT
  (SELECT pool_id FROM pool) IS NOT NULL AS active,
  (
    SELECT b.epoch_no::INTEGER
    FROM tx t
      JOIN block b ON b.id = t.block_id
    WHERE t.id = GREATEST((SELECT last_reg FROM regs), (SELECT last_dereg FROM regs))
  ) AS active_epoch,
  (SELECT last_reg FROM regs) > (SELECT last_dereg FROM regs) AS registered,
  (s.utxo_sum + s.rewards + s.instant_rewards + s.refunds - s.withdrawals)::TEXT
    AS controlled_amount,
  (s.rewards + s.instant_rewards + s.refunds)::TEXT AS rewards_sum,
  s.withdrawals::TEXT AS withdrawals_sum,
  s.reserves::TEXT AS reserves_sum,
  s.treasury::TEXT AS treasury_sum,
  (s.rewards + s.instant_rewards + s.refunds - s.withdrawals)::TEXT AS withdrawable_amount,
  (SELECT pool_id FROM pool) AS pool_id,
  (SELECT drep_id FROM vote) AS drep_id,
  (SELECT has_script FROM vote) AS drep_has_script
FROM sums s
"#;

pub async fn account(
    State(state): State<AppState>,
    Path(stake_address): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let addr_id = resolve_stake_address(&state, &stake_address).await?;
    let row = sqlx::query(ACCOUNT_SQL)
        .bind(addr_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    let drep_id = row.s("drep_id").map(|view| {
        let has_script = row.boolean("drep_has_script").unwrap_or(false);
        ids::drep_view_to_cip129(&view, has_script).0
    });
    Ok(Json(json!({
        "stake_address": stake_address,
        "active": row.boolean("active"),
        "active_epoch": row.int4("active_epoch"),
        "registered": row.boolean("registered"),
        "controlled_amount": row.s("controlled_amount"),
        "rewards_sum": row.s("rewards_sum"),
        "withdrawals_sum": row.s("withdrawals_sum"),
        "reserves_sum": row.s("reserves_sum"),
        "treasury_sum": row.s("treasury_sum"),
        "withdrawable_amount": row.s("withdrawable_amount"),
        "pool_id": row.s("pool_id"),
        "drep_id": drep_id,
    })))
}

pub async fn delegations(
    State(state): State<AppState>,
    Path(stake_address): Path<String>,
    Query(params): Query<PageParams>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let addr_id = resolve_stake_address(&state, &stake_address).await?;
    let page = Page::resolve(&params, &headers)?;
    let sql = format!(
        r#"
SELECT d.active_epoch_no::INTEGER AS active_epoch,
  encode(t.hash, 'hex') AS tx_hash,
  t.out_sum::TEXT AS amount,
  ph.view AS pool_id,
  b.slot_no::BIGINT AS tx_slot,
  b.block_no::INTEGER AS block_height,
  EXTRACT(EPOCH FROM b.time)::BIGINT AS block_time
FROM delegation d
  JOIN tx t ON t.id = d.tx_id
  JOIN block b ON b.id = t.block_id
  JOIN pool_hash ph ON ph.id = d.pool_hash_id
WHERE d.addr_id = $1
ORDER BY d.tx_id {dir}
LIMIT $2 OFFSET $3
"#,
        dir = page.order.sql()
    );
    let rows = sqlx::query(&sql)
        .bind(addr_id)
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&state.db)
        .await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "active_epoch": r.int4("active_epoch"),
                "tx_hash": r.s("tx_hash"),
                "amount": r.s("amount"),
                "pool_id": r.s("pool_id"),
                "tx_slot": r.int8("tx_slot"),
                "block_height": r.int4("block_height"),
                "block_time": r.int8("block_time"),
            })
        })
        .collect();
    Ok(Json(Value::Array(out)))
}
