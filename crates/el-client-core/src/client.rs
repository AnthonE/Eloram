//! A seat's client, without its transport: what the browser and the agents share. It takes the
//! server's snapshot datagrams into a model of the match as the team sees it ([`Client::world`]),
//! predicts its own champion from there with the server's own step ([`Client::pred`]), and turns
//! what the player holds and decides into input packets (`docs/NETCODE.md`, "Your champion is
//! predicted", "Everyone else is interpolated").
//!
//! Nothing here reads a clock: every call is given the page's time in seconds, so a test can run
//! a lossy link in virtual time.

use el_proto::ack::{AckWindow, Verdict};
use el_proto::event::{self, MAX_LIST, WireEvent};
use el_proto::input::MAX_CMDS;
use el_proto::quant::pos_from_code;
use el_proto::snapshot::{SnapshotHeader, snap_flags};
use el_proto::unit::{Delta, QUnit};
use el_proto::{BitReader, DecodeError, InputCmd, InputPacket, Order};
use el_sim::content::rules;
use el_sim::game::{Intent, MAX_ORDERS_TICK, NO_SLOT, SeatInput};
use el_sim::map::Map;
use el_sim::own::{OWN_BASELINES, OwnState, OwnWidths, own_widths};
use el_sim::path::PathScratch;
use el_sim::units::{FIRST_MINION, MAX_UNITS, SEATS, team_of_seat};
use el_sim::{Match, ticks};
use glam::Vec2;

use crate::baselines::Baselines;
use crate::clock::Clock;
use crate::inputs::InputHistory;
use crate::interp::{Sample, Track};
use crate::missiles::Missiles;
use crate::orders::Orders;
use crate::zones::Zones;

/// Events kept for the page to read: the kill feed, damage numbers, casts to animate.
pub const FEED: usize = 256;
/// The own champion's predicted casts and blinks kept for the page (`docs/FX.md`: its effects
/// start from the prediction, a round trip before the server's say).
pub const OWN_FEED: usize = 16;

/// Something the own champion did in the prediction, the first time it was stepped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum OwnEvent {
    #[default]
    Nothing,
    /// It began a cast: the slot, and where it aimed.
    Cast { slot: u8, at: Vec2 },
    /// It was somewhere else at once.
    Blink { from: Vec2, to: Vec2 },
    /// A missile of its left (an ability's, or a ranged auto: its source's code).
    Missile { source: u32, from: Vec2 },
    /// It swung at something in melee.
    Swing,
}
/// The most commands one poll makes; a client further behind than this (a stalled page) jumps.
const CATCH_UP: u32 = 60;
/// The most packets one poll sends: enough to carry every command it made.
pub const MAX_PACKETS: usize = (CATCH_UP as usize).div_ceil(MAX_CMDS) + 1;

/// What one poll sends: one packet, or on a slow frame as many as carry every command it made
/// (the server stands in for a missing command only a few ticks, so a command never sent is a
/// step the prediction took and the server didn't).
#[derive(Clone, Copy, Debug)]
pub struct Packets {
    list: [InputPacket; MAX_PACKETS],
    n: usize,
}

impl Packets {
    pub fn iter(&self) -> impl Iterator<Item = &InputPacket> {
        self.list[..self.n].iter()
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}
/// The own champion's predicted positions kept for drawing, by tick.
const OWN_KEPT: usize = 16;
/// How fast a correction of the own champion is drawn away (s).
const SMOOTH: f64 = 0.1;
/// A correction this far (units) is drawn as a jump.
const SMOOTH_SNAP: f32 = 250.0;

/// What the own champion was doing when the prediction was put right: what the error is laid at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cause {
    /// Walking, standing or holding, of its own accord: the prediction should be exact.
    #[default]
    Walking,
    /// Chasing, attacking or casting: it moves on where others are, which the client sees late.
    Fighting,
    Dashing,
    /// Stunned, rooted, knocked up or slowed: what others did to it, seen a round trip late.
    Crowd,
    Dead,
    /// Back at the fountain.
    Respawned,
}

