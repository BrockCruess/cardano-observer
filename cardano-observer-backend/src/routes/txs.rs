//! Transaction endpoints: the transaction summary and its resolved inputs and
//! outputs. Amount lists follow the `{unit, quantity}` convention with the
//! lovelace entry first, quantities as strings.

use crate::error::ApiError;
use crate::ids;
use crate::rows::RowExt;
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

fn tx_hash_bytes(hash: &str) -> Result<(String, Vec<u8>), ApiError> {
    let hash = hash.trim().to_lowercase();
    if !ids::is_valid_tx_hash(&hash) {
        return Err(ApiError::bad_request(
            "Invalid or malformed transaction hash.",
        ));
    }
    let bytes = hex::decode(&hash)
        .map_err(|_| ApiError::bad_request("Invalid or malformed transaction hash."))?;
    Ok((hash, bytes))
}

/// Builds an amount list: lovelace first, then any native
/// assets from a `json_agg` column.
fn amount_list(lovelace: Option<String>, assets: Option<Value>) -> Value {
    let mut list = vec![json!({
        "unit": "lovelace",
        "quantity": lovelace.unwrap_or_else(|| "0".into()),
    })];
    if let Some(Value::Array(items)) = assets {
        list.extend(items);
    }
    Value::Array(list)
}

const TX_SQL: &str = r#"
WITH t AS (
  SELECT * FROM tx WHERE hash = $1::bytea
)
SELECT encode(t.hash, 'hex') AS hash,
  encode(b.hash, 'hex') AS block,
  b.block_no::INTEGER AS block_height,
  EXTRACT(EPOCH FROM b.time)::BIGINT AS block_time,
  b.slot_no::BIGINT AS slot,
  t.block_index::INTEGER AS index,
  t.out_sum::TEXT AS output_lovelace,
  (
    SELECT json_agg(
        json_build_object(
          'unit', encode(ma.policy, 'hex') || encode(ma.name, 'hex'),
          'quantity', mto.quantity::TEXT
        )
        ORDER BY encode(ma.policy, 'hex') || encode(ma.name, 'hex')
      )
    FROM ma_tx_out mto
      JOIN multi_asset ma ON ma.id = mto.ident
    WHERE mto.tx_out_id IN (SELECT id FROM tx_out WHERE tx_id = t.id)
  ) AS output_assets,
  t.fee::TEXT AS fees,
  t.deposit::TEXT AS deposit,
  t.size::INTEGER AS size,
  t.invalid_before::TEXT AS invalid_before,
  t.invalid_hereafter::TEXT AS invalid_hereafter,
  t.valid_contract AS valid_contract,
  (SELECT COUNT(*) FROM tx_in WHERE tx_in_id = t.id)
    + (SELECT COUNT(*) FROM tx_out WHERE tx_id = t.id) AS utxo_count,
  (SELECT COUNT(*) FROM withdrawal WHERE tx_id = t.id) AS withdrawal_count,
  (SELECT COUNT(DISTINCT tx_id) FROM treasury WHERE tx_id = t.id)
    + (SELECT COUNT(DISTINCT tx_id) FROM reserve WHERE tx_id = t.id) AS mir_cert_count,
  (SELECT COUNT(*) FROM delegation WHERE tx_id = t.id) AS delegation_count,
  (SELECT COUNT(*) FROM stake_registration WHERE tx_id = t.id)
    + (SELECT COUNT(*) FROM stake_deregistration WHERE tx_id = t.id) AS stake_cert_count,
  (SELECT COUNT(*) FROM pool_update WHERE registered_tx_id = t.id) AS pool_update_count,
  (SELECT COUNT(*) FROM pool_retire WHERE announced_tx_id = t.id) AS pool_retire_count,
  (SELECT COUNT(*) FROM ma_tx_mint WHERE tx_id = t.id) AS asset_mint_or_burn_count,
  (SELECT COUNT(*) FROM redeemer WHERE tx_id = t.id) AS redeemer_count,
  COALESCE(t.treasury_donation, 0)::TEXT AS treasury_donation
FROM t
  JOIN block b ON b.id = t.block_id
"#;

