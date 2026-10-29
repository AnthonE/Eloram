//! The lobby: who's waiting, and the matches they're in. Ten in the queue make a match at once;
//! fewer make one when the first of them has waited `fill_after`, the server's bots taking the
//! seats no one did. Champions are dealt at random, all ten different (ARAM: All Random), and a
//! champion select follows ([`Select`]): a reroll sends yours to your team's bench and deals you
//! another, and anyone on the team may take a champion off its bench. The match starts when the
//! select's time is up or everyone seated is ready, with the picks it ended on.
//!
//! A seated player is given a rejoin token with their match: a connection that drops (or a page
//! reloaded) takes the seat back with it while the match lasts ([`Lobby::rejoin`]), the server's
//! bot playing it meanwhile. A match whose players have all gone is kept [`REJOIN_FOR`] for them
//! to come back to before it ends.
//!
//! A token is kept only as its keccak-256, here and in the match's lobby file (`{id}.live`, with a
//! records directory), so a restart can give players their seats back (`crate::recover`) and
//! nothing on disk can be used to take one.
//!
//! It runs on tokio and may lock: it is nowhere near a shard thread, which hears of a match only
//! through its control queue, and only once its select is over.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_client_core::lobby::{Player, ToClient, UNSEEN};
use el_match::{Control, MatchState, Outcome, SeatLease, ShardShared};
use el_sim::content::champions::CHAMPIONS;
use el_sim::math::Rng;
use el_sim::units::{SEATS, team_of_seat};
use tokio::sync::mpsc::UnboundedSender;

use crate::game::Game;
use crate::recover::{LiveFile, Recovered};

/// The winner a match that couldn't go on is given: neither team (the page says no contest).
pub const NO_CONTEST: u8 = 2;
/// A shard whose tick hasn't moved for this long, with matches in it, is taken as gone: its
/// matches are ended and no more are put on it.
const STALLED: Duration = Duration::from_secs(3);

/// How long a finished match is kept (its last snapshots say who won) before its place is freed.
const AFTER: Duration = Duration::from_secs(5);
/// How long a match with none of its players connected is kept for them to rejoin.
pub const REJOIN_FOR: Duration = Duration::from_secs(60);
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
    /// The seat was taken back on another connection (its rejoin token): this one is done.
    Replaced,
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
    /// The rejoin token it came with (rejoining; empty in the queue).
    token: String,
}

struct Live {
    shard: usize,
    m: u16,
    /// The sessions seated, by seat.
    sessions: Vec<(u32, u8, UnboundedSender<LobbyEvent>)>,
    over_at: Option<Instant>,
    /// Since when no one has been seated in it (it ends [`REJOIN_FOR`] after).
    empty_since: Option<Instant>,
    /// What a player is told of the match, their seat and token aside.
    told: ToClient,
    /// Each seat's rejoin token's keccak-256 (zeros: none, a bot's seat from the start).
    tokens: [[u8; 32]; SEATS],
    /// Rejoins waiting for their seat's old session to hand its lease back.
    pending: Vec<(u8, Waiting)>,
}

/// What a rejoin token is kept as.
fn token_hash(token: &str) -> [u8; 32] {
    el_auth::keccak256(token.as_bytes())
}

impl Live {
    /// What `seat` is told of the match: everything, and its rejoin token.
    fn match_msg(&self, seat: u8, token: &str) -> ToClient {
        let mut msg = self.told.clone();
        if let ToClient::Match { seat: s, rejoin, .. } = &mut msg {
            *s = seat;
            *rejoin = token.to_string();
        }
        msg
    }
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
    /// A practice match (`el_sim::practice`).
    practice: bool,
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
    /// Each shard's tick when it last moved, and when that was.
    beats: Vec<(u64, Instant)>,
    /// Shards taken as gone.
    dead: Vec<bool>,
}

pub struct Lobby {
    pub game: Game,
    fill_after: Duration,
    select_for: Duration,
    /// Where each live match's lobby file goes (`{id}.live`), if anywhere.
    dir: Option<PathBuf>,
    /// Practice matches may be asked for.
    practice: std::sync::atomic::AtomicBool,
    inner: Mutex<Inner>,
}

