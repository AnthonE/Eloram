//! Eloram's game server: WebTransport sessions on tokio, shard threads stepping the matches, and a
//! small HTTP side for the page, the certificate hash and the store's status polls.

pub mod game;
pub mod http;
pub mod lobby;
pub mod net;
pub mod records;
pub mod recover;
pub mod referee;
pub mod rerolls;
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
    /// Shard threads, and the matches each holds.
    pub shards: usize,
    pub matches_per_shard: usize,
    /// How long the first in the queue waits before bots fill the empty seats.
    pub fill_after: Duration,
    /// How long champion select lasts, unless everyone seated is ready sooner (0: none).
    pub select_for: Duration,
    /// Where records are written (`{id}.elr`, and the live ones' parts and checkpoints), and the
    /// state a restart takes up: the self-signed certificate, the resume tokens' hashes, the
    /// matches left running (`recover`).
    pub records_dir: Option<PathBuf>,
    /// A referee re-steps every live match's record and checks its checkpoints (`referee`).
    pub referee: bool,
    /// Seconds between a match's checkpoints (with the referee).
    pub checkpoint_secs: f64,
    /// The domains a wallet sign-in may name (`--siwe-domain`): the pages' hosts. Empty: this
    /// server's own HTTP address.
    pub siwe_domains: Vec<String>,
    /// Elo Pros' account key (`GET /api/account/pubkey`). None: no pass is taken, and `/nonce` says so.
    pub elo_pubkey: Option<[u8; 32]>,
    /// People must sign in with a wallet (agents may still play as guests).
    pub require_sign_in: bool,
    /// Practice matches may be asked for (`el_sim::practice`, `ToServer::Practice`).
    pub practice: bool,
    /// How long a self-signed certificate is used before a running server remakes it, and how
    /// often it looks (`net::endpoint::rotate`; a CA-issued one is read again as often).
    pub cert_keep: Duration,
    pub cert_check: Duration,
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
            shards: 1,
            matches_per_shard: 64,
            fill_after: Duration::from_secs(30),
            select_for: lobby::SELECT_FOR,
            records_dir: None,
            referee: true,
            checkpoint_secs: 60.0,
            siwe_domains: Vec::new(),
            elo_pubkey: None,
            require_sign_in: false,
            practice: false,
            cert_keep: net::endpoint::KEEP_CERT,
            cert_check: net::endpoint::CHECK_CERT,
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
    reload: net::endpoint::Reload,
}

impl Handle {
    /// Reads the certificate again now (SIGHUP; a self-signed one is remade): new handshakes
    /// get the new one, nobody is dropped. `net::endpoint::rotate` does it.
    pub fn reload(&self) {
        self.reload.notify_one();
    }