/// How far the prediction was off when a snapshot put it right.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Correction {
    /// The snapshot's tick.
    pub tick: u32,
    /// How far (units) the champion's predicted position moved.
    pub error: f32,
    pub cause: Cause,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub datagrams: u64,
    pub bytes: u64,
    pub stale: u64,
    pub errors: u64,
    pub no_baseline: u64,
    pub records: u64,
    pub events: u64,
    pub reseeds: u64,
    pub packets: u64,
    /// The newest snapshot's report: commands the server stood in for (of the last 30), and the
    /// commands it held.
    pub missed: u8,
    pub depth: u8,
}

/// What became of a datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// Older than one already taken: dropped, and not acked.
    Stale,
    /// Taken: the snapshot's tick, and whether it carried the own champion.
    Taken { tick: u32, own: bool },
}

pub struct Client {
    pub seat: usize,
    pub team: u8,
    /// The match as the server last said, as far as the team may know it.
    pub world: Match,
    /// The world with the own champion stepped on to the input clock.
    pub pred: Match,
    pub clock: Clock,
    acks: AckWindow,
    baselines: Baselines,
    /// The own champion's states taken of late, by tick: the server's baselines for the next.
    own_kept: Box<[(u32, OwnState)]>,
    own_widths: OwnWidths,
    /// The own state the datagram being read carried.
    own_new: Option<OwnState>,
    /// Everyone else's records for drawing, by slot.
    pub tracks: Box<[Track]>,
    pub missiles: Missiles,
    pub zones: Zones,
    inputs: InputHistory,
    pub orders: Orders,
    /// The next order the prediction applies.
    replay_seq: u16,
    /// The next reliable event expected.
    next_event: u16,
    /// The newest [`FEED`] events, with their snapshots' ticks: event `n` sits at `n % FEED`.
    pub feed: Box<[(u32, WireEvent)]>,
    pub feed_n: u64,
    /// The newest [`OWN_FEED`] of the own champion's predicted [`OwnEvent`]s, by tick, as the feed.
    pub own_feed: [(u32, OwnEvent); OWN_FEED],
    pub own_feed_n: u64,
    scratch: PathScratch,
    /// The next tick to make a command for; the prediction is at this tick.
    next_cmd: u32,
    /// The first order no packet has carried yet.
    unsent: u16,
    /// The own champion's state has come at least once.
    started: bool,
    was_dead: bool,
    own_ticks: [(u32, Vec2); OWN_KEPT],
    smooth: Vec2,
    smooth_at: f64,
    /// The newest correction.
    pub correction: Option<Correction>,
    /// The slots the newest datagram taken had records of.
    pub last_records: [u8; MAX_UNITS],
    pub last_n: usize,
    pub stats: Stats,
    /// The match is over (the snapshot said so).
    pub over: bool,
}

fn ms(now: f64) -> u16 {
    ((now * 1_000.0).max(0.0) as u64 & 0xFFFF) as u16
}

