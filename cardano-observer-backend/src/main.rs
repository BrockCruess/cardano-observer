use cardano_observer_backend::{config, routes, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::Config::from_env()?;
    tracing::info!(
        "cardano-observer-backend v{} - network={} bind={} db={}",
        env!("CARGO_PKG_VERSION"),
        config.network.as_str(),
        config.bind,
        config.redacted_db_url(),
    );

    // Lazy pool: the server comes up even while PostgreSQL is still starting;
    // /health reports readiness.
    let statement_timeout_ms = config.db_statement_timeout_ms;
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                if statement_timeout_ms > 0 {
                    sqlx::Executor::execute(
                        &mut *conn,
                        format!("SET statement_timeout = {statement_timeout_ms}").as_str(),
                    )
                    .await?;
                }
                Ok(())
            })
        })
        .connect_lazy(&config.db_url)?;

    let state = AppState {
        db,
        config: Arc::new(config),
    };

    let app = routes::router(state.clone())
        .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(&state.config.bind).await?;
    tracing::info!("listening on {}", state.config.bind);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
