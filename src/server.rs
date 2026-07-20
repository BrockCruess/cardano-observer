//! Axum web server: embedded static UI, WebSocket event stream, and small
//! JSON APIs for the detail modal and metadata enrichment.

use crate::enrich::Enricher;
use crate::state::AppState;
use axum::{
    extract::{ws::WebSocket, ws::WebSocketUpgrade, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tower_http::compression::CompressionLayer;

#[derive(Clone)]
pub struct ServerCtx {
    pub state: Arc<AppState>,
    pub enricher: Arc<Enricher>,
}

pub fn router(ctx: ServerCtx) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/dex/mod.js", get(dex_mod_js))
        .route("/dex/logos/{name}", get(dex_logo))
        .route("/dapp/mod.js", get(dapp_mod_js))
        .route("/dapp/logos/{name}", get(dapp_logo))
        .route("/style.css", get(style_css))
        .route("/cardano-logo.svg", get(cardano_logo))
        .route("/favicon.svg", get(favicon))
        .route("/no-filter-bg.png", get(no_filter_bg))
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
        .route("/api/handle/{addr}", get(api_handle))
        .route("/api/gov-action/{tx}/{index}", get(api_gov_action))
        .route("/api/gov-actions", get(api_gov_actions))
        .route("/api/vote-rationale", get(api_vote_rationale))
        .route("/api/stats", get(api_stats))
        .route("/api/trending", get(api_trending))
        .route("/healthz", get(|| async { "ok" }))
        .layer(CompressionLayer::new())
        .with_state(ctx)
}

/// HTML shell — always revalidate (small; picks up deploys immediately).
const CACHE_HTML: &str = "no-cache";
/// JS / CSS / SVG — reusable for an hour, then revalidate via ETag.
const CACHE_ASSET: &str = "public, max-age=3600, must-revalidate";
/// Large rarely-changing image.
const CACHE_IMAGE: &str = "public, max-age=86400, must-revalidate";

async fn index(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/index.html").as_bytes(),
        "text/html; charset=utf-8",
        CACHE_HTML,
        &headers,
    )
}
async fn app_js(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/app.js").as_bytes(),
        "application/javascript; charset=utf-8",
        CACHE_ASSET,
        &headers,
    )
}

async fn dex_mod_js(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/dex/mod.js").as_bytes(),
        "application/javascript; charset=utf-8",
        CACHE_ASSET,
        &headers,
    )
}

async fn dex_logo(Path(name): Path<String>, headers: HeaderMap) -> Response {
    let (bytes, ctype): (&[u8], &str) = match name.as_str() {
        "minswap.svg" => (
            include_bytes!("../static/dex/logos/minswap.svg"),
            "image/svg+xml",
        ),
        "sundaeswap.png" => (
            include_bytes!("../static/dex/logos/sundaeswap.png"),
            "image/png",
        ),
        "wingriders.png" => (
            include_bytes!("../static/dex/logos/wingriders.png"),
            "image/png",
        ),
        "muesliswap.png" => (
            include_bytes!("../static/dex/logos/muesliswap.png"),
            "image/png",
        ),
        "splash.svg" => (
            include_bytes!("../static/dex/logos/splash.svg"),
            "image/svg+xml",
        ),
        "vyfinance.png" => (
            include_bytes!("../static/dex/logos/vyfinance.png"),
            "image/png",
        ),
        "cswap.png" => (
            include_bytes!("../static/dex/logos/cswap.png"),
            "image/png",
        ),
        "geniusyield.png" => (
            include_bytes!("../static/dex/logos/geniusyield.png"),
            "image/png",
        ),
        "chadswap.png" => (
            include_bytes!("../static/dex/logos/chadswap.png"),
            "image/png",
        ),
        "danofinance.png" => (
            include_bytes!("../static/dex/logos/danofinance.png"),
            "image/png",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    static_asset(bytes, ctype, CACHE_ASSET, &headers)
}

/// Optional dApp UI pack. 404 when `src/dapp/` was absent at compile time.
#[cfg(has_dapp)]
async fn dapp_mod_js(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/dapp/mod.js").as_bytes(),
        "application/javascript; charset=utf-8",
        CACHE_ASSET,
        &headers,
    )
}

#[cfg(not(has_dapp))]
async fn dapp_mod_js() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// Optional dApp brand marks for event-card icons.
#[cfg(has_dapp)]
async fn dapp_logo(Path(name): Path<String>, headers: HeaderMap) -> Response {
    let (bytes, ctype): (&[u8], &str) = match name.as_str() {
        "iagon.png" => (
            include_bytes!("../static/dapp/logos/iagon.png"),
            "image/png",
        ),
        "indigo.png" => (
            include_bytes!("../static/dapp/logos/indigo.png"),
            "image/png",
        ),
        "fluidtokens.png" => (
            include_bytes!("../static/dapp/logos/fluidtokens.png"),
            "image/png",
        ),
        "liqwid.png" => (
            include_bytes!("../static/dapp/logos/liqwid.png"),
            "image/png",
        ),
        "optim.svg" => (
            include_bytes!("../static/dapp/logos/optim.svg"),
            "image/svg+xml",
        ),
        "dano.png" => (
            include_bytes!("../static/dapp/logos/dano.png"),
            "image/png",
        ),
        "strike.png" => (
            include_bytes!("../static/dapp/logos/strike.png"),
            "image/png",
        ),
        "surf.png" => (
            include_bytes!("../static/dapp/logos/surf.png"),
            "image/png",
        ),
        "wayup.svg" => (
            include_bytes!("../static/dapp/logos/wayup.svg"),
            "image/svg+xml",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    // Same cache policy as JS/CSS so logo tweaks show up after rebuild+refresh
    // (CACHE_IMAGE's 24h max-age was hiding updates in the browser).
    static_asset(bytes, ctype, CACHE_ASSET, &headers)
}

#[cfg(not(has_dapp))]
async fn dapp_logo(Path(_name): Path<String>) -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn style_css(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/style.css").as_bytes(),
        "text/css; charset=utf-8",
        CACHE_ASSET,
        &headers,
    )
}
async fn cardano_logo(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/cardano-logo.svg").as_bytes(),
        "image/svg+xml",
        CACHE_ASSET,
        &headers,
    )
}
async fn favicon(headers: HeaderMap) -> Response {
    static_asset(
        include_str!("../static/favicon.svg").as_bytes(),
        "image/svg+xml",
        CACHE_ASSET,
        &headers,
    )
}
async fn no_filter_bg(headers: HeaderMap) -> Response {
    static_asset(
        include_bytes!("../static/no-filter-bg.png"),
        "image/png",
        CACHE_IMAGE,
        &headers,
    )
}

fn content_etag(bytes: &[u8]) -> String {
    let hash = blake2b_simd::Params::new()
        .hash_length(16)
        .hash(bytes);
    format!("\"{}\"", hex::encode(hash.as_bytes()))
}

/// True when `If-None-Match` lists our etag (or `*`).
fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    let Some(raw) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let want = etag.trim_matches('"');
    raw.split(',').any(|tag| {
        let t = tag.trim();
        if t == "*" {
            return true;
        }
        let t = t.strip_prefix("W/").unwrap_or(t).trim_matches('"');
        t == want
    })
}