impl Client {
    /// A client for `seat` of the match the lobby told of: its `picks`, its augment screens'
    /// `tiers` and its `headliner` (never its seed, which would give away every seat's offers).
    pub fn new(map: &'static Map, seat: usize, picks: [u8; SEATS], tiers: [u8; 4], headliner: u8) -> Self {
        let mut world = Match::told(map, picks, tiers, headliner);
        // What the team knows at the start: its structures and the enemy's (always seen), and
        // nothing that moves until the server says where.
        for i in (0..SEATS).chain(FIRST_MINION..MAX_UNITS) {
            world.forget(i);
        }
        let team = team_of_seat(seat);
        for i in SEATS..FIRST_MINION {
            if world.units[i].alive {
                world.vision.units[usize::from(team)].set(i, true);
            }
        }
        let pred = Match::told(map, picks, tiers, headliner);
        Self {
            seat,
            team,
            world,
            pred,
            clock: Clock::default(),
            acks: AckWindow::default(),
            baselines: Baselines::default(),
            own_kept: vec![(u32::MAX, OwnState::default()); OWN_BASELINES].into_boxed_slice(),
            own_widths: own_widths(),
            own_new: None,
            tracks: vec![Track::default(); MAX_UNITS].into_boxed_slice(),
            missiles: Missiles::default(),
            zones: Zones::default(),
            inputs: InputHistory::default(),
            orders: Orders::default(),
            replay_seq: 0,
            next_event: 0,
            feed: vec![(0, WireEvent::Nothing); FEED].into_boxed_slice(),
            feed_n: 0,
            own_feed: [(0, OwnEvent::Nothing); OWN_FEED],
            own_feed_n: 0,
            scratch: PathScratch::new(),
            next_cmd: 0,
            unsent: 0,
            started: false,
            was_dead: false,
            own_ticks: [(u32::MAX, Vec2::ZERO); OWN_KEPT],
            smooth: Vec2::ZERO,
            smooth_at: 0.0,
            correction: None,
            last_records: [0; MAX_UNITS],
            last_n: 0,
            stats: Stats::default(),
            over: false,
        }
    }

    /// Whether the own champion's state has come (before it, there's nothing to predict or send).
    pub fn started(&self) -> bool {
        self.started
    }

    /// The tick the next command is for (the prediction's tick).
    pub fn next_cmd(&self) -> u32 {
        self.next_cmd
    }

    /// Gives an order, for the next command's tick: it shows in the prediction at once.
    pub fn order(&mut self, o: Order) -> Option<u16> {
        self.orders.give(o, self.next_cmd)
    }

    // -----------------------------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------------------------

    /// Takes a snapshot datagram that arrived at `now` (s).
    pub fn on_datagram(&mut self, bytes: &[u8], now: f64) -> Result<Taken, DecodeError> {
        self.stats.datagrams += 1;
        self.stats.bytes += bytes.len() as u64;
        let mut r = BitReader::new(bytes);
        let h = match SnapshotHeader::read(&mut r) {
            Ok(h) => h,
            Err(e) => {
                self.stats.errors += 1;
                return Err(e);
            }
        };
        let verdict = self.acks.check(h.stream, h.seq, h.tick);
        let new_stream = match verdict {
            Verdict::Stale => {
                self.stats.stale += 1;
                return Ok(Taken::Stale);
            }
            Verdict::Unsequenced => {
                self.stats.errors += 1;
                return Err(DecodeError::Invalid);
            }
            Verdict::NewStream(_) => true,
            Verdict::Next(_) => false,
        };
        if new_stream {
            self.start_stream();
        }
        let own = h.flags & snap_flags::OWN != 0;
        let mut rel = [WireEvent::Nothing; MAX_LIST];
        let mut once = [WireEvent::Nothing; MAX_LIST];
        let mut recs = [QUnit::default(); MAX_UNITS];
        let decoded = self.decode(&h, &mut r, &mut rel, &mut once, &mut recs);
        let (first, n_rel, n_once, n) = match decoded {
            Ok(d) => d,
            Err(e) => {
                self.stats.errors += 1;
                if e == DecodeError::NoBaseline {
                    self.stats.no_baseline += 1;
                }
                return Err(e);
            }
        };
        self.acks.commit(h.stream, verdict, h.tick);
        if let Some(s) = self.own_new.take() {
            self.own_kept[h.tick as usize % OWN_BASELINES] = (h.tick, s);
        }
        if new_stream {
            self.next_event = first;
        }
        // Units first: a Leave this datagram carries is older than any record in it.
        for (k, q) in recs[..n].iter().enumerate() {
            self.last_records[k] = q.slot;
            self.baselines.put(h.tick, *q);
            self.tracks[usize::from(q.slot)].push(h.tick, *q);
            self.world.apply_record(q);
        }
        self.last_n = n;
        self.stats.records += n as u64;
        for (k, e) in rel[..n_rel].iter().enumerate() {
            let id = first.wrapping_add(k as u16);
            if (id.wrapping_sub(self.next_event) as i16) < 0 {
                continue;
            }
            self.next_event = id.wrapping_add(1);
            self.event(h.tick, e);
        }
        for e in &once[..n_once] {
            self.event(h.tick, e);
        }
        self.zones.expire(h.tick);
        self.carry(h.tick);
        let rtt_ms = ms(now).wrapping_sub(h.time_echo).wrapping_sub(h.hold_ms);
        let rtt = (self.stats.packets > 0 && h.hold_ms < 1_023 && rtt_ms < 5_000)
            .then(|| f64::from(rtt_ms) / 1_000.0);
        self.clock.on_snapshot(h.tick, now, rtt, h.depth.min(i8::MAX as u8) as i8);
        self.stats.missed = h.missed;
        self.stats.depth = h.depth;
        self.orders.on_ack(h.order_ack);
        if h.flags & snap_flags::OVER != 0 {
            self.over = true;
        }
        if own {
            self.reseed(now, true);
            self.started = true;
        }
        Ok(Taken::Taken { tick: h.tick, own })
    }

