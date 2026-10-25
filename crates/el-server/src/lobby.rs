//! The lobby: who's waiting, and the matches they're in. Ten in the queue start a match at once;
//! fewer start one when the first of them has waited `fill_after`, the server's bots taking the
//! seats no one did. Champions are dealt at random, a team's five all different (ARAM: All Random).
//!
//! It runs on tokio and may lock: it is nowhere near a shard thread, which hears of a match only
//! through its control queue.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_client_core::lobby::{Player, ToClient};
use el_match::{Control, MatchState, Outcome, SeatLease, ShardShared};
use el_sim::content::champions::CHAMPIONS;
use el_sim::math::Rng;
use el_sim::units::SEATS;
use tokio::sync::mpsc::UnboundedSender;

use crate::game::Game;

/// How long a finished match is kept (its last snapshots say who won) before its place is freed.
const AFTER: Duration = Duration::from_secs(5);
/// Seats in the order people fill them: a side each in turn.
const SEAT_ORDER: [u8; SEATS] = [0, 5, 1, 6, 2, 7, 3, 8, 4, 9];

/// What the lobby tells a session.
pub enum LobbyEvent {
    Queued { waiting: usize, fill_in: u32 },
    Seated(Box<Seating>),
    Over { winner: u8 },
}

/// A session's seat in a match.
pub struct Seating {
    pub shard: usize,
    pub m: u16,
    pub seat: u8,
    pub g: u32,
    pub msg: ToClient,
    pub lease: SeatLease,
    pub shared: Arc<ShardShared>,
}

struct Waiting {
    session: u32,
    name: String,
    agent: bool,
    max_datagram: u16,
    tx: UnboundedSender<LobbyEvent>,
}

struct Live {
    shard: usize,
    m: u16,
    /// The sessions seated, by seat.
    sessions: Vec<(u32, u8, UnboundedSender<LobbyEvent>)>,
    over_at: Option<Instant>,
}

#[derive(Default)]
struct Inner {
    queue: Vec<Waiting>,
    /// When the first in the queue joined it.
    since: Option<Instant>,
    live: Vec<Live>,
    next: u64,
    started: u64,
}

pub struct Lobby {
    pub game: Game,
    fill_after: Duration,
    inner: Mutex<Inner>,
}

/// Each seat's champion for `seed`: each team's five dealt from the whole pool, none twice.
pub fn deal(seed: u64) -> [u8; SEATS] {
    let mut picks = [0u8; SEATS];
    for team in 0..2 {
        let mut rng = Rng::new(seed ^ (0xA5A5 + team as u64));
        let mut pool: Vec<u8> = (0..CHAMPIONS.len() as u8).collect();
        for k in 0..SEATS / 2 {
            if pool.is_empty() {
                pool = (0..CHAMPIONS.len() as u8).collect();
            }
            let i = rng.below(pool.len() as u32) as usize;
            picks[team * SEATS / 2 + k] = pool.swap_remove(i);
        }
    }
    picks
}

