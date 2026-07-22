//! Stake pool endpoints: the registered-pool list, the extended list with
//! embedded metadata, and per-pool metadata.
//!
//! A pool counts as registered when its newest registration certificate is not
//! superseded by a newer retirement certificate that has already taken effect.
//! `/pools/extended` intentionally omits the expensive per-pool aggregations
//! (active_stake / live_stake / live_saturation / blocks_minted): each one
//! costs a ledger-wide or chain-wide scan per request and nothing consuming
//! this API uses them. It also omits the off-chain-fetch `error` envelope that
//! `/pools/{id}/metadata` carries: the per-metadata-ref error lookup is too
//! costly to run across the whole pool list (the bulk scrape reads only
//! ticker / name / homepage), so it stays on the cheap single-pool endpoint.

use crate::error::ApiError;
use crate::ids;
use crate::pagination::{Page, PageParams};
use crate::rows::RowExt;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};

/// Filter for pools whose latest registration is still in force. Bound CTEs:
/// `latest_update` and `latest_retire` (one row per pool each), `now_epoch`.
const REGISTERED_POOLS_CTE: &str = r#"
latest_update AS (
  SELECT DISTINCT ON (hash_id) hash_id, registered_tx_id, cert_index, margin,
    fixed_cost, pledge, meta_id
  FROM pool_update
  ORDER BY hash_id, registered_tx_id DESC, cert_index DESC
),
latest_retire AS (
  SELECT DISTINCT ON (hash_id) hash_id, announced_tx_id, cert_index, retiring_epoch
  FROM pool_retire
  ORDER BY hash_id, announced_tx_id DESC, cert_index DESC
),
now_epoch AS (
  SELECT MAX(no) AS no FROM epoch
)
"#;

const REGISTERED_POOLS_WHERE: &str = r#"
  lr.hash_id IS NULL
  OR lu.registered_tx_id > lr.announced_tx_id
  OR (lu.registered_tx_id = lr.announced_tx_id AND lu.cert_index > lr.cert_index)
  OR (lu.registered_tx_id < lr.announced_tx_id
      AND lr.retiring_epoch > (SELECT no FROM now_epoch))
"#;

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let page = Page::resolve(&params, &headers)?;
    let sql = format!(
        r#"
WITH {REGISTERED_POOLS_CTE}
SELECT ph.view AS pool_id
FROM pool_hash ph
  JOIN latest_update lu ON lu.hash_id = ph.id
  LEFT JOIN latest_retire lr ON lr.hash_id = ph.id
WHERE {REGISTERED_POOLS_WHERE}
ORDER BY ph.id {dir}
LIMIT $1 OFFSET $2
"#,
        dir = page.order.sql()
    );
    let rows = sqlx::query(&sql)
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&state.db)
        .await?;
    let ids: Vec<Value> = rows
        .iter()
        .filter_map(|r| r.s("pool_id"))
        .map(Value::String)
        .collect();
    Ok(Json(Value::Array(ids)))
}

