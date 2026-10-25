//! Eloram's game server: WebTransport sessions on tokio, shard threads stepping the matches, and a
//! small HTTP side for the page, the certificate hash and the store's status polls.

pub mod game;
pub mod http;
pub mod lobby;
pub mod net;
pub mod records;
pub mod signin;
pub mod telemetry;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

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
    /// Shard threads, and the matches each holds.
    pub shards: usize,
    pub matches_per_shard: usize,
    /// How long the first in the queue waits before bots fill the empty seats.
    pub fill_after: Duration,
    /// Where finished records are written (`{id}.elr`), if anywhere.
    pub records_dir: Option<PathBuf>,
    /// The domains a wallet sign-in may name (`--siwe-domain`): the pages' hosts. Empty: this
    /// server's own HTTP address.
    pub siwe_domains: Vec<String>,
    /// People must sign in with a wallet (agents may still play as guests).
    pub require_sign_in: bool,
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
            shards: 1,
            matches_per_shard: 64,
            fill_after: Duration::from_secs(30),
            records_dir: None,
            siwe_domains: Vec::new(),
            require_sign_in: false,
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
    pub lobby: Option<Arc<lobby::Lobby>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Handle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        if let Some(l) = &self.lobby {
            l.game.halt();
        }
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
    let lobby = match cfg.mode {
        Mode::Game => {
            let map = el_sim::map::Map::leaked();
            if let Some(dir) = &cfg.records_dir {
                std::fs::create_dir_all(dir)?;
            }
            let game = game::Game::start(
                cfg.shards.max(1),
                cfg.matches_per_shard.max(1),
                map,
                cfg.records_dir.clone(),
            )?;
            let lobby = lobby::Lobby::new(game, cfg.fill_after);
            tokio::spawn(lobby.clone().run());
            Some(lobby)
        }
        Mode::Echo => None,
    };
    // The HTTP side's address first: a sign-in names it when no other domain is given.
    let listener = tokio::net::TcpListener::bind(cfg.http_addr).await?;
    let http_addr = listener.local_addr()?;
    let domains = if cfg.siwe_domains.is_empty() {
        let mut own = vec![http_addr.to_string()];
        if http_addr.ip().is_loopback() {
            own.push(format!("localhost:{}", http_addr.port()));
        }
        own
    } else {
        cfg.siwe_domains.clone()
    };
    let signin = Arc::new(signin::SignIn::new(domains, cfg.require_sign_in));
    tokio::spawn(net::endpoint::accept_loop(
        endpoint,
        cfg.mode,
        stats.clone(),
        admission,
        lobby.clone(),
        signin.clone(),
        rx.clone(),
    ));

    let mode = cfg.mode;
    let online = signin.online.clone();
    let max_players = cfg.max_players;
    let status_stats = stats.clone();
    let launcher_stats = stats.clone();
    let status_lobby = lobby.clone();
    let state = http::HttpState {
        cert_hash_hex: pinned.then(|| hex::encode(hash)),
        wt_port,
        web_dir: cfg.web_dir.clone(),
        status: Arc::new(move || {
            let mut doc = net::status_json(&status_stats, mode);
            if let Some(l) = &status_lobby {
                doc["game"] = game_json(l);
            }
            doc
        }),
        launcher_status: Arc::new(move || net::launcher_status_json(&launcher_stats, max_players)),
        cors_origins: cfg.cors_origins.clone().into(),
        records: lobby.as_ref().map(|l| l.game.records.clone()),
        online: Arc::new(move || online.json()),
    };
    let mut http_shutdown = rx;
    tokio::spawn(async move {
        let serve = axum::serve(listener, http::router(state)).with_graceful_shutdown(async move {
            let _ = http_shutdown.changed().await;
        });
        if let Err(e) = serve.await {
            tracing::error!("http: {e}");
        }
    });
    Ok(Handle { http_addr, wt_port, lobby, shutdown })
}

/// `/status`'s `game`: the lobby, and each shard's ticks, costs and traffic.
fn game_json(lobby: &lobby::Lobby) -> serde_json::Value {
    let (live, queued, started) = lobby.counts();
    let r = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
    let shards: Vec<_> = lobby
        .game
        .shards
        .iter()
        .map(|s| {
            let m = &s.shared.metrics;
            let e = s.egress_stats();
            serde_json::json!({
                "ticks": s.shared.tick.load(Ordering::Relaxed),
                "matches": r(&m.matches),
                "overruns": r(&m.overruns),
                "tick_us": { "p50": m.tick_hist.quantile_us(0.5), "p99": m.tick_hist.quantile_us(0.99) },
                "match_sim_us": { "p50": m.sim_hist.quantile_us(0.5), "p99": m.sim_hist.quantile_us(0.99) },
                "match_replicate_us": { "p50": m.replicate_hist.quantile_us(0.5), "p99": m.replicate_hist.quantile_us(0.99) },
                "inputs": r(&m.inputs),
                "snapshots": r(&m.snapshots),
                "snapshot_bytes": r(&m.snapshot_bytes),
                "snapshot_max_bytes": r(&m.snapshot_max_bytes),
                "out_drops": r(&m.out_drops),
                "sent": r(&e.datagrams),
                "sent_bytes": r(&e.bytes),
                "refused": r(&e.refused),
            })
        })
        .collect();
    serde_json::json!({
        "matches": live,
        "queued": queued,
        "matches_started": started,
        "shards": shards,
    })
}
