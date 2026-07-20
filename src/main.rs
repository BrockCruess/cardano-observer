mod config;
#[cfg(has_dapp)]
mod dapp;
#[cfg(not(has_dapp))]
#[path = "dapp_stub.rs"]
mod dapp;
mod demo;
mod deleg;
mod dex;
mod dreps;
mod enrich;
mod gov_actions;
mod handles;
mod model;
mod ogmios;
mod parse;
mod persist;
mod pools;
mod registry;
mod scam_tokens;
mod server;
mod state;
mod trending;

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

    let config = config::Config::from_env();
    tracing::info!(
        "cardano-observer v{} - network={} ogmios={} blockfrost={} handles={} demo={}",
        env!("CARGO_PKG_VERSION"),
        config.network.as_str(),
        config.ogmios_url,
        config.blockfrost_url.as_deref().unwrap_or("(disabled)"),
        config.ada_handle_url.as_deref().unwrap_or("(disabled)"),
        config.demo,
    );

    // Persistence - skipped in demo mode so synthetic events never pollute
    // the on-disk history of a real deployment.
    let persister = match (&config.data_dir, config.demo) {
        (Some(dir), false) => {
            let p = persist::Persister::open(std::path::Path::new(dir))?;
            tracing::info!("persisting events to {dir}");
            Some(Arc::new(p))
        }
        _ => None,
    };

    let state = Arc::new(state::AppState::new(
        match config.network {
            config::Network::Mainnet => "mainnet",
            config::Network::Preprod => "preprod",
            config::Network::Preview => "preview",
        },
        config.event_retention_hours,
        config.tx_cache,
        persister.clone(),
    ));
    if let Some(p) = &persister {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let retention_cutoff = now - state.event_retention_secs();
        let events = p.load_events_since(retention_cutoff);
        // Backfill: resume chain-sync from the last persisted blocks so
        // nothing that happened while the server was down is lost. Points
        // older than the backfill window are ignored (we restart at the tip
        // rather than replaying ancient history into a bounded buffer).
        if config.backfill_hours > 0 {
            let cutoff = now - (config.backfill_hours as i64) * 3600;
            let blocks: Vec<model::BlockRef> = events
                .iter()
                .filter(|e| e.kind == "block" && e.timestamp >= cutoff)
                .filter_map(|e| {
                    Some(model::BlockRef {
                        hash: e.block_hash.clone()?,
                        slot: e.slot,
                        height: e.height?,
                    })
                })
                .collect();
            state.restore_recent_blocks(blocks);
        }
        state.restore(events, p.load_txs_since(retention_cutoff));
    }
    let enricher = Arc::new(enrich::Enricher::new(&config).await);
    state.set_keyword_meta(enricher.clone());
    // Restored JSONL events predate registry stamping — fill decimals now.
    state.stamp_buffered_assets();
    state.stamp_buffered_scam();
    // Learn DRep names from registration anchors in the retention window, then stamp.
    {
        let buf = state.events.lock().unwrap();
        let snap: Vec<_> = buf.iter().cloned().collect();
        drop(buf);
        enricher.warm_dreps_from_events(&snap).await;
        enricher.ensure_gov_action_titles(&snap).await;
    }
    state.stamp_buffered_dreps();
    state.stamp_buffered_gov_actions();
    // Seed trending from the in-memory retention window (already loaded above).
    {
        let buf = state.events.lock().unwrap();
        let snap: Vec<_> = buf.iter().cloned().collect();
        drop(buf);
        tracing::info!(
            "seeding trending from {} buffered events ({}h window)",
            snap.len(),
            config.event_retention_hours
        );
        state.seed_trending(snap);
    }
    tracing::info!(
        "token registry ready ({} subjects); pool cache ready ({} pools); drep cache ready ({} dreps); gov-action titles ready ({} titled); scam-token list ready ({} fingerprints)",
        enricher.registry_len(),
        enricher.pool_cache_len(),
        enricher.drep_cache_len(),
        enricher.gov_action_cache_len(),
        enricher.scam_token_list_len()
    );
    tokio::spawn(enricher.clone().refresh_meta_caches_loop());
    // Empty (or forced) pool/DRep caches scrape in the background; lookups
    // start using them as soon as the in-memory maps are filled.
    {
        let enricher = enricher.clone();
        let state = state.clone();
        let force_pools = config.pool_cache_refresh;
        let force_dreps = config.drep_cache_refresh;
        tokio::spawn(async move {
            if enricher.run_initial_scrapes(force_pools, force_dreps).await {
                state.stamp_buffered_dreps();
                tracing::info!(
                    "re-stamped buffered events with {} drep names",
                    enricher.drep_cache_len()
                );
            }
        });
    }
    let deleg = Arc::new(deleg::DelegationTracker::new());
    // Seed from restored buffer so re-delegations show from→to immediately.
    {
        let buf = state.events.lock().unwrap();
        let snap: Vec<_> = buf.iter().cloned().collect();
        drop(buf);
        deleg.seed_from_events(&snap);
    }

    // Event source: real chain-sync via Ogmios, or the synthetic generator.
    if config.demo {
        tokio::spawn(demo::run(state.clone()));
    } else {
        let dex = Arc::new(dex::DexRegistry::new());
        let dapp = Arc::new(dapp::DappRegistry::with_restored_txs(
            state.cached_tx_entries(),
        ));
        // Drop stale shared-script edges from JSONL so light-cone matches hub rules.
        state.rewrite_buffered_input_txs(&dapp);
        if config.network == config::Network::Mainnet {
            tokio::spawn(dex.clone().refresh_vyfi_loop());
            tokio::spawn(dex.clone().refresh_minswap_pools_loop());
        }
        tokio::spawn(ogmios::run(
            config.clone(),
            state.clone(),
            dex,
            dapp,
            enricher.clone(),
            deleg,
        ));
        // Refill txs.jsonl for any events whose bodies were lost to older
        // count-based compaction — Ogmios replay only, never republishes events.
        tokio::spawn(ogmios::backfill_missing_txs(config.clone(), state.clone()));
    }

    let app = server::router(server::ServerCtx { state, enricher });
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!("web ui listening on http://{}", config.bind);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