pub async fn extended(
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let page = Page::resolve(&params, &headers)?;
    let sql = format!(
        r#"
WITH {REGISTERED_POOLS_CTE},
page AS (
  SELECT ph.id, ph.view, ph.hash_raw, lu.margin, lu.fixed_cost, lu.pledge, lu.meta_id
  FROM pool_hash ph
    JOIN latest_update lu ON lu.hash_id = ph.id
    LEFT JOIN latest_retire lr ON lr.hash_id = ph.id
  WHERE {REGISTERED_POOLS_WHERE}
  ORDER BY ph.id {dir}
  LIMIT $1 OFFSET $2
)
SELECT p.view AS pool_id,
  encode(p.hash_raw, 'hex') AS hex,
  p.margin::FLOAT8 AS margin_cost,
  p.fixed_cost::TEXT AS fixed_cost,
  p.pledge::TEXT AS declared_pledge,
  (
    SELECT json_build_object(
      'url', pmr.url,
      'hash', encode(pmr.hash, 'hex'),
      'ticker', pod.ticker_name,
      'name', pod.json->>'name',
      'description', pod.json->>'description',
      'homepage', pod.json->>'homepage'
    )
    FROM pool_metadata_ref pmr
      LEFT JOIN LATERAL (
        SELECT od.ticker_name, od.json
        FROM off_chain_pool_data od
        WHERE od.hash = pmr.hash
        ORDER BY od.id DESC
        LIMIT 1
      ) pod ON TRUE
    WHERE pmr.id = p.meta_id
  ) AS metadata
FROM page p
ORDER BY p.id {dir}
"#,
        dir = page.order.sql()
    );
    let rows = sqlx::query(&sql)
        .bind(page.limit)
        .bind(page.offset)
        .fetch_all(&state.db)
        .await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "pool_id": r.s("pool_id"),
                "hex": r.s("hex"),
                "margin_cost": r.float8("margin_cost"),
                "fixed_cost": r.s("fixed_cost"),
                "declared_pledge": r.s("declared_pledge"),
                "metadata": r.json("metadata"),
            })
        })
        .collect();
    Ok(Json(Value::Array(out)))
}

const POOL_METADATA_SQL: &str = r#"
WITH pool AS (
  SELECT id, view, hash_raw FROM pool_hash WHERE view = $1
),
latest_meta AS (
  SELECT pu.meta_id
  FROM pool_update pu
  WHERE pu.hash_id = (SELECT id FROM pool)
    AND pu.meta_id IS NOT NULL
  ORDER BY pu.registered_tx_id DESC, pu.cert_index DESC
  LIMIT 1
)
SELECT (SELECT view FROM pool) AS pool_id,
  (SELECT encode(hash_raw, 'hex') FROM pool) AS hex,
  pmr.url AS url,
  encode(pmr.hash, 'hex') AS hash,
  pod.ticker_name AS ticker,
  pod.json->>'name' AS name,
  pod.json->>'description' AS description,
  pod.json->>'homepage' AS homepage,
  CASE WHEN pod.json IS NULL THEN fe.fetch_error ELSE NULL END AS fetch_error
FROM pool_metadata_ref pmr
  LEFT JOIN LATERAL (
    SELECT od.ticker_name, od.json
    FROM off_chain_pool_data od
    WHERE od.hash = pmr.hash
    ORDER BY od.id DESC
    LIMIT 1
  ) pod ON TRUE
  LEFT JOIN LATERAL (
    SELECT f.fetch_error
    FROM off_chain_pool_fetch_error f
    WHERE f.pmr_id = pmr.id
    ORDER BY f.id DESC
    LIMIT 1
  ) fe ON TRUE
WHERE pmr.id = (SELECT meta_id FROM latest_meta)
"#;

pub async fn metadata(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let pool_id = ids::normalize_pool_id(&pool_id)
        .ok_or_else(|| ApiError::bad_request("Invalid or malformed pool id format."))?;

    let exists = sqlx::query("SELECT 1 FROM pool_hash WHERE view = $1")
        .bind(&pool_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }

    let row = sqlx::query(POOL_METADATA_SQL)
        .bind(&pool_id)
        .fetch_optional(&state.db)
        .await?;
    let Some(row) = row else {
        return Ok(Json(json!({})));
    };
    let mut body = json!({
        "pool_id": row.s("pool_id"),
        "hex": row.s("hex"),
        "url": row.s("url"),
        "hash": row.s("hash"),
        "ticker": row.s("ticker"),
        "name": row.s("name"),
        "description": row.s("description"),
        "homepage": row.s("homepage"),
    });
    if let Some(msg) = row.s("fetch_error") {
        body.as_object_mut()
            .expect("object body")
            .insert("error".into(), crate::fetch_error::envelope(&msg));
    }
    Ok(Json(body))
}
