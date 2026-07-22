//! Governance endpoints: the DRep list with embedded metadata anchors, per-DRep
//! metadata, and governance action (proposal) metadata.
//!
//! DRep status semantics follow the on-chain rules: a DRep is retired when its
//! newest deposit-refunding certificate is more recent than its newest
//! registration; it is expired when registered but inactive (no certificate or
//! vote) for more than `drep_activity` epochs.

use crate::error::ApiError;
use crate::ids;
use crate::pagination::{parse_bool_filter, Page, PageParams};
use crate::rows::RowExt;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Default, Deserialize)]
pub struct DrepListParams {
    pub count: Option<i64>,
    pub page: Option<i64>,
    pub order: Option<String>,
    pub retired: Option<String>,
    pub expired: Option<String>,
}

pub async fn dreps(
    State(state): State<AppState>,
    Query(params): Query<DrepListParams>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let page = Page::resolve(
        &PageParams {
            count: params.count,
            page: params.page,
            order: params.order.clone(),
        },
        &headers,
    )?;
    let retired = parse_bool_filter(params.retired.as_deref(), "retired")?;
    let expired = parse_bool_filter(params.expired.as_deref(), "expired")?;

    let sql = format!(
        r#"
WITH now_epoch AS (
  SELECT e.no AS epoch_no, ep.drep_activity
  FROM epoch e
    JOIN epoch_param ep ON ep.epoch_no = e.no
  ORDER BY e.no DESC
  LIMIT 1
),
-- Newest tx that could still count as activity for an expired DRep: anything
-- at or before this tx means the DRep has been quiet longer than drep_activity
-- epochs. COALESCE to 0 keeps the comparison false for everyone when the
-- boundary epoch has no blocks.
activity_cutoff AS (
  SELECT COALESCE(MAX(t.id), 0) AS tx_id
  FROM tx t
    JOIN block b ON b.id = t.block_id
  WHERE b.epoch_no::INTEGER = (
    SELECT epoch_no::INTEGER - drep_activity::INTEGER - 1 FROM now_epoch
  )
),
certs AS (
  SELECT drep_hash_id,
    MAX(CASE WHEN deposit > 0 THEN tx_id END) AS last_registered,
    MAX(CASE WHEN deposit < 0 THEN tx_id END) AS last_retired,
    MAX(tx_id) AS last_cert
  FROM drep_registration
  GROUP BY drep_hash_id
),
last_votes AS (
  SELECT drep_voter AS drep_hash_id, MAX(tx_id) AS last_vote
  FROM voting_procedure
  WHERE drep_voter IS NOT NULL
  GROUP BY drep_voter
),
status AS (
  SELECT dh.id, dh.view, dh.raw, dh.has_script,
    COALESCE(c.last_registered, 1) > COALESCE(c.last_retired, -1) AS registered,
    GREATEST(c.last_cert, v.last_vote) AS last_active_tx
  FROM drep_hash dh
    LEFT JOIN certs c ON c.drep_hash_id = dh.id
    LEFT JOIN last_votes v ON v.drep_hash_id = dh.id
),
flagged AS (
  SELECT s.*,
    (
      s.registered
      AND s.last_active_tx IS NOT NULL
      AND s.last_active_tx <= (SELECT tx_id FROM activity_cutoff)
    ) AS expired
  FROM status s
),
page AS (
  SELECT *
  FROM flagged
  WHERE ($3::boolean IS NULL OR (NOT registered) = $3)
    AND ($4::boolean IS NULL OR expired = $4)
  ORDER BY id {dir}
  LIMIT $1 OFFSET $2
)
SELECT p.view AS drep_id,
  encode(p.raw, 'hex') AS hex,
  p.has_script AS has_script,
  COALESCE(dd.amount, 0)::TEXT AS amount,
  (NOT p.registered) AS retired,
  p.expired AS expired,
  b.epoch_no::INTEGER AS last_active_epoch,
  va.url AS metadata_url,
  encode(va.data_hash, 'hex') AS metadata_hash,
  ocvd.json AS metadata_json,
  ocvd.bytes::TEXT AS metadata_bytes,
  CASE WHEN ocvd.json IS NULL THEN vfe.fetch_error ELSE NULL END AS metadata_fetch_error
FROM page p
  LEFT JOIN tx ON tx.id = p.last_active_tx
  LEFT JOIN block b ON b.id = tx.block_id
  LEFT JOIN drep_distr dd ON dd.hash_id = p.id
    AND dd.epoch_no = (SELECT epoch_no FROM now_epoch)
  LEFT JOIN LATERAL (
    SELECT dr.voting_anchor_id
    FROM drep_registration dr
    WHERE dr.drep_hash_id = p.id
      AND dr.voting_anchor_id IS NOT NULL
    ORDER BY dr.tx_id DESC, dr.cert_index DESC
    LIMIT 1
  ) anchor ON TRUE
  LEFT JOIN voting_anchor va ON va.id = anchor.voting_anchor_id
  LEFT JOIN LATERAL (
    SELECT od.json, od.bytes
    FROM off_chain_vote_data od
    WHERE od.voting_anchor_id = va.id
    ORDER BY od.id DESC
    LIMIT 1
  ) ocvd ON TRUE
  LEFT JOIN LATERAL (
    SELECT f.fetch_error
    FROM off_chain_vote_fetch_error f
    WHERE f.voting_anchor_id = va.id
    ORDER BY f.id DESC
    LIMIT 1
  ) vfe ON TRUE
ORDER BY p.id {dir}
"#,
        dir = page.order.sql()
    );

    let rows = sqlx::query(&sql)
        .bind(page.limit)
        .bind(page.offset)
        .bind(retired)
        .bind(expired)
        .fetch_all(&state.db)
        .await?;

    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            let view = r.s("drep_id").unwrap_or_default();
            let has_script = r.boolean("has_script").unwrap_or(false);
            let (drep_id, cip129_hex) = ids::drep_view_to_cip129(&view, has_script);
            let metadata = match r.s("metadata_url") {
                None => Value::Null,
                Some(url) => {
                    let mut m = json!({
                        "url": url,
                        "hash": r.s("metadata_hash"),
                        "json_metadata": r.json("metadata_json"),
                        "bytes": r.s("metadata_bytes"),
                        "fetch_error": r.s("metadata_fetch_error"),
                    });
                    crate::fetch_error::transform_metadata(&mut m);
                    m
                }
            };
            json!({
                "drep_id": drep_id,
                "hex": cip129_hex.or_else(|| r.s("hex")),
                "has_script": has_script,
                "amount": r.s("amount"),
                "retired": r.boolean("retired"),
                "expired": r.boolean("expired"),
                "last_active_epoch": r.int4("last_active_epoch"),
                "metadata": metadata,
            })
        })
        .collect();
    Ok(Json(Value::Array(out)))
}

