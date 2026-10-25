//! The lobby: who's waiting, and the matches they're in. Ten in the queue make a match at once;
//! fewer make one when the first of them has waited `fill_after`, the server's bots taking the
//! seats no one did. Champions are dealt at random, all ten different (ARAM: All Random), and a
//! champion select follows ([`Select`]): a reroll sends yours to your team's bench and deals you
//! another, and anyone on the team may take a champion off its bench. The match starts when the
//! select's time is up or everyone seated is ready, with the picks it ended on.
//!
//! It runs on tokio and may lock: it is nowhere near a shard thread, which hears of a match only
//! through its control queue, and only once its select is over.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_client_core::lobby::{Player, ToClient, UNSEEN};
use el_match::{Control, MatchState, Outcome, SeatLease, ShardShared};
use el_sim::content::champions::CHAMPIONS;
use el_sim::math::Rng;
use el_sim::units::{SEATS, team_of_seat};
use tokio::sync::mpsc::UnboundedSender;

use crate::game::Game;

/// How long a finished match is kept (its last snapshots say who won) before its place is freed.
const AFTER: Duration = Duration::from_secs(5);
/// Seats in the order people fill them: a side each in turn.
const SEAT_ORDER: [u8; SEATS] = [0, 5, 1, 6, 2, 7, 3, 8, 4, 9];
/// How long champion select lasts, unless everyone seated is ready sooner.
pub const SELECT_FOR: Duration = Duration::from_secs(20);
/// Rerolls a seat has in a select (League banks them across games; nothing persists here yet).
pub const REROLLS: u8 = 2;

/// What the lobby tells a session.
pub enum LobbyEvent {
    Queued {
        waiting: usize,
        fill_in: u32,
    },
    /// Champion select, as this session's team sees it.
    Select(ToClient),
    Seated(Box<Seating>),
    Over {
        winner: u8,
    },
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

/// Each seat's champion for `seed`: ten different ones across both teams, dealt from the whole
/// pool (ARAM: All Random); if the pool is ever smaller than the seats, it is dealt again.
pub fn deal(seed: u64) -> [u8; SEATS] {
    let mut picks = [0u8; SEATS];
    let mut rng = Rng::new(seed ^ 0xA5A5);
    let mut pool: Vec<u8> = Vec::new();
    for p in picks.iter_mut() {
        if pool.is_empty() {
            pool = (0..CHAMPIONS.len() as u8).collect();
        }
        let i = rng.below(pool.len() as u32) as usize;
        *p = pool.swap_remove(i);
    }
    picks
}

/// ARAM's champion select for one match: each seat's champion, each team's bench, the champions no
/// one holds, and each seat's rerolls left and whether it's ready. No champion is in two places.
#[derive(Clone, Debug)]
pub struct Select {
    pub picks: [u8; SEATS],
    pub bench: [Vec<u8>; 2],
    /// Neither held nor on a bench: what a reroll deals from.
    unused: Vec<u8>,
    pub rerolls: [u8; SEATS],
    pub ready: [bool; SEATS],
    rng: Rng,
}

impl Select {
    pub fn new(seed: u64) -> Select {
        let picks = deal(seed);
        let unused = (0..CHAMPIONS.len() as u8).filter(|c| !picks.contains(c)).collect();
        Select {
            picks,
            bench: [Vec::new(), Vec::new()],
            unused,
            rerolls: [REROLLS; SEATS],
            ready: [false; SEATS],
            rng: Rng::new(seed ^ 0x5E1E),
        }
    }

    /// `seat`'s champion onto its team's bench, and a new one dealt it. False if it has no
    /// rerolls left, is ready, or there's nothing left to deal.
    pub fn reroll(&mut self, seat: usize) -> bool {
        if self.rerolls[seat] == 0 || self.ready[seat] || self.unused.is_empty() {
            return false;
        }
        let i = self.rng.below(self.unused.len() as u32) as usize;
        let new = self.unused.swap_remove(i);
        let team = usize::from(team_of_seat(seat));
        self.bench[team].push(self.picks[seat]);
        self.picks[seat] = new;
        self.rerolls[seat] -= 1;
        true
    }