pub async fn tx(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, hash_bytes) = tx_hash_bytes(&hash)?;
    let row = sqlx::query(TX_SQL)
        .bind(&hash_bytes)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(json!({
        "hash": row.s("hash"),
        "block": row.s("block"),
        "block_height": row.int4("block_height"),
        "block_time": row.int8("block_time"),
        "slot": row.int8("slot"),
        "index": row.int4("index"),
        "output_amount": amount_list(row.s("output_lovelace"), row.json("output_assets")),
        "fees": row.s("fees"),
        "deposit": row.s("deposit"),
        "size": row.int4("size"),
        "invalid_before": row.s("invalid_before"),
        "invalid_hereafter": row.s("invalid_hereafter"),
        "utxo_count": row.int8("utxo_count"),
        "withdrawal_count": row.int8("withdrawal_count"),
        "mir_cert_count": row.int8("mir_cert_count"),
        "delegation_count": row.int8("delegation_count"),
        "stake_cert_count": row.int8("stake_cert_count"),
        "pool_update_count": row.int8("pool_update_count"),
        "pool_retire_count": row.int8("pool_retire_count"),
        "asset_mint_or_burn_count": row.int8("asset_mint_or_burn_count"),
        "redeemer_count": row.int8("redeemer_count"),
        "valid_contract": row.boolean("valid_contract"),
        "treasury_donation": row.s("treasury_donation"),
    })))
}

/// Inputs resolve each consumed (or referenced / collateral) output of an
/// earlier transaction. `ord_id` keeps the on-chain input order stable.
const TX_INPUTS_SQL: &str = r#"
SELECT * FROM (
  SELECT txi.id AS ord_id,
    prev_out.address AS address,
    encode(prev_tx.hash, 'hex') AS tx_hash,
    txi.tx_out_index::INTEGER AS output_index,
    prev_out.value::TEXT AS lovelace,
    (
      SELECT json_agg(
          json_build_object(
            'unit', encode(ma.policy, 'hex') || encode(ma.name, 'hex'),
            'quantity', mto.quantity::TEXT
          )
        )
      FROM ma_tx_out mto
        JOIN multi_asset ma ON ma.id = mto.ident
      WHERE mto.tx_out_id = prev_out.id
    ) AS assets,
    FALSE AS collateral,
    FALSE AS reference,
    encode(prev_out.data_hash, 'hex') AS data_hash,
    encode(d.bytes, 'hex') AS inline_datum,
    NULL::TEXT AS reference_script_hash
  FROM tx_in txi
    JOIN tx_out prev_out ON prev_out.tx_id = txi.tx_out_id
      AND prev_out.index = txi.tx_out_index
    JOIN tx prev_tx ON prev_tx.id = txi.tx_out_id
    LEFT JOIN datum d ON d.id = prev_out.inline_datum_id
  WHERE txi.tx_in_id = $1

  UNION ALL

  SELECT rti.id AS ord_id,
    prev_out.address,
    encode(prev_tx.hash, 'hex'),
    rti.tx_out_index::INTEGER,
    prev_out.value::TEXT,
    (
      SELECT json_agg(
          json_build_object(
            'unit', encode(ma.policy, 'hex') || encode(ma.name, 'hex'),
            'quantity', mto.quantity::TEXT
          )
        )
      FROM ma_tx_out mto
        JOIN multi_asset ma ON ma.id = mto.ident
      WHERE mto.tx_out_id = prev_out.id
    ),
    FALSE,
    TRUE,
    encode(prev_out.data_hash, 'hex'),
    encode(d.bytes, 'hex'),
    encode(s.hash, 'hex')
  FROM reference_tx_in rti
    JOIN tx_out prev_out ON prev_out.tx_id = rti.tx_out_id
      AND prev_out.index = rti.tx_out_index
    JOIN tx prev_tx ON prev_tx.id = rti.tx_out_id
    LEFT JOIN datum d ON d.id = prev_out.inline_datum_id
    LEFT JOIN script s ON s.id = prev_out.reference_script_id
  WHERE rti.tx_in_id = $1

  UNION ALL

  SELECT cti.id AS ord_id,
    prev_out.address,
    encode(prev_tx.hash, 'hex'),
    cti.tx_out_index::INTEGER,
    prev_out.value::TEXT,
    NULL,
    TRUE,
    FALSE,
    encode(prev_out.data_hash, 'hex'),
    encode(d.bytes, 'hex'),
    NULL
  FROM collateral_tx_in cti
    JOIN tx_out prev_out ON prev_out.tx_id = cti.tx_out_id
      AND prev_out.index = cti.tx_out_index
    JOIN tx prev_tx ON prev_tx.id = cti.tx_out_id
    LEFT JOIN datum d ON d.id = prev_out.inline_datum_id
  WHERE cti.tx_in_id = $1
) inputs
ORDER BY collateral ASC, ord_id ASC, output_index ASC
"#;