    /// Reads the datagram past its header, changing nothing but the own champion's state.
    #[allow(clippy::type_complexity)]
    fn decode(
        &mut self,
        h: &SnapshotHeader,
        r: &mut BitReader<'_>,
        rel: &mut [WireEvent; MAX_LIST],
        once: &mut [WireEvent; MAX_LIST],
        recs: &mut [QUnit; MAX_UNITS],
    ) -> Result<(u16, usize, usize, usize), DecodeError> {
        self.world.tick = self.world.tick.max(h.tick);
        self.own_new = None;
        if h.flags & snap_flags::OWN != 0 {
            let kept = &self.own_kept;
            let s = OwnState::read(r, &self.own_widths, |back| {
                let t = h.tick.wrapping_sub(back);
                let (at, s) = kept[t as usize % OWN_BASELINES];
                (at == t).then_some(s)
            })?;
            self.world.apply_own(self.seat, &s)?;
            self.own_new = Some(s);
        }
        let (first, n_rel) = event::read_list(r, h.tick, true, rel)?;
        let (_, n_once) = event::read_list(r, h.tick, false, once)?;
        let mut n = 0;
        while r.read_bool() {
            let slot = r.read_u8();
            let q = if r.read_bool() {
                let back = Delta::read_back(r);
                if back == 0 {
                    return Err(DecodeError::Invalid);
                }
                let base =
                    self.baselines.get(slot, h.tick.wrapping_sub(back)).ok_or(DecodeError::NoBaseline)?;
                Delta::read(r, &base, back)?
            } else {
                QUnit::read_whole(r, slot)
            };
            if usize::from(slot) == self.seat || n == MAX_UNITS {
                return Err(DecodeError::Invalid);
            }
            recs[n] = q;
            n += 1;
        }
        if r.overflowed() {
            return Err(DecodeError::Truncated);
        }
        Ok((first, n_rel, n_once, n))
    }

    /// A new stream: another seat (a reconnect, a match begun). The server tells everything again,
    /// whole, and counts the orders from 0.
    fn start_stream(&mut self) {
        self.baselines.clear();
        self.own_kept.fill((u32::MAX, OwnState::default()));
        self.missiles.clear();
        self.zones.clear();
        for i in 0..MAX_UNITS {
            if i != self.seat && !(SEATS..FIRST_MINION).contains(&i) {
                self.tracks[i].forget();
                self.world.forget(i);
            }
        }
        self.orders.clear();
        self.replay_seq = 0;
        self.unsent = 0;
    }