impl Lobby {
    pub fn new(game: Game, fill_after: Duration) -> Arc<Lobby> {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Arc::new(Lobby { game, fill_after, inner: Mutex::new(Inner { next: seed, ..Inner::default() }) })
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts a session in the queue (again, if it was).
    pub fn enqueue(
        &self,
        session: u32,
        name: &str,
        agent: bool,
        max_datagram: u16,
        tx: UnboundedSender<LobbyEvent>,
    ) {
        let mut inner = self.inner();
        inner.queue.retain(|w| w.session != session);
        inner.queue.push(Waiting { session, name: name.to_string(), agent, max_datagram, tx });
        if inner.since.is_none() {
            inner.since = Some(Instant::now());
        }
        let waiting = inner.queue.len();
        let left = self.fill_after.saturating_sub(inner.since.map_or(Duration::ZERO, |s| s.elapsed()));
        for w in &inner.queue {
            let _ = w.tx.send(LobbyEvent::Queued { waiting, fill_in: left.as_secs_f32().ceil() as u32 });
        }
    }

    /// Takes a session out of the queue.
    pub fn leave(&self, session: u32) {
        let mut inner = self.inner();
        inner.queue.retain(|w| w.session != session);
        if inner.queue.is_empty() {
            inner.since = None;
        }
    }

    /// A seated session is done with its seat (it left, or its match ended): the server's bot plays
    /// the seat from here, and its lease comes back. A match no one plays any more ends.
    pub fn unseat(&self, s: Seating) {
        let Seating { shard, m, seat, lease, shared, .. } = s;
        let mut inner = self.inner();
        let mut end = false;
        if let Some(l) = inner.live.iter_mut().find(|l| l.shard == shard && l.m == m) {
            l.sessions.retain(|(_, st, _)| *st != seat);
            end = l.sessions.is_empty();
        }
        let _ = shared.control.push(Control::Unseat { m, seat });
        self.game.detach(shard, lease.g);
        self.game.give_lease(shard, lease);
        if end {
            inner.live.retain(|l| !(l.shard == shard && l.m == m));
            let _ = shared.control.push(Control::End { m });
        }
    }

    /// Matches running, and sessions waiting.
    pub fn counts(&self) -> (usize, usize, u64) {
        let inner = self.inner();
        (inner.live.len(), inner.queue.len(), inner.started)
    }

    /// The lobby's loop: matches made, results passed on, finished matches cleared.
    pub async fn run(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        loop {
            tick.tick().await;
            self.pass();
        }
    }

    fn pass(&self) {
        let mut inner = self.inner();
        // Results.
        for (k, s) in self.game.shards.iter().enumerate() {
            while let Some(Outcome::Over { m, winner, tick, hash }) = s.shared.outcomes.pop() {
                tracing::info!(shard = k, m, winner, tick, hash = format_args!("{hash:016x}"), "match over");
                if let Some(l) = inner.live.iter_mut().find(|l| l.shard == k && l.m == m) {
                    l.over_at = Some(Instant::now());
                    for (_, _, tx) in &l.sessions {
                        let _ = tx.send(LobbyEvent::Over { winner });
                    }
                }
            }
        }
        let game = &self.game;
        inner.live.retain(|l| {
            let done = l.over_at.is_some_and(|t| t.elapsed() >= AFTER);
            if done {
                let _ = game.shards[l.shard].shared.control.push(Control::End { m: l.m });
            }
            !done
        });
        // New matches.
        loop {
            let full = inner.queue.len() >= SEATS;
            let waited = inner.since.is_some_and(|t| t.elapsed() >= self.fill_after);
            if inner.queue.is_empty() || !(full || waited) {
                break;
            }
            let Some((shard, m)) = self.free_match(&inner) else {
                break;
            };
            let n = inner.queue.len().min(SEATS);
            let players: Vec<Waiting> = inner.queue.drain(..n).collect();
            inner.since = (!inner.queue.is_empty()).then(Instant::now);
            inner.next =
                inner.next.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let seed = inner.next;
            if let Some(live) = self.start(shard, m, seed, players) {
                inner.live.push(live);
                inner.started += 1;
            }
        }
    }

    /// A place for a match: free on its shard, and not still being cleared.
    fn free_match(&self, inner: &Inner) -> Option<(usize, u16)> {
        let mut best = None;
        for (k, s) in self.game.shards.iter().enumerate() {
            let busy = inner.live.iter().filter(|l| l.shard == k).count();
            let free = (0..self.game.matches_per_shard).find(|&m| {
                s.shared.state(m) == MatchState::Free
                    && !inner.live.iter().any(|l| l.shard == k && usize::from(l.m) == m)
            });
            if let Some(m) = free
                && best.is_none_or(|(_, _, b)| busy < b)
            {
                best = Some((k, m as u16, busy));
            }
        }
        best.map(|(k, m, _)| (k, m))
    }

    /// Starts match `m` of `shard` with these players seated; `None` if every one of them had gone.
    fn start(&self, shard: usize, m: u16, seed: u64, players: Vec<Waiting>) -> Option<Live> {
        let shared = self.game.shards[shard].shared.clone();
        let picks = deal(seed);
        let humans = players.iter().enumerate().fold(0u16, |mask, (k, _)| mask | 1 << SEAT_ORDER[k]);
        let mut roster: Vec<Player> =
            (0..SEATS).map(|s| Player { name: format!("Bot {}", s + 1), agent: false, bot: true }).collect();
        for (k, p) in players.iter().enumerate() {
            roster[usize::from(SEAT_ORDER[k])] = Player { name: p.name.clone(), agent: p.agent, bot: false };
        }
        let _ = shared.control.push(Control::Start { m, seed, picks, humans });
        let mut sessions = Vec::new();
        for (k, p) in players.into_iter().enumerate() {
            let seat = SEAT_ORDER[k];
            let g = u32::from(m) * SEATS as u32 + u32::from(seat);
            let Some(lease) = self.game.take_lease(shard, g) else {
                tracing::error!(shard, m, seat, "a seat's lease is held elsewhere");
                continue;
            };
            let _ = shared.control.push(Control::Seat { m, seat, max_datagram: p.max_datagram });
            let msg = ToClient::Match {
                seat,
                seed: seed.to_string(),
                picks: picks.to_vec(),
                roster: roster.clone(),
            };
            let seating = Seating { shard, m, seat, g, msg, lease, shared: shared.clone() };
            if let Err(e) = p.tx.send(LobbyEvent::Seated(Box::new(seating))) {
                // The session went as its match was made: its seat goes back to the bots.
                if let LobbyEvent::Seated(s) = e.0 {
                    let _ = shared.control.push(Control::Unseat { m, seat });
                    self.game.give_lease(shard, s.lease);
                }
                continue;
            }
            sessions.push((p.session, seat, p.tx));
        }
        if sessions.is_empty() {
            let _ = shared.control.push(Control::End { m });
            return None;
        }
        tracing::info!(shard, m, seed, humans = sessions.len(), "match started");
        Some(Live { shard, m, sessions, over_at: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_team_draws_five_different_champions() {
        for seed in 0..200 {
            let p = deal(seed);
            for team in p.chunks(SEATS / 2) {
                let mut t = team.to_vec();
                t.sort_unstable();
                t.dedup();
                assert_eq!(t.len(), team.len().min(CHAMPIONS.len()), "seed {seed}: {p:?}");
            }
            assert!(p.iter().all(|&c| usize::from(c) < CHAMPIONS.len()));
        }
        assert_ne!(deal(1), deal(2));
    }
}