const TX_OUTPUTS_SQL: &str = r#"
SELECT * FROM (
  SELECT o.address AS address,
    o.value::TEXT AS lovelace,
    (
      SELECT json_agg(
          json_build_object(
            'unit', encode(ma.policy, 'hex') || encode(ma.name, 'hex'),
            'quantity', mto.quantity::TEXT
          )
        )
      FROM ma_tx_out mto
        JOIN multi_asset ma ON ma.id = mto.ident
      WHERE mto.tx_out_id = o.id
    ) AS assets,
    encode(o.data_hash, 'hex') AS data_hash,
    encode(d.bytes, 'hex') AS inline_datum,
    FALSE AS collateral,
    encode(s.hash, 'hex') AS reference_script_hash,
    o.index::INTEGER AS output_index,
    (
      SELECT encode(t2.hash, 'hex')
      FROM tx t2
      WHERE t2.id = o.consumed_by_tx_id
    ) AS consumed_by_tx
  FROM tx_out o
    LEFT JOIN datum d ON d.id = o.inline_datum_id
    LEFT JOIN script s ON s.id = o.reference_script_id
  WHERE o.tx_id = $1

  UNION ALL

  SELECT co.address,
    co.value::TEXT,
    NULL,
    encode(co.data_hash, 'hex'),
    encode(d.bytes, 'hex'),
    TRUE,
    encode(s.hash, 'hex'),
    co.index::INTEGER,
    NULL
  FROM collateral_tx_out co
    LEFT JOIN datum d ON d.id = co.inline_datum_id
    LEFT JOIN script s ON s.id = co.reference_script_id
  WHERE co.tx_id = $1
) outputs
ORDER BY output_index ASC, collateral ASC
"#;

pub async fn utxos(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (hash, hash_bytes) = tx_hash_bytes(&hash)?;
    let tx_row = sqlx::query("SELECT id FROM tx WHERE hash = $1::bytea")
        .bind(&hash_bytes)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    let tx_id = tx_row.int8("id").ok_or(ApiError::NotFound)?;

    let input_rows = sqlx::query(TX_INPUTS_SQL)
        .bind(tx_id)
        .fetch_all(&state.db)
        .await?;
    let output_rows = sqlx::query(TX_OUTPUTS_SQL)
        .bind(tx_id)
        .fetch_all(&state.db)
        .await?;

    let inputs: Vec<Value> = input_rows
        .iter()
        .map(|r| {
            json!({
                "address": r.s("address"),
                "amount": amount_list(r.s("lovelace"), r.json("assets")),
                "tx_hash": r.s("tx_hash"),
                "output_index": r.int4("output_index"),
                "data_hash": r.s("data_hash"),
                "inline_datum": r.s("inline_datum"),
                "reference_script_hash": r.s("reference_script_hash"),
                "collateral": r.boolean("collateral"),
                "reference": r.boolean("reference"),
            })
        })
        .collect();
    let outputs: Vec<Value> = output_rows
        .iter()
        .map(|r| {
            json!({
                "address": r.s("address"),
                "amount": amount_list(r.s("lovelace"), r.json("assets")),
                "output_index": r.int4("output_index"),
                "data_hash": r.s("data_hash"),
                "inline_datum": r.s("inline_datum"),
                "collateral": r.boolean("collateral"),
                "reference_script_hash": r.s("reference_script_hash"),
                "consumed_by_tx": r.s("consumed_by_tx"),
            })
        })
        .collect();

    Ok(Json(json!({
        "hash": hash,
        "inputs": inputs,
        "outputs": outputs,
    })))
}