    fn event(&mut self, tick: u32, e: &WireEvent) {
        self.stats.events += 1;
        match *e {
            WireEvent::Leave { slot, tick: left, .. } => {
                let i = usize::from(slot);
                if i != self.seat && self.tracks[i].newest().is_none_or(|(t, _)| t <= left) {
                    self.tracks[i].forget();
                    self.world.forget(i);
                }
            }
            WireEvent::MissileSpawn { .. } | WireEvent::MissileEnd { .. } => self.missiles.take(e),
            WireEvent::ZoneSpawn { .. } | WireEvent::ZoneEnd { .. } => self.zones.take(tick, e),
            WireEvent::LevelUp { unit, level } => {
                if let Some(c) = self.world.champs.get_mut(usize::from(unit)) {
                    c.level = level;
                }
            }
            WireEvent::Relic { pad, beam: false, .. } => {
                if let Some(r) = self.world.relics.get_mut(usize::from(pad)) {
                    r.up = false;
                    r.next = tick + ticks(rules::RELIC_RESPAWN);
                }
            }
            WireEvent::Outcome { winner } => self.world.outcome = Some(winner),
            WireEvent::Vote { team, yes, no, open, .. } => self.world.set_vote(team, yes, no, open),
            // For the page alone.
            WireEvent::Cast { .. }
            | WireEvent::Death { .. }
            | WireEvent::Respawn { .. }
            | WireEvent::Augment { .. }
            | WireEvent::Relic { beam: true, .. }
            | WireEvent::Damage { .. }
            | WireEvent::Heal { .. }
            | WireEvent::Shield { .. }
            | WireEvent::Attack { .. }
            | WireEvent::Area { .. }
            | WireEvent::Blink { .. }
            | WireEvent::Ping { .. }
            | WireEvent::Nothing => {}
        }
        self.feed[(self.feed_n % FEED as u64) as usize] = (tick, *e);
        self.feed_n += 1;
    }

