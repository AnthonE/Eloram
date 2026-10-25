//! Network side of the server: WebTransport sessions. Code here may allocate and use tokio
//! primitives; it talks to the shard threads only through the lock-free queues in `el-match`.

pub mod admit;
pub mod echo;
pub mod endpoint;
mod session;

use std::sync::atomic::{AtomicU64, Ordering};

/// Counters maintained by the network tasks (all relaxed; they're only for `/status`).
#[derive(Default)]
pub struct NetStats {
    pub sessions_total: AtomicU64,
    pub sessions_active: AtomicU64,
    pub sessions_rejected: AtomicU64,
    pub datagrams_in: AtomicU64,
    pub datagrams_out: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub malformed: AtomicU64,
    /// Connection attempts turned away (`admit`): the server full, an address over its share,
    /// sent a Retry to prove their address, or too slow to finish their handshakes.
    pub refused_full: AtomicU64,
    pub refused_address: AtomicU64,
    pub retried: AtomicU64,
    pub handshake_timeouts: AtomicU64,
    /// Sessions open now past their Hello, by who's at the controls: people (the store's player
    /// count, `/status.json`) and agents.
    pub humans_active: AtomicU64,
    pub agents_active: AtomicU64,
}

impl NetStats {
    #[inline]
    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }
}

/// The `/status` document.
pub fn status_json(stats: &NetStats, mode: crate::Mode) -> serde_json::Value {
    let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
    serde_json::json!({
        "mode": match mode { crate::Mode::Game => "game", crate::Mode::Echo => "echo" },
        "hot_path_allocations": el_alloc::violations(),
        "net": {
            "sessions_total": r(&stats.sessions_total),
            "sessions_active": r(&stats.sessions_active),
            "sessions_rejected": r(&stats.sessions_rejected),
            "datagrams_in": r(&stats.datagrams_in),
            "datagrams_out": r(&stats.datagrams_out),
            "bytes_in": r(&stats.bytes_in),
            "bytes_out": r(&stats.bytes_out),
            "malformed": r(&stats.malformed),
            "refused_full": r(&stats.refused_full),
            "refused_address": r(&stats.refused_address),
            "retried": r(&stats.retried),
            "handshake_timeouts": r(&stats.handshake_timeouts),
            "humans_active": r(&stats.humans_active),
            "agents_active": r(&stats.agents_active),
        },
    })
}

/// The `/status.json` document, in the shape the Elo Pros launcher and store poll (Gates' shard
/// and Before Colony's server serve the same): `players` are the people connected (agents
/// apart), `max_players` the places.
pub fn launcher_status_json(stats: &NetStats, max_players: usize) -> serde_json::Value {
    let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
    serde_json::json!({
        "players": r(&stats.humans_active),
        "agents": r(&stats.agents_active),
        "max_players": max_players,
        // What a page must speak to be let in (`scripts/publish-web.sh` checks it before publishing).
        "proto": el_proto::PROTOCOL_VERSION,
    })
}