const DREP_METADATA_SQL: &str = r#"
SELECT dh.view AS drep_id,
  encode(dh.raw, 'hex') AS hex,
  va.url AS url,
  encode(va.data_hash, 'hex') AS hash,
  ocvd.json AS json_metadata,
  ocvd.bytes::TEXT AS bytes,
  CASE WHEN ocvd.json IS NULL THEN vfe.fetch_error ELSE NULL END AS fetch_error
FROM drep_hash dh
  JOIN LATERAL (
    SELECT dr.voting_anchor_id
    FROM drep_registration dr
    WHERE dr.drep_hash_id = dh.id
      AND dr.voting_anchor_id IS NOT NULL
    ORDER BY dr.tx_id DESC, dr.cert_index DESC
    LIMIT 1
  ) anchor ON TRUE
  JOIN voting_anchor va ON va.id = anchor.voting_anchor_id
  LEFT JOIN LATERAL (
    SELECT od.json, od.bytes
    FROM off_chain_vote_data od
    WHERE od.voting_anchor_id = va.id
    ORDER BY od.id DESC
    LIMIT 1
  ) ocvd ON TRUE
  LEFT JOIN LATERAL (
    SELECT f.fetch_error
    FROM off_chain_vote_fetch_error f
    WHERE f.voting_anchor_id = va.id
    ORDER BY f.id DESC
    LIMIT 1
  ) vfe ON TRUE