    /// `champ` off `seat`'s team's bench for `seat`, its own onto the bench in its place. False if
    /// it isn't there or `seat` is ready.
    pub fn swap(&mut self, seat: usize, champ: u8) -> bool {
        let team = usize::from(team_of_seat(seat));
        let Some(i) = self.bench[team].iter().position(|&c| c == champ) else { return false };
        if self.ready[seat] {
            return false;
        }
        self.bench[team][i] = self.picks[seat];
        self.picks[seat] = champ;
        true
    }

    /// What `seat` is sent: every seat's champion as its team sees them (the other team's
    /// unseen), its team's bench, its rerolls.
    pub fn view(&self, seat: usize, ends_in: u32, roster: &[Player]) -> ToClient {
        let team = team_of_seat(seat);
        ToClient::Select {
            seat: seat as u8,
            picks: (0..SEATS).map(|s| if team_of_seat(s) == team { self.picks[s] } else { UNSEEN }).collect(),
            bench: self.bench[usize::from(team)].clone(),
            rerolls: self.rerolls[seat],
            ends_in,
            roster: roster.to_vec(),
        }
    }
}

/// A match made, in champion select: its place, its select, and who sits where.
struct Selecting {
    shard: usize,
    m: u16,
    seed: u64,
    select: Select,
    /// The sessions in it, by seat.
    players: Vec<(u8, Waiting)>,
    roster: Vec<Player>,
    ends: Instant,
}

impl Selecting {
    fn seat_of(&self, session: u32) -> Option<usize> {
        self.players.iter().find(|(_, w)| w.session == session).map(|(s, _)| usize::from(*s))
    }

    /// Everyone seated is ready.
    fn all_ready(&self) -> bool {
        self.players.iter().all(|(s, _)| self.select.ready[usize::from(*s)])
    }