fn static_asset(
    body: &'static [u8],
    content_type: &'static str,
    cache_control: &'static str,
    headers: &HeaderMap,
) -> Response {
    let etag = content_etag(body);
    if if_none_match(headers, &etag) {
        let mut map = HeaderMap::new();
        map.insert(header::ETAG, HeaderValue::from_str(&etag).expect("etag"));
        map.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
        return (StatusCode::NOT_MODIFIED, map).into_response();
    }

    let mut map = HeaderMap::new();
    map.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    map.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    map.insert(header::ETAG, HeaderValue::from_str(&etag).expect("etag"));
    (map, body).into_response()
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
    let reason = if ctx.enricher.has_blockfrost() {
        "cache_miss_blockfrost_failed"
    } else {
        "cache_miss_no_blockfrost"
    };
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "transaction not found",
            "reason": reason,
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct EventsQuery {
    before: Option<u64>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct BufferQuery {
    /// Only return retention-window events with id < before (newest page of older).
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

/// In-memory retention window for client-side search / feed hydrate.
///
/// - No query → full window (compat).
/// - `?before=&limit=` → one page for progressive hydrate (never disk).
async fn api_buffer(State(ctx): State<ServerCtx>, Query(q): Query<BufferQuery>) -> Response {
    if q.before.is_some() || q.limit.is_some() {
        let before = q.before.unwrap_or(u64::MAX);
        let limit = q.limit.unwrap_or(2_500);
        return Json(ctx.state.retention_buffer_page(before, limit)).into_response();
    }
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

/// Preferred ADA Handle for a stake/payment address (`handle` may be null).
async fn api_handle(State(ctx): State<ServerCtx>, Path(addr): Path<String>) -> Response {
    Json(ctx.enricher.handle(&addr).await).into_response()
}

/// Full in-memory DRep name map for browser-side labels (no per-id round-trips).
async fn api_dreps(State(ctx): State<ServerCtx>) -> Response {
    Json(ctx.enricher.dreps_json()).into_response()
}

async fn api_gov_action(
    State(ctx): State<ServerCtx>,
    Path((tx, index)): Path<(String, u64)>,
) -> Response {
    Json(ctx.enricher.gov_action(&tx, index).await).into_response()
}

/// Full in-memory gov-action title map for browser-side labels.
async fn api_gov_actions(State(ctx): State<ServerCtx>) -> Response {
    Json(ctx.enricher.gov_actions_json()).into_response()
}

#[derive(Deserialize)]
struct VoteRationaleQuery {
    url: String,
}

/// Proxy + normalize a vote's CIP-100/136 rationale JSON (avoids browser CORS).
async fn api_vote_rationale(
    State(ctx): State<ServerCtx>,
    Query(q): Query<VoteRationaleQuery>,
) -> Response {
    let url = q.url.trim();
    if url.is_empty() || url.len() > 2048 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "bad url" })),
        )
            .into_response();
    }
    match ctx.enricher.vote_rationale(url).await {
        Some(v) => Json(v).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "rationale unavailable", "url": url })),
        )
            .into_response(),
    }
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
