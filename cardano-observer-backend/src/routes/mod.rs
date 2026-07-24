mod accounts;
mod governance;
mod misc;
mod pools;
mod txs;

use crate::AppState;
use axum::routing::get;
use axum::Router;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(misc::root))
        .route("/health", get(misc::health))
        .route("/health/clock", get(misc::health_clock))
        .route("/blocks/latest", get(misc::blocks_latest))
        .route("/pools", get(pools::list))
        .route("/pools/extended", get(pools::extended))
        .route("/pools/{pool_id}/metadata", get(pools::metadata))
        .route("/pools/{pool_id}/updates", get(pools::updates))
        .route("/pools/{pool_id}/registrations", get(pools::registrations))
        .route("/governance/dreps", get(governance::dreps))
        .route(
            "/governance/dreps/{drep_id}/metadata",
            get(governance::drep_metadata),
        )
        .route(
            "/governance/proposals/{tx_hash}/{cert_index}/metadata",
            get(governance::proposal_metadata),
        )
        .route("/accounts/{stake_address}", get(accounts::account))
        .route(
            "/accounts/{stake_address}/delegations",
            get(accounts::delegations),
        )
        .route("/txs/{hash}", get(txs::tx))
        .route("/txs/{hash}/utxos", get(txs::utxos))
        .fallback(misc::not_found)
        .with_state(state)
}