    /// Stops the server: the matches where they stand, the last of their records taken off the
    /// rings (a restart with the same records directory takes them up again).
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        if let Some(l) = &self.lobby {
            l.game.halt();
            l.game.join_recorders();
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
    if let Some(dir) = &cfg.records_dir {
        std::fs::create_dir_all(dir)?;
    }
    let (identity, hash, pinned, source) = match &cfg.tls {
        Some((cert, key)) => {
            let (id, hash) = net::endpoint::pem_identity(cert, key).await?;
            (id, hash, false, net::endpoint::TlsSource::Pem { cert: cert.clone(), key: key.clone() })
        }
        None => {
            let (id, hash) =
                net::endpoint::kept_self_signed_identity(cfg.records_dir.as_deref(), cfg.cert_keep).await?;
            let source =
                net::endpoint::TlsSource::SelfSigned { dir: cfg.records_dir.clone(), keep: cfg.cert_keep };
            (id, hash, true, source)
        }
    };
    let (endpoint, bind) = net::endpoint::make_endpoint(cfg.wt_port, identity)?;
    let endpoint = Arc::new(endpoint);
    let wt_port = endpoint.local_addr()?.port();
    let cert_hash: net::endpoint::CertHash =
        Arc::new(std::sync::RwLock::new(pinned.then(|| hex::encode(hash))));
    let reload: net::endpoint::Reload = Arc::new(tokio::sync::Notify::new());
    let stats = Arc::new(NetStats::default());
    let admission = Admission::new(cfg.limits);
    let (shutdown, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(net::endpoint::rotate(
        endpoint.clone(),
        source,
        bind,
        cfg.wt_port,
        cfg.cert_check,
        hash,
        cert_hash.clone(),
        reload.clone(),
        rx.clone(),
    ));
    let lobby = match cfg.mode {
        Mode::Game => {
            let map = el_sim::map::Map::leaked();
            let (game, recovered) = game::Game::start(
                cfg.shards.max(1),
                cfg.matches_per_shard.max(1),
                map,
                cfg.records_dir.clone(),
                cfg.referee,
                el_match::queues::checkpoint_ticks(cfg.checkpoint_secs),
            )?;
            let lobby = lobby::Lobby::with_state(
                game,
                cfg.fill_after,
                cfg.select_for,
                cfg.records_dir.clone(),
                recovered,
            );
            if cfg.practice {
                lobby.allow_practice();
            }
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
    let mut signin = signin::SignIn::new(domains, cfg.require_sign_in);
    if let Some(dir) = &cfg.records_dir {
        signin.tokens = signin::Tokens::kept(dir.join("tokens.json"));
    }
    signin.pass_key = cfg.elo_pubkey;
    let signin = Arc::new(signin);
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
    let nonces = signin.clone();
    // The places `/status.json` names are the connections the server takes: a launcher or the
    // store told of more would send players the admission refuses.
    let max_players = cfg.limits.connections as usize;
    let status_stats = stats.clone();
    let launcher_stats = stats.clone();
    let status_lobby = lobby.clone();
    let state = http::HttpState {
        cert_hash_hex: cert_hash,
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
        nonce: Arc::new(move || {
            let nonce = nonces.pass_key.map(|_| nonces.nonces.issue());
            serde_json::json!({ "nonce": nonce, "game": el_auth::GAME })
        }),
    };
    let mut http_shutdown = rx.clone();
    tokio::spawn(async move {
        let serve = axum::serve(listener, http::router(state)).with_graceful_shutdown(async move {
            let _ = http_shutdown.changed().await;
        });
        if let Err(e) = serve.await {
            tracing::error!("http: {e}");
        }
    });
    tokio::spawn(journal(stats, lobby.clone(), rx));
    Ok(Handle { http_addr, wt_port, lobby, shutdown, reload })
}

/// How often the journal gets a status line, and what it says: what `/status` says, in one line,
/// every ten seconds that something in it moved (oneshot's `health.rs` line, after Gates'
/// `status.rs`): the players and agents on, the matches live and queued, the datagrams in and
/// out, and what should stay at zero (connections refused, bad datagrams, shard overruns, output
/// drops, records astray, referee divergences, hot-path allocations).
pub const JOURNAL: Duration = Duration::from_secs(10);

async fn journal(
    stats: Arc<NetStats>,
    lobby: Option<Arc<lobby::Lobby>>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut last = String::new();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(JOURNAL) => {}
            _ = shutdown.changed() => return,
        }
        let line = status_line(&stats, lobby.as_deref());
        if line != last {
            tracing::info!("status: {line}");
            last = line;
        }
    }
}

/// The journal's line (`journal`).
pub fn status_line(stats: &NetStats, lobby: Option<&lobby::Lobby>) -> String {
    let r = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
    let mut line = format!(
        "humans {} agents {} | in {} out {} datagrams | refused {} bad {} timeouts {}",
        r(&stats.humans_active),
        r(&stats.agents_active),
        r(&stats.datagrams_in),
        r(&stats.datagrams_out),
        r(&stats.sessions_rejected) + r(&stats.refused_full) + r(&stats.refused_address),
        r(&stats.malformed),
        r(&stats.handshake_timeouts),
    );
    if let Some(l) = lobby {
        let (live, queued, started) = l.counts();
        let (overruns, out_drops, failed) = l.game.shards.iter().fold((0, 0, 0), |(o, d, f), s| {
            let m = &s.shared.metrics;
            (o + r(&m.overruns), d + r(&m.out_drops), f + r(&m.failed))
        });
        let astray = l.game.records.lock().map_or(0, |rec| rec.astray());
        let divergences = l.game.referee.json()["divergences"].as_u64().unwrap_or(0);
        line.push_str(&format!(
            " | matches {live} queued {queued} started {started} | overruns {overruns} drops {out_drops} failed {failed} astray {astray} divergences {divergences}"
        ));
    }
    let allocs = el_alloc::violations();
    if allocs > 0 {
        line.push_str(&format!(" | HOT-PATH ALLOCATIONS {allocs}"));
    }
    line
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
                "record_bytes": r(&m.record_bytes),
                "record_drops": r(&m.record_drops),
                "checkpoints": r(&m.checkpoints),
                "checkpoint_drops": r(&m.checkpoint_drops),
                "failed": r(&m.failed),
                "sent": r(&e.datagrams),
                "sent_bytes": r(&e.bytes),
                "refused": r(&e.refused),
            })
        })
        .collect();
    let (astray, written) = lobby.game.records.lock().map_or((0, 0), |r| (r.astray(), r.written));
    serde_json::json!({
        "matches": live,
        "queued": queued,
        "matches_started": started,
        "shards": shards,
        "referee": lobby.game.referee.json(),
        "records": { "written": written, "astray": astray },
    })
}
