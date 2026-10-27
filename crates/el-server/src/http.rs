//! The HTTP side: serves the built web client, `/cert-hash` for WebTransport's
//! `serverCertificateHashes` (its `hash` is null when the certificate is CA-issued: nothing to pin),
//! `/status` metrics and `/status.json`, the player count in the shape the Elo Pros launcher and
//! store poll; `/online.json`, the signed-in wallets on now (the store's playtime); and the
//! matches' records (`/records.json`, `/records/{id}`, and `/records/{id}/checkpoint`, the last
//! checkpoint the referee verified within what's served, for a spectator to start from). The JSON routes answer pages on the store's origin too (CORS, `Config::cors_origins`),
//! since the store serves the game's page from its own site.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

type StatusFn = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

#[derive(Clone)]
pub struct HttpState {
    /// The self-signed certificate's SHA-256 for browsers to pin; None with a CA-issued one.
    pub cert_hash_hex: Option<String>,
    pub wt_port: u16,
    pub web_dir: Option<PathBuf>,
    pub status: StatusFn,
    /// `/status.json`: `{players, agents, max_players}`.
    pub launcher_status: StatusFn,
    /// Origins allowed to read the JSON routes from another site.
    pub cors_origins: Arc<[String]>,
    /// The matches' records (game mode).
    pub records: Option<Arc<std::sync::Mutex<crate::records::RecordStore>>>,
    /// `/online.json`: the signed-in wallets on now, for the store's playtime.
    pub online: StatusFn,
}

pub fn router(state: HttpState) -> Router {
    let web_dir = state.web_dir.clone();
    let origins = state.cors_origins.clone();
    let api = Router::new()
        .route("/cert-hash", get(cert_hash))
        .route("/status", get(status))
        .route("/status.json", get(launcher_status))
        .route("/online.json", get(online))
        .route("/records.json", get(records_list))
        .route("/records/{id}", get(record))
        .route("/records/{id}/checkpoint", get(checkpoint))
        .with_state(state)
        // Gzipped for those who ask: a checkpoint's dead slots take it from 156 KB to about 5.
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let origins = origins.clone();
            async move { cors(&origins, req, next).await }
        }));
    let mut router = api;
    if let Some(dir) = web_dir {
        router = router.fallback_service(ServeDir::new(dir).precompressed_br().precompressed_gzip());
    }
    // Dev server: never let the browser run a stale wasm build.
    router.layer(SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    ))
}

/// The `Access-Control-Allow-Origin` a request from `headers` gets: its own `Origin`, when that's
/// one of `allowed`.
pub fn allowed_origin(allowed: &[String], headers: &HeaderMap) -> Option<HeaderValue> {
    let origin = headers.get(header::ORIGIN)?;
    let text = origin.to_str().ok()?;
    allowed.iter().any(|a| a == text).then(|| origin.clone())
}

async fn cors(allowed: &[String], req: Request, next: Next) -> Response {
    let allow = allowed_origin(allowed, req.headers());
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    if let Some(origin) = allow {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    res
}

async fn cert_hash(State(s): State<HttpState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "algorithm": "sha-256",
        "hash": s.cert_hash_hex,
        "port": s.wt_port,
        "path": crate::net::endpoint::WT_PATH,
    }))
}

async fn status(State(s): State<HttpState>) -> Json<serde_json::Value> {
    Json((s.status)())
}

async fn launcher_status(State(s): State<HttpState>) -> Json<serde_json::Value> {
    Json((s.launcher_status)())
}

async fn online(State(s): State<HttpState>) -> Json<serde_json::Value> {
    Json((s.online)())
}

/// The records kept: the live matches' and the last finished ones.
async fn records_list(State(s): State<HttpState>) -> Json<serde_json::Value> {
    let list = s.records.as_ref().and_then(|r| r.lock().ok().map(|r| r.list()));
    Json(list.unwrap_or_else(|| serde_json::json!([])))
}

/// One record, as far as it may be served (a live match's a minute behind).
async fn record(
    State(s): State<HttpState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    use axum::response::IntoResponse;
    let id = id.trim_end_matches(".elr");
    let bytes = s.records.as_ref().and_then(|r| r.lock().ok().and_then(|r| r.get(id)));
    match bytes {
        Some(b) => ([(header::CONTENT_TYPE, "application/octet-stream")], b).into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// The checkpoint a spectator of record `id` may start from (a live match's at least a minute
/// behind, as its record is).
async fn checkpoint(
    State(s): State<HttpState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    use axum::response::IntoResponse;
    let bytes = s.records.as_ref().and_then(|r| r.lock().ok().and_then(|r| r.checkpoint(&id)));
    match bytes {
        Some(b) => ([(header::CONTENT_TYPE, "application/octet-stream")], b.to_vec()).into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_origins_are_answered() {
        let allowed = [crate::STORE_ORIGIN.to_string()];
        let mut h = HeaderMap::new();
        assert_eq!(allowed_origin(&allowed, &h), None);
        h.insert(header::ORIGIN, HeaderValue::from_static("https://elopros.com"));
        assert_eq!(allowed_origin(&allowed, &h), Some(HeaderValue::from_static("https://elopros.com")));
        h.insert(header::ORIGIN, HeaderValue::from_static("https://elopros.com.evil.example"));
        assert_eq!(allowed_origin(&allowed, &h), None);
        h.insert(header::ORIGIN, HeaderValue::from_static("http://elopros.com"));
        assert_eq!(allowed_origin(&allowed, &h), None);
    }
}
