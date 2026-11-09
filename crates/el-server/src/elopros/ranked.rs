//! The RANKED queue: the store's matchmaker (`POST /api/lobbies/serve/eloram`, signed with the
//! server key) puts ten signed-in wallets together by rating; this server seats a match it made
//! on this server, side 0 in seats 0 to 4 and side 1 in 5 to 9, no bots, and reports the result
//! with the store's id (`report::ratings`).
//!
//! A wallet queued here waits in [`Ranked`] (its session's lobby details kept to seat it with)
//! while the store has it queued. Every [`POLL`] the waiters are polled together: each hears how
//! long it has waited; a match made on this server is read (`GET /lobbies/match/{id}`) and seated
//! once all ten of its players are waiting here; one whose players don't all come within
//! [`GATHER`] puts the ones who did back in the queue. A wait the store gives up (its own expiry,
//! no server with room) is said to the player and not tried again. Parties are not taken (yet):
//! everyone queues alone.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_client_core::lobby::ToClient;
use serde_json::json;

use super::EloPros;
use crate::lobby::{Lobby, LobbyEvent, Waiting};

/// How often the waiters are polled.
const POLL: Duration = Duration::from_secs(3);
/// How long a match made here waits for its ten.
const GATHER: Duration = Duration::from_secs(30);
const QUEUE_ROUTE: &str = "/lobbies/serve/eloram";

pub(crate) struct Waiter {
    pub(crate) player: String,
    pub(crate) since: Instant,
    pub(crate) w: Waiting,
}

#[derive(Default)]
pub struct Ranked {
    waiting: Mutex<Vec<Waiter>>,
    /// Matches made here, waiting for their players: when first seen.
    gathering: Mutex<BTreeMap<String, Instant>>,
}

fn tell(w: &Waiting, msg: ToClient) {
    let _ = w.tx.send(LobbyEvent::Tell(msg));
}

fn state(state: &str, waited_s: u32, why: Option<String>) -> ToClient {
    ToClient::Ranked { state: state.into(), waited_s, why }
}

impl Ranked {
    pub fn count(&self) -> usize {
        self.waiting.lock().map_or(0, |w| w.len())
    }

    fn take(&self, session: u32) -> Option<Waiter> {
        let mut w = self.waiting.lock().ok()?;
        let i = w.iter().position(|x| x.w.session == session)?;
        Some(w.swap_remove(i))
    }

    fn put(&self, waiter: Waiter) {
        if let Ok(mut w) = self.waiting.lock() {
            w.retain(|x| x.w.session != waiter.w.session && x.player != waiter.player);
            w.push(waiter);
        }
    }
}

/// `w` (signed in as `player`) into the store's ranked queue.
pub(crate) fn join(elo: &Arc<EloPros>, player: String, w: Waiting) {
    let elo = elo.clone();
    tokio::spawn(async move {
        let body = json!({"op": "queue", "players": [player], "ladder": super::report::LADDER, "teams": 2, "size": 5});
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let api = elo.clone();
        let got = tokio::task::spawn_blocking(move || api.api.post(QUEUE_ROUTE, &bytes)).await;
        match got {
            Ok(Ok(a)) if a.ok() => {
                tell(&w, state("waiting", 0, None));
                elo.ranked.put(Waiter { player, since: Instant::now(), w });
            }
            Ok(Ok(a)) => tell(
                &w,
                ToClient::Refused { what: "ranked".into(), why: format!("The ranked queue: {}", a.why()) },
            ),
            _ => tell(
                &w,
                ToClient::Refused {
                    what: "ranked".into(),
                    why: "Elo Pros' ranked queue can't be reached just now.".into(),
                },
            ),
        }
    });
}

/// `session` out of the ranked queue (it left, or went).
pub fn leave(elo: &Arc<EloPros>, session: u32) {
    let Some(waiter) = elo.ranked.take(session) else { return };
    let elo = elo.clone();
    tokio::spawn(async move {
        let bytes =
            serde_json::to_vec(&json!({"op": "unqueue", "players": [waiter.player]})).unwrap_or_default();
        let _ = tokio::task::spawn_blocking(move || elo.api.post(QUEUE_ROUTE, &bytes)).await;
    });
}

