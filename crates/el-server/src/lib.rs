//! Eloram's game server: WebTransport sessions on tokio, shard threads stepping the matches, and a
//! small HTTP side for the page, the certificate hash and the store's status polls.

pub mod http;
pub mod net;
pub mod telemetry;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use net::NetStats;
use net::admit::{Admission, Limits};

/// The store's origin: its pages may read the JSON routes.
pub const STORE_ORIGIN: &str = "https://elopros.com";

/// What the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// The game: the lobby and the matches.
    Game,
    /// Datagrams and control-stream bytes bounced back: the transport smoke test.
    Echo,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub wt_port: u16,
    pub http_addr: SocketAddr,
    pub web_dir: Option<PathBuf>,
    /// A CA-issued certificate and key (PEM); without them the server self-signs.
    pub tls: Option<(PathBuf, PathBuf)>,
    pub cors_origins: Vec<String>,
    pub limits: Limits,
    /// Places for players, for `/status.json`.
    pub max_players: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Game,
            wt_port: 4433,
            http_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
            web_dir: None,
            tls: None,
            cors_origins: vec![STORE_ORIGIN.to_string()],
            limits: Limits::default(),
            max_players: 640,
        }
    }
}

/// A command line's list: repeated flags and comma-separated values alike, trimmed, empties
/// dropped (`--cors-origin https://elopros.com,https://example.com`).
pub fn split_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

/// A running server.
pub struct Handle {
    pub http_addr: SocketAddr,
    pub wt_port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Handle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Installs the process-wide rustls crypto provider (ring, the same one wtransport uses). Safe to
/// call more than once.
pub fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Starts the WebTransport endpoint and the HTTP side; returns once both are listening.
pub async fn start(cfg: Config) -> anyhow::Result<Handle> {
    install_crypto();
    let (identity, hash, pinned) = match &cfg.tls {
        Some((cert, key)) => {
            let (id, hash) = net::endpoint::pem_identity(cert, key).await?;
            (id, hash, false)
        }
        None => {
            let (id, hash) = net::endpoint::self_signed_identity()?;
            (id, hash, true)
        }
    };
    let endpoint = net::endpoint::make_endpoint(cfg.wt_port, identity)?;
    let wt_port = endpoint.local_addr()?.port();
    let stats = Arc::new(NetStats::default());
    let admission = Admission::new(cfg.limits);
    let (shutdown, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(net::endpoint::accept_loop(endpoint, cfg.mode, stats.clone(), admission, rx.clone()));

    let mode = cfg.mode;
    let max_players = cfg.max_players;
    let status_stats = stats.clone();
    let launcher_stats = stats.clone();
    let state = http::HttpState {
        cert_hash_hex: pinned.then(|| hex::encode(hash)),
        wt_port,
        web_dir: cfg.web_dir.clone(),
        status: Arc::new(move || net::status_json(&status_stats, mode)),
        launcher_status: Arc::new(move || net::launcher_status_json(&launcher_stats, max_players)),
        cors_origins: cfg.cors_origins.clone().into(),
    };
    let listener = tokio::net::TcpListener::bind(cfg.http_addr).await?;
    let http_addr = listener.local_addr()?;
    let mut http_shutdown = rx;
    tokio::spawn(async move {
        let serve = axum::serve(listener, http::router(state)).with_graceful_shutdown(async move {
            let _ = http_shutdown.changed().await;
        });
        if let Err(e) = serve.await {
            tracing::error!("http: {e}");
        }
    });
    Ok(Handle { http_addr, wt_port, shutdown })
}