/// A practice match's seats: the player's, and nine dummies.
fn practice_roster(player: Player) -> Vec<Player> {
    std::iter::once(player)
        .chain((1..SEATS).map(|s| Player { name: format!("Dummy {s}"), agent: false, bot: true }))
        .collect()
}

/// The seats with nobody in them, as the scoreboard names them.
fn bots_roster() -> Vec<Player> {
    (0..SEATS).map(|s| Player { name: format!("Bot {}", s + 1), agent: false, bot: true }).collect()
}

impl Lobby {
    pub fn new(game: Game, fill_after: Duration, select_for: Duration) -> Arc<Lobby> {
        Self::with_state(game, fill_after, select_for, None, Vec::new())
    }

    /// A lobby that keeps each live match's lobby file in `dir`, holding the matches a restart
    /// took up (`recovered`) for their players to rejoin.
    pub fn with_state(
        game: Game,
        fill_after: Duration,
        select_for: Duration,
        dir: Option<PathBuf>,
        recovered: Vec<Recovered>,
    ) -> Arc<Lobby> {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let now = Instant::now();
        let live = recovered
            .into_iter()
            .map(|r| Live {
                shard: r.shard,
                m: r.m,
                sessions: Vec::new(),
                over_at: None,
                empty_since: Some(now),
                tokens: r.live.token_hashes(),
                told: r.live.told,
                pending: Vec::new(),
            })
            .collect();
        let shards = game.shards.len();
        Arc::new(Lobby {
            game,
            fill_after,
            select_for,
            dir,
            practice: std::sync::atomic::AtomicBool::new(false),
            inner: Mutex::new(Inner {
                next: seed,
                live,
                beats: vec![(0, now); shards],
                dead: vec![false; shards],
                ..Inner::default()
            }),
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
        inner.queue.push(Waiting {
            session,
            name: name.to_string(),
            agent,
            max_datagram,
            tx,
            token: String::new(),
        });
        if inner.since.is_none() {
            inner.since = Some(Instant::now());
        }
        let waiting = inner.queue.len();
        let left = self.fill_after.saturating_sub(inner.since.map_or(Duration::ZERO, |s| s.elapsed()));
        for w in &inner.queue {
            let _ = w.tx.send(LobbyEvent::Queued { waiting, fill_in: left.as_secs_f32().ceil() as u32 });
        }
    }

    /// Lets sessions ask for practice matches (`el-server --practice`).
    pub fn allow_practice(&self) {
        self.practice.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Starts a practice match for `session` as champion `champ` (`el_sim::practice`): seat 0 its,
    /// the other nine dummies, no champion select. False if practice isn't allowed here or no
    /// match is free.
    #[allow(clippy::too_many_arguments)]
    pub fn practice(
        &self,
        session: u32,
        name: &str,
        agent: bool,
        max_datagram: u16,
        tx: UnboundedSender<LobbyEvent>,
        champ: u8,
    ) -> bool {
        if !self.practice.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        let mut inner = self.inner();
        Self::out_of_select(&mut inner, session);
        inner.queue.retain(|w| w.session != session);
        let Some((shard, m)) = self.free_match(&inner) else { return false };
        inner.next =
            inner.next.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let seed = inner.next;
        // The champion asked for in seat 0, the dummies dealt the rest (none twice).
        let mut select = Select::new(seed);
        let champ = champ % el_sim::content::champions::CHAMPIONS.len() as u8;
        match select.picks.iter().position(|&c| c == champ) {
            Some(k) => select.picks.swap(0, k),
            None => select.picks[0] = champ,
        }
        let w = Waiting { session, name: name.to_string(), agent, max_datagram, tx, token: String::new() };
        let roster = practice_roster(Player { name: name.to_string(), agent, bot: false });
        let s = Selecting {
            shard,
            m,
            seed,
            select,
            players: vec![(0, w)],
            roster,
            ends: Instant::now(),
            practice: true,
        };
        match self.start(s) {
            Some(live) => {
                inner.live.push(live);
                inner.started += 1;
                true
            }
            None => false,
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
        let _ = shared.control.push(Control::Unseat { m, seat });
        self.game.detach(shard, lease.g);
        self.game.give_lease(shard, lease);
        if let Some(l) = inner.live.iter_mut().find(|l| l.shard == shard && l.m == m) {
            l.sessions.retain(|(_, st, _)| *st != seat);
            // Someone rejoining this seat was waiting for it.
            if let Some(i) = l.pending.iter().position(|(st, _)| *st == seat) {
                let (_, w) = l.pending.swap_remove(i);
                self.seat_in(l, seat, w);
            }
            if l.sessions.is_empty() && l.pending.is_empty() {
                // A finished match goes now, and a practice one; a running one waits for its
                // players a while.
                end = l.over_at.is_some() || matches!(l.told, ToClient::Match { practice: true, .. });
                l.empty_since.get_or_insert_with(Instant::now);
            }
        }
        if end {
            inner.live.retain(|l| !(l.shard == shard && l.m == m));
            let _ = shared.control.push(Control::End { m });
        }
    }

    /// Takes `session` back to the seat `token` was given for, in a match still running. The old
    /// session, if it still holds the seat, is told it's been replaced and the seat follows once
    /// it lets go. False if there's no such seat.
    #[allow(clippy::too_many_arguments)]
    pub fn rejoin(
        &self,
        session: u32,
        name: &str,
        agent: bool,
        max_datagram: u16,
        tx: UnboundedSender<LobbyEvent>,
        token: &str,
    ) -> bool {
        let Some(token) = u128::from_str_radix(token, 16).ok().filter(|&t| t != 0 && token.len() == 32)
        else {
            return false;
        };
        let token = format!("{token:032x}");
        let hash = token_hash(&token);
        let mut inner = self.inner();
        inner.queue.retain(|w| w.session != session);
        Self::out_of_select(&mut inner, session);
        let Some(l) = inner.live.iter_mut().find(|l| l.over_at.is_none() && l.tokens.contains(&hash)) else {
            return false;
        };
        let Some(seat) = l.tokens.iter().position(|t| *t == hash).map(|s| s as u8) else { return false };
        let w = Waiting { session, name: name.to_string(), agent, max_datagram, tx, token };
        l.pending.retain(|(st, _)| *st != seat);
        if let Some((_, _, old)) = l.sessions.iter().find(|(_, st, _)| *st == seat) {
            if old.send(LobbyEvent::Replaced).is_ok() {
                l.pending.push((seat, w));
                return true;
            }
            // Its session is gone already and its seat on its way back: take it now.
            l.sessions.retain(|(_, st, _)| *st != seat);
        }
        self.seat_in(l, seat, w)
    }

    /// Seats `w` in `seat` of `l`, its lease taken and the shard told. False if the lease is held.
    fn seat_in(&self, l: &mut Live, seat: u8, w: Waiting) -> bool {
        let (shard, m) = (l.shard, l.m);
        let g = u32::from(m) * SEATS as u32 + u32::from(seat);
        let Some(lease) = self.game.take_lease(shard, g) else { return false };
        let shared = self.game.shards[shard].shared.clone();
        let _ = shared.control.push(Control::Seat { m, seat, max_datagram: w.max_datagram });
        let seating =
            Seating { shard, m, seat, g, msg: l.match_msg(seat, &w.token), lease, shared: shared.clone() };
        if let Err(e) = w.tx.send(LobbyEvent::Seated(Box::new(seating))) {
            if let LobbyEvent::Seated(s) = e.0 {
                let _ = shared.control.push(Control::Unseat { m, seat });
                self.game.give_lease(shard, s.lease);
            }
            return false;
        }
        tracing::info!(shard, m, seat, session = w.session, "rejoined");
        l.sessions.push((w.session, seat, w.tx));
        l.empty_since = None;
        true
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
            while let Some(o) = s.shared.outcomes.pop() {
                let (m, winner) = match o {
                    Outcome::Over { m, winner, tick, hash } => {
                        tracing::info!(
                            shard = k,
                            m,
                            winner,
                            tick,
                            hash = format_args!("{hash:016x}"),
                            "match over"
                        );
                        (m, winner)
                    }
                    Outcome::Failed { m, tick } => {
                        tracing::error!(shard = k, m, tick, "a match panicked: stopped, no contest");
                        (m, NO_CONTEST)
                    }
                };
                Self::over(&mut inner, k, m, winner);
            }
        }
        // A shard whose thread has stopped ticking (it panicked outside a match): its matches are
        // over, and nothing more goes on it.
        let now = Instant::now();
        for (k, s) in self.game.shards.iter().enumerate() {
            let tick = s.shared.tick.load(std::sync::atomic::Ordering::Acquire);
            if tick != inner.beats[k].0 {
                inner.beats[k] = (tick, now);
                continue;
            }
            let busy = inner.live.iter().any(|l| l.shard == k && l.over_at.is_none());
            if busy && !inner.dead[k] && now.duration_since(inner.beats[k].1) >= STALLED {
                tracing::error!(shard = k, tick, "a shard stopped ticking: its matches end, no contest");
                inner.dead[k] = true;
                let ms: Vec<u16> = inner.live.iter().filter(|l| l.shard == k).map(|l| l.m).collect();
                for m in ms {
                    Self::over(&mut inner, k, m, NO_CONTEST);
                }
            }
        }
        let game = &self.game;
        inner.live.retain(|l| {
            let done = l.over_at.is_some_and(|t| t.elapsed() >= AFTER)
                || l.empty_since.is_some_and(|t| t.elapsed() >= REJOIN_FOR);
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
                practice: false,
            };
            if !self.select_for.is_zero() {
                s.announce(None);
            }
            inner.selecting.push(s);
        }
        // Selects that are over (the page counts each down from its `ends_in`).
        self.start_due(&mut inner);
    }

    /// Match `m` of shard `k` is over: its players told who won.
    fn over(inner: &mut Inner, k: usize, m: u16, winner: u8) {
        if let Some(l) = inner.live.iter_mut().find(|l| l.shard == k && l.m == m && l.over_at.is_none()) {
            l.over_at = Some(Instant::now());
            for (_, _, tx) in &l.sessions {
                let _ = tx.send(LobbyEvent::Over { winner });
            }
        }
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
            if inner.dead[k] {
                continue;
            }
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
        let Selecting { shard, m, seed, select, players, roster, practice, .. } = s;
        let picks = select.picks;
        let shared = self.game.shards[shard].shared.clone();
        let humans = players.iter().fold(0u16, |mask, (seat, _)| mask | 1 << seat);
        // The augment offers' secret: from the system's randomness, never in the record's header
        // or told to anyone (the record gets each screen's key as it opens).
        let secret = getrandom::u64().unwrap_or(seed.rotate_left(29) ^ 0x5EC2_E7C0_FFEE_D00D) | 1;
        let _ = shared.control.push(Control::Start { m, seed, picks, humans, secret, practice });
        let mut sessions = Vec::new();
        let mut tokens = [[0u8; 32]; SEATS];
        // What a player is told of the match: never the seed, from which every seat's augment
        // offers follow.
        let told = ToClient::Match {
            seat: 0,
            picks: picks.to_vec(),
            tiers: el_sim::game::tiers_for(seed),
            headliner: el_sim::game::headliner_for(seed),
            roster: roster.clone(),
            rejoin: String::new(),
            practice,
        };
        for (seat, p) in players {
            let g = u32::from(m) * SEATS as u32 + u32::from(seat);
            let Some(lease) = self.game.take_lease(shard, g) else {
                tracing::error!(shard, m, seat, "a seat's lease is held elsewhere");
                continue;
            };
            let _ = shared.control.push(Control::Seat { m, seat, max_datagram: p.max_datagram });
            let mut buf = [0u8; 16];
            let token = match getrandom::fill(&mut buf) {
                Ok(()) => format!("{:032x}", u128::from_le_bytes(buf).max(1)),
                Err(_) => String::new(),
            };
            if !token.is_empty() {
                tokens[usize::from(seat)] = token_hash(&token);
            }
            let mut msg = told.clone();
            if let ToClient::Match { seat: s, rejoin, .. } = &mut msg {
                *s = seat;
                *rejoin = token;
            }
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
        if let Some(dir) = &self.dir {
            let hex = |t: &[u8; 32]| if *t == [0; 32] { String::new() } else { hex::encode(t) };
            let file = LiveFile { told: told.clone(), tokens: tokens.iter().map(hex).collect() };
            file.write(dir, &format!("{seed:016x}"));
        }
        Some(Live { shard, m, sessions, over_at: None, empty_since: None, told, tokens, pending: Vec::new() })
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
