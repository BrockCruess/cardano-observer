//! Axum web server: embedded static UI, WebSocket event stream, and small
//! JSON APIs for the detail modal and metadata enrichment.

use crate::enrich::Enricher;
use crate::state::AppState;
use axum::{
    extract::{ws::WebSocket, ws::WebSocketUpgrade, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct ServerCtx {
    pub state: Arc<AppState>,
    pub enricher: Arc<Enricher>,
}

pub fn router(ctx: ServerCtx) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/cardano-logo.svg", get(cardano_logo))
        .route("/favicon.svg", get(favicon))
        .route("/ws", get(ws_upgrade))
        .route("/api/events", get(api_events))
        .route("/api/buffer", get(api_buffer))
        .route("/api/search", get(api_search))
        .route("/api/tx/{hash}", get(api_tx))
        .route("/api/asset/{unit}", get(api_asset))
        .route("/api/registry", get(api_registry))
        .route("/api/pool/{id}", get(api_pool))
        .route("/api/drep/{id}", get(api_drep))
        .route("/api/dreps", get(api_dreps))
        .route("/api/stats", get(api_stats))
        .route("/api/trending", get(api_trending))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(ctx)
}

async fn index() -> Response {
    static_file(include_str!("../static/index.html"), "text/html; charset=utf-8")
}
async fn app_js() -> Response {
    static_file(include_str!("../static/app.js"), "application/javascript; charset=utf-8")
}
async fn style_css() -> Response {
    static_file(include_str!("../static/style.css"), "text/css; charset=utf-8")
}
async fn cardano_logo() -> Response {
    static_file(include_str!("../static/cardano-logo.svg"), "image/svg+xml")
}
async fn favicon() -> Response {
    static_file(include_str!("../static/favicon.svg"), "image/svg+xml")
}

fn static_file(body: &'static str, content_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-cache")], body)
        .into_response()
}

async fn ws_upgrade(State(ctx): State<ServerCtx>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| ws_client(socket, ctx.state))
}

async fn ws_client(mut socket: WebSocket, state: Arc<AppState>) {
    use axum::extract::ws::Message;

    // Snapshot first so the feed is instantly populated, then live events.
    let mut rx = state.sender.subscribe();
    // Keep the first paint short (~one viewport); older events load on scroll.
    let snapshot = state.snapshot(25).to_string();
    if socket.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }

    let mut stats_tick = tokio::time::interval(Duration::from_secs(10));
    stats_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            broadcast = rx.recv() => match broadcast {
                Ok(msg) => {
                    if socket.send(Message::Text(msg.into())).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Client fell behind; resync it with a fresh snapshot.
                    tracing::debug!("ws client lagged by {n}, resyncing");
                    rx = rx.resubscribe();
                    let snap = state.snapshot(25).to_string();
                    if socket.send(Message::Text(snap.into())).await.is_err() {
                        return;
                    }
                }
                Err(_) => return,
            },
            _ = stats_tick.tick() => {
                if socket.send(Message::Text(state.stats().to_string().into())).await.is_err() {
                    return;
                }
                let trending = json!({
                    "type": "trending",
                    "terms": state.trending_top(),
                })
                .to_string();
                if socket.send(Message::Text(trending.into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None => return,
                Some(Err(_)) => return,
                _ => {} // ignore pings/other client messages
            },
        }
    }
}

async fn api_tx(State(ctx): State<ServerCtx>, Path(hash): Path<String>) -> Response {
    if let Some(cached) = ctx.state.get_tx(&hash) {
        return Json(cached).into_response();
    }
    if let Some(fallback) = ctx.enricher.tx_fallback(&hash).await {
        return Json(fallback).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({ "error": "transaction not found" }))).into_response()
}

#[derive(Deserialize)]
struct EventsQuery {
    before: Option<u64>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    /// Only return matches with id < before (for paging older hits).
    before: Option<u64>,
    limit: Option<usize>,
}

async fn api_events(State(ctx): State<ServerCtx>, Query(q): Query<EventsQuery>) -> Response {
    let before = q.before.unwrap_or(u64::MAX);
    let limit = q.limit.unwrap_or(100);
    Json(ctx.state.events_before(before, limit)).into_response()
}

/// Full in-memory retention window for client-side search indexing.
async fn api_buffer(State(ctx): State<ServerCtx>) -> Response {
    Json(ctx.state.retention_buffer()).into_response()
}

async fn api_search(State(ctx): State<ServerCtx>, Query(q): Query<SearchQuery>) -> Response {
    Json(ctx.state.search_buffered(&q.q, q.before, q.limit.unwrap_or(5_000))).into_response()
}

async fn api_asset(State(ctx): State<ServerCtx>, Path(unit): Path<String>) -> Response {
    Json(ctx.enricher.asset(&unit).await).into_response()
}

/// Full in-memory CIP-26 map for browser-side quantity formatting.
async fn api_registry(State(ctx): State<ServerCtx>) -> Response {
    Json(ctx.enricher.registry_assets_json()).into_response()
}

async fn api_pool(State(ctx): State<ServerCtx>, Path(id): Path<String>) -> Response {
    Json(ctx.enricher.pool(&id).await).into_response()
}

async fn api_drep(State(ctx): State<ServerCtx>, Path(id): Path<String>) -> Response {
    Json(ctx.enricher.drep(&id).await).into_response()
}

/// Full in-memory DRep name map for browser-side labels (no per-id round-trips).
async fn api_dreps(State(ctx): State<ServerCtx>) -> Response {
    Json(ctx.enricher.dreps_json()).into_response()
}

async fn api_stats(State(ctx): State<ServerCtx>) -> Response {
    let mut stats = ctx.state.stats();
    stats["tip"] = serde_json::to_value(&*ctx.state.tip.lock().unwrap()).unwrap_or_default();
    stats["network"] = json!(ctx.state.network);
    Json(stats).into_response()
}

async fn api_trending(State(ctx): State<ServerCtx>) -> Response {
    Json(json!({ "terms": ctx.state.trending_top() })).into_response()
}