/// Polls the waiters and seats the matches made here, for as long as the server runs.
pub async fn run(elo: Arc<EloPros>, lobby: Arc<Lobby>) {
    let mut tick = tokio::time::interval(POLL);
    loop {
        tick.tick().await;
        let players: Vec<String> = match elo.ranked.waiting.lock() {
            Ok(w) if !w.is_empty() => w.iter().map(|x| x.player.clone()).collect(),
            _ => continue,
        };
        let e = elo.clone();
        let Ok(Ok(a)) = tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&json!({"op": "poll", "players": players})).unwrap_or_default();
            e.api.post(QUEUE_ROUTE, &bytes)
        })
        .await
        else {
            continue;
        };
        if !a.ok() {
            tracing::info!("the ranked poll: {}", a.why());
            continue;
        }
        let mut matches = Vec::new();
        for p in a.json()["players"].as_array().into_iter().flatten() {
            let Some(player) = p["player"].as_str() else { continue };
            let st = p["state"].as_str().unwrap_or("");
            let Ok(mut waiting) = elo.ranked.waiting.lock() else { continue };
            let Some(i) = waiting.iter().position(|x| x.player == player) else { continue };
            match (st, p["match"].as_str()) {
                ("queued", _) => {
                    let s = p["waited_s"].as_u64().unwrap_or(0).min(u64::from(u32::MAX)) as u32;
                    tell(&waiting[i].w, state("waiting", s, None));
                }
                ("launched", Some(id)) => {
                    if p["server"].as_str() != elo.public_addr.as_deref() {
                        let x = waiting.swap_remove(i);
                        tell(&x.w, state("failed", 0, Some("matched onto another server".into())));
                    } else if !matches.iter().any(|m: &String| m == id) {
                        matches.push(id.to_string());
                    }
                }
                _ => {
                    let x = waiting.swap_remove(i);
                    let why = p["unqueued_why"].as_str().unwrap_or("the store let the wait go").to_string();
                    tell(&x.w, state("expired", 0, Some(why)));
                }
            }
        }
        for id in matches {
            seat(&elo, &lobby, &id).await;
        }
    }
}

/// A match the store made here: seated if its ten are waiting, else waited for a while, else the
/// ones who came are queued again.
async fn seat(elo: &Arc<EloPros>, lobby: &Arc<Lobby>, id: &str) {
    let e = elo.clone();
    let route = format!("/lobbies/match/{id}");
    let Ok(Ok(a)) = tokio::task::spawn_blocking(move || e.api.get(&route)).await else { return };
    if !a.ok() {
        return;
    }
    let m = a.json();
    let sides: Vec<Vec<String>> = m["sides"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            s["players"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect()
        })
        .collect();
    let all: Vec<&String> = sides.iter().flatten().collect();
    let first = elo
        .ranked
        .gathering
        .lock()
        .map(|mut g| *g.entry(id.to_string()).or_insert_with(Instant::now))
        .unwrap_or_else(|_| Instant::now());
    let here = elo
        .ranked
        .waiting
        .lock()
        .map_or(0, |w| all.iter().filter(|p| w.iter().any(|x| &x.player == **p)).count());
    if sides.len() != 2 || here < all.len() {
        if first.elapsed() < GATHER {
            if let Ok(w) = elo.ranked.waiting.lock() {
                for x in w.iter().filter(|x| all.contains(&&x.player)) {
                    tell(&x.w, state("gathering", x.since.elapsed().as_secs() as u32, None));
                }
            }
            return;
        }
        // Not everyone came: the ones who did wait again.
        let came: Vec<Waiter> = elo
            .ranked
            .waiting
            .lock()
            .map(|mut w| {
                let (came, rest): (Vec<Waiter>, Vec<Waiter>) =
                    w.drain(..).partition(|x| all.contains(&&x.player));
                *w = rest;
                came
            })
            .unwrap_or_default();
        if let Ok(mut g) = elo.ranked.gathering.lock() {
            g.remove(id);
        }
        for x in came {
            tell(&x.w, state("waiting", 0, Some("not everyone matched came: queued again".into())));
            join(elo, x.player, x.w);
        }
        return;
    }
    let taken: Vec<Waiter> = elo
        .ranked
        .waiting
        .lock()
        .map(|mut w| {
            let (taken, rest): (Vec<Waiter>, Vec<Waiter>) =
                w.drain(..).partition(|x| all.contains(&&x.player));
            *w = rest;
            taken
        })
        .unwrap_or_default();
    if let Ok(mut g) = elo.ranked.gathering.lock() {
        g.remove(id);
    }
    let mut pool: Vec<Option<Waiter>> = taken.into_iter().map(Some).collect();
    let mut side_of = |players: &[String]| -> Vec<Waiting> {
        players
            .iter()
            .filter_map(|p| {
                pool.iter_mut().find(|x| x.as_ref().is_some_and(|x| &x.player == p))?.take().map(|x| x.w)
            })
            .collect()
    };
    let a = side_of(&sides[0]);
    let b = side_of(&sides[1]);
    if let Err(back) = lobby.start_ranked(id, [a, b]) {
        tracing::warn!(ranked = id, "no room for a ranked match made here");
        for w in back.into_iter().flatten() {
            tell(&w, state("failed", 0, Some("this server had no room for the match".into())));
        }
    }
}
