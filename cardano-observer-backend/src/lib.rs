//! Self-hosted Cardano data API over a cardano-db-sync PostgreSQL database.
//!
//! The HTTP layer lives in [`routes`]; additional services (for example a
//! chain-sync event stream) can be mounted alongside it in the binary.

pub mod config;
pub mod error;
pub mod fetch_error;
pub mod ids;
pub mod pagination;
pub mod routes;
pub mod rows;

use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: Arc<config::Config>,
}