    /// The select as each of `team`'s seated sessions sees it (both teams, if `None`).
    fn announce(&self, team: Option<u8>) {
        let ends_in = self.ends.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u32;
        for (seat, w) in &self.players {
            if team.is_none_or(|t| team_of_seat(usize::from(*seat)) == t) {
                let msg = self.select.view(usize::from(*seat), ends_in, &self.roster);
                let _ = w.tx.send(LobbyEvent::Select(msg));
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    queue: Vec<Waiting>,
    /// When the first in the queue joined it.
    since: Option<Instant>,
    selecting: Vec<Selecting>,
    live: Vec<Live>,
    next: u64,
    started: u64,
}

pub struct Lobby {
    pub game: Game,
    fill_after: Duration,
    select_for: Duration,
    inner: Mutex<Inner>,
}

/// The seats with nobody in them, as the scoreboard names them.
fn bots_roster() -> Vec<Player> {
    (0..SEATS).map(|s| Player { name: format!("Bot {}", s + 1), agent: false, bot: true }).collect()
}

impl Lobby {
    pub fn new(game: Game, fill_after: Duration, select_for: Duration) -> Arc<Lobby> {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Arc::new(Lobby {
            game,
            fill_after,
            select_for,
            inner: Mutex::new(Inner { next: seed, ..Inner::default() }),
        })
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts a session in the queue (again, if it was; out of a select it was in).
    pub fn enqueue(
        &self,
        session: u32,
        name: &str,
        agent: bool,
        max_datagram: u16,
        tx: UnboundedSender<LobbyEvent>,
    ) {
        let mut inner = self.inner();
        Self::out_of_select(&mut inner, session);
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

    /// Takes a session out of the queue, or out of its champion select (a bot takes its seat).
    pub fn leave(&self, session: u32) {
        let mut inner = self.inner();
        inner.queue.retain(|w| w.session != session);
        if inner.queue.is_empty() {
            inner.since = None;
        }
        Self::out_of_select(&mut inner, session);
    }

    /// A session leaves the select it's in: its seat goes to a bot, and a select with nobody left
    /// in it is over (nothing was started: its place is simply free again).
    fn out_of_select(inner: &mut Inner, session: u32) {
        let Some(k) = inner.selecting.iter().position(|s| s.seat_of(session).is_some()) else { return };
        let s = &mut inner.selecting[k];
        if let Some(seat) = s.seat_of(session) {
            s.players.retain(|(st, _)| usize::from(*st) != seat);
            s.roster[seat] = bots_roster().swap_remove(seat);
            s.select.ready[seat] = true;
        }
        if s.players.is_empty() {
            inner.selecting.swap_remove(k);
        } else {
            s.announce(None);
        }
    }

    /// In champion select: `session`'s champion rerolled.
    pub fn reroll(&self, session: u32) {
        self.choose(session, |sel, seat| sel.reroll(seat));
    }

    /// In champion select: `champ` off the team's bench for `session`.
    pub fn swap(&self, session: u32, champ: u8) {
        self.choose(session, |sel, seat| sel.swap(seat, champ));
    }

    /// In champion select: `session` is done choosing; the match starts once everyone seated is.
    pub fn ready(&self, session: u32) {
        let mut inner = self.inner();
        if let Some(s) = inner.selecting.iter_mut().find(|s| s.seat_of(session).is_some())
            && let Some(seat) = s.seat_of(session)
        {
            s.select.ready[seat] = true;
        }
        self.start_due(&mut inner);
    }

    /// A change to `session`'s select; its team hears of it if it was made.
    fn choose(&self, session: u32, change: impl FnOnce(&mut Select, usize) -> bool) {
        let mut inner = self.inner();
        let Some(s) = inner.selecting.iter_mut().find(|s| s.seat_of(session).is_some()) else { return };
        let Some(seat) = s.seat_of(session) else { return };
        if change(&mut s.select, seat) {
            s.announce(Some(team_of_seat(seat)));
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

    /// Matches running (or in champion select), sessions waiting, and matches started.
    pub fn counts(&self) -> (usize, usize, u64) {
        let inner = self.inner();
        (inner.live.len() + inner.selecting.len(), inner.queue.len(), inner.started)
    }

    /// The lobby's loop: matches made, selects ended, results passed on, finished matches cleared.
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
        // New matches, into champion select.
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
            let waiting: Vec<Waiting> = inner.queue.drain(..n).collect();
            inner.since = (!inner.queue.is_empty()).then(Instant::now);
            inner.next =
                inner.next.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let seed = inner.next;
            let mut roster = bots_roster();
            let players: Vec<(u8, Waiting)> = waiting
                .into_iter()
                .enumerate()
                .map(|(k, w)| {
                    let seat = SEAT_ORDER[k];
                    roster[usize::from(seat)] = Player { name: w.name.clone(), agent: w.agent, bot: false };
                    (seat, w)
                })
                .collect();
            let s = Selecting {
                shard,
                m,
                seed,
                select: Select::new(seed),
                players,
                roster,
                ends: Instant::now() + self.select_for,
            };
            if !self.select_for.is_zero() {
                s.announce(None);
            }
            inner.selecting.push(s);
        }
        // Selects that are over (the page counts each down from its `ends_in`).
        self.start_due(&mut inner);
    }

    /// Starts every match whose select is over: its time up, or everyone in it ready.
    fn start_due(&self, inner: &mut Inner) {
        let now = Instant::now();
        let mut k = 0;
        while k < inner.selecting.len() {
            let s = &inner.selecting[k];
            if now < s.ends && !s.all_ready() {
                k += 1;
                continue;
            }
            let s = inner.selecting.swap_remove(k);
            if let Some(live) = self.start(s) {
                inner.live.push(live);
                inner.started += 1;
            }
        }
    }

    /// A place for a match: free on its shard, and neither in a select nor still being cleared.
    fn free_match(&self, inner: &Inner) -> Option<(usize, u16)> {
        let mut best = None;
        for (k, s) in self.game.shards.iter().enumerate() {
            let busy = inner.live.iter().filter(|l| l.shard == k).count()
                + inner.selecting.iter().filter(|s| s.shard == k).count();
            let free = (0..self.game.matches_per_shard).find(|&m| {
                s.shared.state(m) == MatchState::Free
                    && !inner.live.iter().any(|l| l.shard == k && usize::from(l.m) == m)
                    && !inner.selecting.iter().any(|s| s.shard == k && usize::from(s.m) == m)
            });
            if let Some(m) = free
                && best.is_none_or(|(_, _, b)| busy < b)
            {
                best = Some((k, m as u16, busy));
            }
        }
        best.map(|(k, m, _)| (k, m))
    }

    /// Starts the match its select ended on, its players seated; `None` if every one of them had
    /// gone.
    fn start(&self, s: Selecting) -> Option<Live> {
        let Selecting { shard, m, seed, select, players, roster, .. } = s;
        let picks = select.picks;
        let shared = self.game.shards[shard].shared.clone();
        let humans = players.iter().fold(0u16, |mask, (seat, _)| mask | 1 << seat);
        let _ = shared.control.push(Control::Start { m, seed, picks, humans });
        let mut sessions = Vec::new();
        for (seat, p) in players {
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
    fn a_match_draws_ten_different_champions() {
        for seed in 0..200 {
            let p = deal(seed);
            let mut all = p.to_vec();
            all.sort_unstable();
            all.dedup();
            assert_eq!(all.len(), SEATS.min(CHAMPIONS.len()), "seed {seed}: {p:?}");
            assert!(p.iter().all(|&c| usize::from(c) < CHAMPIONS.len()));
        }
        assert_ne!(deal(1), deal(2));
    }

    /// Every champion is in exactly one place: a seat, a bench, or unused.
    fn whole(s: &Select) {
        let mut all: Vec<u8> =
            s.picks.iter().chain(&s.bench[0]).chain(&s.bench[1]).chain(&s.unused).copied().collect();
        all.sort_unstable();
        assert_eq!(all, (0..CHAMPIONS.len() as u8).collect::<Vec<_>>(), "{s:?}");
    }

    #[test]
    fn a_reroll_benches_the_champion_and_deals_an_unused_one() {
        let mut s = Select::new(7);
        let was = s.picks[2];
        assert!(s.reroll(2));
        assert_ne!(s.picks[2], was);
        assert_eq!(s.bench[0], vec![was], "on its own team's bench");
        assert!(s.bench[1].is_empty());
        whole(&s);
        assert!(s.reroll(2));
        assert!(!s.reroll(2), "two a seat");
        assert_eq!(s.rerolls[2], 0);
        whole(&s);
    }

    #[test]
    fn a_teammate_takes_a_champion_off_the_bench_and_leaves_their_own() {
        let mut s = Select::new(9);
        let benched = s.picks[0];
        s.reroll(0);
        let mine = s.picks[1];
        assert!(!s.swap(5, benched), "the other team's bench is theirs");
        assert!(s.swap(1, benched));
        assert_eq!(s.picks[1], benched);
        assert_eq!(s.bench[0], vec![mine]);
        assert!(!s.swap(1, 99), "only what's on the bench");
        s.ready[1] = true;
        assert!(!s.swap(1, mine) && !s.reroll(1), "ready is ready");
        whole(&s);
    }

    #[test]
    fn a_team_sees_its_own_champions_only() {
        let s = Select::new(3);
        let ToClient::Select { picks, rerolls, .. } = s.view(6, 20, &bots_roster()) else { panic!() };
        assert_eq!(rerolls, REROLLS);
        for (seat, &c) in picks.iter().enumerate() {
            if team_of_seat(seat) == 1 {
                assert_eq!(c, s.picks[seat]);
            } else {
                assert_eq!(c, UNSEEN, "seat {seat} is the other team's");
            }
        }
    }

    #[test]
    fn rerolling_everything_never_holds_a_champion_twice() {
        let mut s = Select::new(1);
        for seat in 0..SEATS {
            while s.reroll(seat) {}
        }
        whole(&s);
        let mut held = s.picks.to_vec();
        held.sort_unstable();
        held.dedup();
        assert_eq!(held.len(), SEATS);
    }
}
