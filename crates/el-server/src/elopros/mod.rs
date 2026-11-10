//! Elo Pros from the server's side: the server's key and its signed calls (`key`, `api`), the
//! reports a finished match owes, kept on disk until they're taken (`report`, `outbox`), and the
//! RANKED queue the store's matchmaker fills (`ranked`). All of it is off unless the server is
//! given a key (`--server-key`); ranked also wants `--ranked` and the address the store lists
//! this server at (`--public-addr`).
//!
//! Nothing here runs on a shard thread or in the lobby's lock for longer than a file write: the
//! calls go on tokio's blocking pool.

pub mod api;
pub mod key;
pub mod outbox;
pub mod ranked;
pub mod report;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the server is told of Elo Pros.
#[derive(Clone, Debug, Default)]
pub struct EloConfig {
    /// The API's root (`https://elopros.com/api`).
    pub api: String,
    /// The server key's PEM file, and the id it's named by in `elo.json`'s `server_keys`.
    pub key_file: PathBuf,
    pub key_id: String,
    /// `host:port` as the title's server list names this server: the store's matchmaker puts a
    /// ranked match on a server by it.
    pub public_addr: Option<String>,
    /// Where this server's records are served from, publicly (`https://.../records`): a ranked
    /// match's result names its record there.
    pub records_url: Option<String>,
    /// Hold ranked matches.
    pub ranked: bool,
}

pub struct EloPros {
    pub api: api::Api,
    pub outbox: outbox::Outbox,
    pub ranked: ranked::Ranked,
    pub ranked_on: bool,
    pub public_addr: Option<String>,
    pub records_url: Option<String>,
    pub records: Option<PathBuf>,
    /// What the store said of the key at startup (`/serverkeys/check`): `ok`, or why not.
    pub key_state: Mutex<String>,
}

impl EloPros {
    pub fn new(cfg: &EloConfig, records: Option<PathBuf>) -> Result<Arc<EloPros>, String> {
        let pem =
            std::fs::read_to_string(&cfg.key_file).map_err(|e| format!("{}: {e}", cfg.key_file.display()))?;
        let key = key::ServerKey::from_pem(key::GAME, &cfg.key_id, &pem)?;
        if cfg.ranked && cfg.public_addr.is_none() {
            return Err(
                "--ranked needs --public-addr: the store puts a match on a server by its address".into()
            );
        }
        Ok(Arc::new(EloPros {
            api: api::Api::new(&cfg.api, key),
            outbox: outbox::Outbox::new(records.as_deref()),
            ranked: ranked::Ranked::default(),
            ranked_on: cfg.ranked,
            public_addr: cfg.public_addr.clone(),
            records_url: cfg.records_url.clone(),
            records,
            key_state: Mutex::new("unchecked".into()),
        }))
    }

    /// Asks the store whether it knows this key, then keeps the outbox going and (with ranked)
    /// the queue.
    pub fn start(self: &Arc<Self>, lobby: Arc<crate::lobby::Lobby>) {
        let e = self.clone();
        tokio::spawn(async move {
            let api = e.clone();
            let got = tokio::task::spawn_blocking(move || api.api.post("/serverkeys/check", b"")).await;
            let said = match got {
                Ok(Ok(a)) if a.ok() => "ok".to_string(),
                Ok(Ok(a)) => a.why(),
                Ok(Err(err)) => err,
                Err(err) => err.to_string(),
            };
            if said == "ok" {
                tracing::info!(key = e.api.key.who().as_str(), "Elo Pros knows this server's key");
            } else {
                tracing::warn!(
                    key = e.api.key.who().as_str(),
                    "Elo Pros doesn't take this server's key yet: {said}"
                );
            }
            if let Ok(mut k) = e.key_state.lock() {
                *k = said;
            }
        });
        tokio::spawn(outbox::run(self.clone()));
        if self.ranked_on {
            tokio::spawn(ranked::run(self.clone(), lobby));
        }
    }

    /// `/status`'s line.
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "key": self.key_state.lock().map_or_else(|_| String::new(), |k| k.clone()),
            "signer": self.api.key.who(),
            "outbox": self.outbox.waiting().len(),
            "ranked": self.ranked_on,
            "ranked_waiting": self.ranked.count(),
        })
    }
}