WHERE (
    ($1::bytea IS NOT NULL AND dh.raw = $1)
    OR ($1::bytea IS NULL AND dh.view = $2)
  )
  AND dh.has_script = $3
"#;

pub async fn drep_metadata(
    State(state): State<AppState>,
    Path(drep_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let parsed = ids::parse_drep_id(&drep_id)
        .ok_or_else(|| ApiError::bad_request("Invalid or malformed drep id."))?;

    let row = sqlx::query(DREP_METADATA_SQL)
        .bind(parsed.raw.as_deref())
        .bind(&parsed.view)
        .bind(parsed.has_script)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Echo the id in the form the caller used: CIP-129 in, CIP-129 out.
    let (drep_id, hex) = if parsed.is_cip129 {
        (parsed.cip129.clone(), parsed.cip129_hex.clone())
    } else {
        (row.s("drep_id").unwrap_or(parsed.view), row.s("hex"))
    };
    let mut body = json!({
        "drep_id": drep_id,
        "hex": hex,
        "url": row.s("url"),
        "hash": row.s("hash"),
        "json_metadata": row.json("json_metadata"),
        "bytes": row.s("bytes"),
    });
    if let Some(msg) = row.s("fetch_error") {
        body.as_object_mut()
            .expect("object body")
            .insert("error".into(), crate::fetch_error::envelope(&msg));
    }
    Ok(Json(body))
}

const PROPOSAL_METADATA_SQL: &str = r#"
SELECT encode(t.hash, 'hex') AS tx_hash,
  gap.index::INTEGER AS cert_index,
  va.url AS url,
  encode(va.data_hash, 'hex') AS hash,
  ocvd.json AS json_metadata,
  ocvd.bytes::TEXT AS bytes,
  CASE WHEN ocvd.json IS NULL THEN vfe.fetch_error ELSE NULL END AS fetch_error
FROM gov_action_proposal gap
  JOIN tx t ON t.id = gap.tx_id
  JOIN voting_anchor va ON va.id = gap.voting_anchor_id
  LEFT JOIN LATERAL (
    SELECT od.json, od.bytes
    FROM off_chain_vote_data od
    WHERE od.voting_anchor_id = va.id
    ORDER BY od.id DESC
    LIMIT 1
  ) ocvd ON TRUE
  LEFT JOIN LATERAL (
    SELECT f.fetch_error
    FROM off_chain_vote_fetch_error f
    WHERE f.voting_anchor_id = va.id
    ORDER BY f.id DESC
    LIMIT 1
  ) vfe ON TRUE
WHERE t.hash = $1::bytea
  AND gap.index = $2
"#;

pub async fn proposal_metadata(
    State(state): State<AppState>,
    Path((tx_hash, cert_index)): Path<(String, i32)>,
) -> Result<Json<Value>, ApiError> {
    let tx_hash = tx_hash.trim().to_lowercase();
    if !ids::is_valid_tx_hash(&tx_hash) {
        return Err(ApiError::bad_request(
            "Invalid or malformed transaction hash.",
        ));
    }
    let hash_bytes = hex::decode(&tx_hash).map_err(|_| {
        ApiError::bad_request("Invalid or malformed transaction hash.")
    })?;

    let row = sqlx::query(PROPOSAL_METADATA_SQL)
        .bind(&hash_bytes)
        .bind(cert_index)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    let mut body = json!({
        "id": ids::gov_action_id(&tx_hash, cert_index as u64),
        "tx_hash": row.s("tx_hash"),
        "cert_index": row.int4("cert_index"),
        "url": row.s("url"),
        "hash": row.s("hash"),
        "json_metadata": row.json("json_metadata"),
        "bytes": row.s("bytes"),
    });
    if let Some(msg) = row.s("fetch_error") {
        body.as_object_mut()
            .expect("object body")
            .insert("error".into(), crate::fetch_error::envelope(&msg));
    }
    Ok(Json(body))
}