    /// Units the datagram didn't mention are where their newest records carry them to by `tick`
    /// (the server sends one at rest, or walking straight, only once), and relics come back.
    fn carry(&mut self, tick: u32) {
        for i in 0..MAX_UNITS {
            if i == self.seat || !self.tracks[i].known {
                continue;
            }
            if let Some((t, q)) = self.tracks[i].newest()
                && t < tick
            {
                self.world.units[i].pos = pos_from_code(q.carried(tick - t));
            }
        }
        for r in &mut self.world.relics {
            if !r.up && r.next != 0 && tick >= r.next {
                r.up = true;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Prediction
    // -----------------------------------------------------------------------------------------

    /// What the champion was up to, before the snapshot (in the prediction) or as it says.
    fn cause(&self) -> Cause {
        let s = self.seat;
        let (w, p) = (&self.world, &self.pred);
        if !w.units[s].alive {
            return Cause::Dead;
        }
        if self.was_dead {
            return Cause::Respawned;
        }
        let dashing = |m: &Match| m.champs[s].dash.active;
        let crowd = |m: &Match| {
            let (u, t) = (&m.units[s], m.tick);
            u.cc.stun > t || u.cc.root > t || u.cc.airborne > t || u.cc.slow_until > t
        };
        let fighting = |m: &Match| {
            let c = &m.champs[s];
            matches!(c.intent, Intent::Attack(_) | Intent::AttackMove(_) | Intent::CastAt { .. })
                || c.cast.slot != NO_SLOT
                || m.units[s].windup > 0
        };
        if dashing(w) || dashing(p) {
            Cause::Dashing
        } else if crowd(w) || crowd(p) {
            Cause::Crowd
        } else if fighting(w) || fighting(p) {
            Cause::Fighting
        } else {
            Cause::Walking
        }
    }

    /// Starts the prediction over from the snapshot just taken, and steps it on through every
    /// command since: what the server will make of them, as far as the client can tell.
    /// `measure`: count how far the prediction moved (not when the client has jumped its clock).
    fn reseed(&mut self, now: f64, measure: bool) {
        let s = self.seat;
        let t = self.world.tick;
        let at = self.next_cmd;
        let before = self.pred.units[s].pos;
        let cause = self.cause();
        self.was_dead = !self.world.units[s].alive;
        self.pred.assume(&self.world);
        self.replay_seq = self.orders.acked;
        if t > self.next_cmd {
            self.next_cmd = t;
        }
        for k in t..self.next_cmd {
            self.pred_step(k);
        }
        self.own_ticks[t as usize % OWN_KEPT] = (t, self.world.units[s].pos);
        self.stats.reseeds += 1;
        if measure && self.started && at == self.next_cmd {
            let after = self.pred.units[s].pos;
            let error = before.distance(after);
            self.correction = Some(Correction { tick: t, error, cause });
            let off = self.smoothing(now) + (before - after);
            self.smooth = if off.length() > SMOOTH_SNAP { Vec2::ZERO } else { off };
            self.smooth_at = now;
        }
    }

    /// One tick of the own champion, as the server will step it: the command made for `k`, and the
    /// orders due by then, in order, as many as the server takes a tick.
    fn pred_step(&mut self, k: u32) {
        let steer = self.inputs.get(k).map_or(0, |c| c.steer);
        let mut input = SeatInput { steer, ..SeatInput::default() };
        while usize::from(input.n) < MAX_ORDERS_TICK && self.replay_seq != self.orders.next_seq {
            match self.orders.get(self.replay_seq) {
                Some(o) if o.tick <= k => {
                    input.push(o.order);
                    self.replay_seq = self.replay_seq.wrapping_add(1);
                }
                _ => break,
            }
        }
        self.pred.tick = k;
        self.pred.step_seat(self.seat, &input, &mut self.scratch);
        self.pred.tick = k + 1;
        self.own_ticks[(k + 1) as usize % OWN_KEPT] = (k + 1, self.pred.units[self.seat].pos);
    }

    /// What the own champion did in tick `k`'s first step (a reseed steps it again: that isn't
    /// news).
    fn own_events(&mut self, k: u32) {
        let s = self.seat as u16;
        for e in self.pred.events.as_slice() {
            let own = match *e {
                el_sim::events::Event::Cast { unit, slot, at } if unit == s => OwnEvent::Cast { slot, at },
                el_sim::events::Event::Blink { unit, from, to } if unit == s => OwnEvent::Blink { from, to },
                el_sim::events::Event::MissileSpawn { missile } => {
                    let m = &self.pred.missiles[usize::from(missile)];
                    if m.owner.idx != s {
                        continue;
                    }
                    OwnEvent::Missile { source: m.source.code(), from: m.origin }
                }
                el_sim::events::Event::Attack { unit, .. } if unit == s => OwnEvent::Swing,
                _ => continue,
            };
            self.own_feed[(self.own_feed_n % OWN_FEED as u64) as usize] = (k, own);
            self.own_feed_n += 1;
        }
    }

    // -----------------------------------------------------------------------------------------
    // Inputs
    // -----------------------------------------------------------------------------------------

    /// Makes the commands due by `now` (s), each from what `brain` says for its tick given the
    /// prediction (WASD in `steer`, and any orders it gives), steps the prediction through them,
    /// and returns the input packet to send: the newest commands and every order not yet acked.
    /// `view` is where the camera looks (position codes). `None` until the champion's state has
    /// come and the clock is set, and on a frame that made no command and gave no order.
    pub fn poll(
        &mut self,
        now: f64,
        view: [u32; 2],
        brain: &mut dyn FnMut(&Match, u32, &mut SeatInput),
    ) -> Option<Packets> {
        if !self.started || !self.clock.synced() {
            return None;
        }
        let target = self.clock.input_tick(now);
        if target >= self.next_cmd + CATCH_UP {
            // Far behind (a stalled page): the ticks missed are the server's to stand in for, and
            // what was ordered since the last packet is for the ticks the clock is on now.
            self.next_cmd = target.saturating_sub(2).max(self.world.tick);
            self.orders.restamp(self.unsent, self.next_cmd);
            self.reseed(now, false);
        }
        // Orders given since the last poll are the player's now: they go for the newest tick this
        // poll makes, not its oldest (a slow frame's batch starts a frame ago, in the server's past).
        if target >= self.next_cmd {
            self.orders.restamp(self.unsent, target);
        }
        let first = self.next_cmd;
        while self.next_cmd <= target {
            let k = self.next_cmd;
            let mut si = SeatInput::default();
            brain(&self.pred, k, &mut si);
            for o in si.orders() {
                self.orders.give(*o, k);
            }
            self.inputs.push(InputCmd { tick: k, steer: si.steer.min(8), view });
            self.pred_step(k);
            self.own_events(k);
            self.next_cmd = k + 1;
        }
        let newest = self.next_cmd.checked_sub(1)?;
        // Nothing new (a frame between two ticks): nothing to send. The last packet's commands and
        // acks stand until the next tick's.
        if self.next_cmd == first && self.unsent == self.orders.next_seq {
            return None;
        }
        let (ack_stream, ack_seq, ack_bits) = self.acks.ack();
        let mut out = Packets { list: [InputPacket::default(); MAX_PACKETS], n: 0 };
        // Windows of the newest commands, back until every command this poll made is in one. The
        // newest goes first: it carries the orders, and is the redundancy that rides out a lost
        // packet; if a sender can't take them all, it is the oldest commands that wait.
        let made = self.next_cmd - first;
        let windows = (made.div_ceil(MAX_CMDS as u32) as usize).clamp(1, MAX_PACKETS);
        for w in 0..windows {
            let end = newest.saturating_sub((w * MAX_CMDS) as u32);
            let mut p = InputPacket {
                ack_stream,
                ack_seq,
                ack_bits,
                client_time_ms: ms(now),
                ..InputPacket::default()
            };
            for i in 0..MAX_CMDS {
                let Some(c) = end.checked_sub(i as u32).and_then(|t| self.inputs.get(t)) else { break };
                p.cmds[i] = c;
                p.n_cmds += 1;
            }
            if p.n_cmds == 0 {
                continue;
            }
            // Orders ride the newest window only: a packet's orders are stamped no later than its
            // newest command, so an older window would apply them early.
            if w == 0 {
                self.orders.fill(&mut p);
            }
            out.list[out.n] = p;
            out.n += 1;
        }
        if out.n == 0 {
            return None;
        }
        self.unsent = self.orders.next_seq;
        self.stats.packets += out.n as u64;
        Some(out)
    }

    // -----------------------------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------------------------

    fn smoothing(&self, now: f64) -> Vec2 {
        self.smooth * (-(now - self.smooth_at).max(0.0) / SMOOTH).exp() as f32
    }

    /// Where the own champion is drawn at `now`: a tick behind the input clock, between the two
    /// ticks predicted there, with the last correction eased away.
    pub fn own_pos(&self, now: f64) -> Vec2 {
        let t = self.clock.own_tick(now) - 1.0;
        let a = t.floor().max(0.0) as u32;
        let w = (t - f64::from(a)).clamp(0.0, 1.0) as f32;
        let get = |k: u32| {
            let (at, p) = self.own_ticks[k as usize % OWN_KEPT];
            (at == k).then_some(p)
        };
        let here = self.pred.units[self.seat].pos;
        let pos = match (get(a), get(a + 1)) {
            (Some(p), Some(q)) => p.lerp(q, w),
            (Some(p), None) | (None, Some(p)) => p,
            (None, None) => here,
        };
        pos + self.smoothing(now)
    }

    /// Where unit `i` (not the own champion) is drawn at `now`, if the team sees it.
    pub fn unit(&self, i: usize, now: f64) -> Option<Sample> {
        let t = self.tracks.get(i)?;
        if !t.known {
            return None;
        }
        t.sample(self.clock.view_tick(now))
    }

    /// The tick missiles are drawn at: the own champion's.
    pub fn missile_tick(&self, now: f64) -> f64 {
        self.clock.own_tick(now)
    }
}
