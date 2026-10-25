//! What each seat is sent each tick (`docs/NETCODE.md`, "The wire" and "Fog is the server's").
//!
//! Each unit's record is quantized once a tick a team ([`MatchNet::record`]) and kept [`KEPT`]
//! ticks, so a seat's records are deltas against the last one it acked, which the seat keeps however
//! old. A record goes once: it is sent again only if what it says changes, or the datagram that
//! carried it is known lost (the client acks a newer one and not it: it takes nothing older than
//! what it has), or no ack came for [`KEPT`] ticks. A seat is sent only its team's view: allies,
//! and what the team sees; when it stops seeing a unit, a `Leave`. Events go through a team rule
//! each, written out case by case: there is no catch-all arm, so a new event can't reach anyone
//! until someone decides who may know it.

use el_proto::event::{self, MAX_LIST, WireEvent};
use el_proto::quant::{facing_code, pos_code};
use el_proto::snapshot::{SnapshotHeader, snap_flags};
use el_proto::unit::{BACK_MAX, Delta, KEPT, QUnit};
use el_proto::{BitWriter, ack};
use el_sim::events::Event;
use el_sim::game::Match;
use el_sim::map::Map;
use el_sim::missile::MAX_MISSILES;
use el_sim::own::{OWN_BASELINES, OwnState, OwnWidths, own_widths};
use el_sim::storage::boxed;
use el_sim::units::{Kind, MAX_UNITS, SEATS, team_of_seat};

use crate::metrics::Metrics;
use crate::seat::SeatNet;

/// Reliable events a seat may have outstanding.
pub const OUTBOX: usize = 256;
/// Datagrams a seat remembers sending, waiting for their acks.
const SENT: usize = 64;
/// Datagrams a seat may be sent in one tick.
pub const MAX_PARTS: usize = 2;
/// "Its caster (or target) isn't seen".
pub const HIDDEN: u8 = u8::MAX;

/// A match's units' records for the last [`KEPT`] ticks, as each team has them.
pub struct MatchNet {
    history: Box<[[QUnit; 2]]>,
    own_widths: OwnWidths,
}

impl Default for MatchNet {
    fn default() -> Self {
        Self::new()
    }
}

impl MatchNet {
    pub fn new() -> Self {
        Self { history: boxed(KEPT * MAX_UNITS, [QUnit::default(); 2]), own_widths: own_widths() }
    }

    /// Quantizes every live unit for both teams, once, after the tick (nothing, when no seat is
    /// sent to: a match of bots).
    pub fn record(&mut self, m: &Match, any: bool) {
        if !any {
            return;
        }
        let base = (m.tick as usize % KEPT) * MAX_UNITS;
        for i in 0..MAX_UNITS {
            if m.units[i].alive {
                self.history[base + i] = m.quantize_teams(i);
            }
        }
    }

    fn get(&self, tick: u32, slot: usize, team: u8) -> &QUnit {
        &self.history[(tick as usize % KEPT) * MAX_UNITS + slot][usize::from(team)]
    }
}

/// What a client has of a unit's record.
enum Told {
    /// Not all it says now: it goes.
    No,
    /// All of it, in one datagram not yet acked: it goes once more.
    Once,
    /// All of it, acked or sent twice and not known lost.
    Yes,
}

#[derive(Clone, Copy, Default)]
struct Sent {
    seq: u32,
    tick: u32,
    slots: [u64; 4],
    events_end: u16,
    /// It carried the own champion.
    own: bool,
    valid: bool,
}

/// One seat's replication state.
pub struct Replica {
    pub stream: u8,
    seq: u32,
    sent: [Sent; SENT],
    /// One past the newest datagram acked: a packet's acks of older ones were all in the packets
    /// before it (the client takes nothing older than what it has).
    acked: u32,
    /// The own champion's states sent of late, by tick (0: none), and the newest the client acked.
    own_sent: [(u32, OwnState); OWN_BASELINES],
    own_base: (u32, OwnState),
    /// For each unit, the newest record of it the client acked, and its tick (0: none).
    base: [QUnit; MAX_UNITS],
    base_tick: [u32; MAX_UNITS],
    /// Records of it sent since that one, at most (what the client may have taken since: it keeps
    /// the newest [`KEPT`]).
    since: [u8; MAX_UNITS],
    /// The tick of the newest record of it sent; whether that one is known lost; and whether it
    /// has gone only once with what it says (it goes twice, so one datagram lost costs nothing).
    last_sent: [u32; MAX_UNITS],
    lost: [u64; 4],
    single: [u64; 4],
    priority: [f32; MAX_UNITS],
    /// Units the client has been told of, and which life.
    told: [u64; 4],
    told_gen: [u8; MAX_UNITS],
    missiles: [u64; MAX_MISSILES / 64],
    missile_gen: [u8; MAX_MISSILES],
    out: [WireEvent; OUTBOX],
    out_first: u16,
    out_next: u16,
    pub out_dropped: u32,
    once: [WireEvent; MAX_LIST],
    n_once: usize,
}

impl Default for Replica {
    fn default() -> Self {
        Self {
            stream: 0,
            seq: 0,
            sent: [Sent::default(); SENT],
            acked: 0,
            own_sent: [(0, OwnState::default()); OWN_BASELINES],
            own_base: (0, OwnState::default()),
            base: [QUnit::default(); MAX_UNITS],
            base_tick: [0; MAX_UNITS],
            since: [0; MAX_UNITS],
            last_sent: [0; MAX_UNITS],
            lost: [0; 4],
            single: [0; 4],
            priority: [0.0; MAX_UNITS],
            told: [0; 4],
            told_gen: [0; MAX_UNITS],
            missiles: [0; MAX_MISSILES / 64],
            missile_gen: [0; MAX_MISSILES],
            out: [WireEvent::Nothing; OUTBOX],
            out_first: 0,
            out_next: 0,
            out_dropped: 0,
            once: [WireEvent::Nothing; MAX_LIST],
            n_once: 0,
        }
    }
}

fn bit(words: &[u64], i: usize) -> bool {
    words[i / 64] & (1 << (i % 64)) != 0
}

/// Calls `f` with every bit set in `words`.
fn for_each(words: &[u64], mut f: impl FnMut(usize)) {
    for (w, &word) in words.iter().enumerate() {
        let mut bits = word;
        while bits != 0 {
            f(w * 64 + bits.trailing_zeros() as usize);
            bits &= bits - 1;
        }
    }
}

fn set(words: &mut [u64], i: usize, on: bool) {
    if on {
        words[i / 64] |= 1 << (i % 64);
    } else {
        words[i / 64] &= !(1 << (i % 64));
    }
}

impl Replica {
    /// A new stream for a player (re)taking the seat: everything is told again, whole.
    pub fn reset(&mut self) {
        let stream = ack::next_stream(self.stream);
        *self = Replica { stream, ..Replica::default() };
    }

    pub fn has_stream(&self) -> bool {
        self.stream != 0
    }

    /// What an input packet acks: the stream, the newest datagram taken and the 32 before it.
    /// `now` is the match's tick, `net` its records, `team` the seat's.
    pub fn on_ack(&mut self, net: &MatchNet, team: u8, now: u32, stream: u8, seq16: u16, bits: u32) {
        if stream != self.stream || self.seq == 0 {
            return;
        }
        let newest = ack::expand(seq16, self.seq - 1);
        if newest >= self.seq || newest < self.acked {
            return;
        }
        self.apply_ack(net, team, now, newest);
        for k in 0..32 {
            if bits & (1 << k) != 0 && newest > k && newest - 1 - k >= self.acked {
                self.apply_ack(net, team, now, newest - 1 - k);
            }
        }
        self.acked = newest + 1;
        // What is older than the newest taken and wasn't taken never will be: the client takes
        // nothing older than what it has. What it said goes again, if nothing newer has.
        for k in 0..SENT {
            let e = self.sent[k];
            if !e.valid || e.seq >= newest {
                continue;
            }
            for_each(&e.slots, |i| {
                if self.last_sent[i] == e.tick {
                    set(&mut self.lost, i, true);
                }
            });
            self.sent[k].valid = false;
        }
    }

    fn apply_ack(&mut self, net: &MatchNet, team: u8, now: u32, s: u32) {
        let e = self.sent[s as usize % SENT];
        if !e.valid || e.seq != s {
            return;
        }
        // (A record older than the match's history can't be a baseline: the seat would need to have
        // kept it as it went.)
        if now - e.tick < KEPT as u32 {
            for_each(&e.slots, |i| {
                if e.tick > self.base_tick[i] {
                    self.base[i] = *net.get(e.tick, i, team);
                    self.base_tick[i] = e.tick;
                    // Sent since: none if it was the newest, and never more than one a tick.
                    let after = if self.last_sent[i] == e.tick { 0 } else { now - e.tick };
                    self.since[i] = self.since[i].min(after.min(u32::from(u8::MAX)) as u8);
                }
            });
        }
        let own = &self.own_sent[e.tick as usize % OWN_BASELINES];
        if e.own && own.0 == e.tick && e.tick > self.own_base.0 {
            self.own_base = *own;
        }
        if (e.events_end.wrapping_sub(self.out_first) as i16) > 0 {
            self.out_first = e.events_end;
        }
        self.sent[s as usize % SENT].valid = false;
    }

    fn queue(&mut self, e: WireEvent) {
        if e.reliable() {
            if usize::from(self.out_next.wrapping_sub(self.out_first)) < OUTBOX {
                self.out[usize::from(self.out_next) % OUTBOX] = e;
                self.out_next = self.out_next.wrapping_add(1);
            } else {
                self.out_dropped += 1;
            }
        } else if self.n_once < MAX_LIST {
            self.once[self.n_once] = e;
            self.n_once += 1;
        }
    }

    fn knows(&self, i: usize) -> bool {
        i < MAX_UNITS && bit(&self.told, i)
    }

    /// This tick's events, as `team` may know them, into the seat's queues.
    pub fn gather(&mut self, m: &Match, team: u8) {
        self.n_once = 0;
        let now = m.tick;
        let t = usize::from(team);
        let seen = &m.vision.units[t];
        let sees = |i: u16| usize::from(i) < MAX_UNITS && seen.get(usize::from(i));
        // Units it can no longer see, or that died, or whose slot is someone else's now.
        let told = self.told;
        for_each(&told, |i| {
            let u = &m.units[i];
            if !u.alive || u.generation as u8 != self.told_gen[i] || !seen.get(i) {
                self.queue(WireEvent::Leave { slot: i as u8, generation: self.told_gen[i], tick: now });
                set(&mut self.told, i, false);
            }
        });
        let shown = |i: u16, sees: &dyn Fn(u16) -> bool| {
            if usize::from(i) < SEATS || sees(i) { i as u8 } else { HIDDEN }
        };
        for e in m.events.as_slice() {
            match *e {
                // Seen doing it.
                Event::Cast { unit, slot, at } => {
                    if sees(unit) {
                        self.queue(WireEvent::Cast { unit: unit as u8, slot, at: pos_code(at) });
                    }
                }
                Event::Attack { unit, target } => {
                    if sees(unit) && sees(target) {
                        self.queue(WireEvent::Attack { unit: unit as u8, target: target as u8 });
                    }
                }
                Event::Blink { unit, to, .. } => {
                    if sees(unit) {
                        self.queue(WireEvent::Blink { unit: unit as u8, to: pos_code(to) });
                    }
                }
                // About a unit the seat knew of: who did it only if that's seen too.
                Event::Damage { target, source, amount } => {
                    if self.knows(usize::from(target)) || sees(target) {
                        let source = if sees(source) { source as u8 } else { HIDDEN };
                        self.queue(WireEvent::Damage { target: target as u8, source, amount: amount as u16 });
                    }
                }
                Event::Heal { target, amount } => {
                    if self.knows(usize::from(target)) || sees(target) {
                        self.queue(WireEvent::Heal { target: target as u8, amount: amount as u16 });
                    }
                }
                // A champion's death is the kill feed's, everyone's; anything else's, the seat's
                // only if it knew of it.
                Event::Death { unit, killer } => {
                    if usize::from(unit) < SEATS || self.knows(usize::from(unit)) {
                        self.queue(WireEvent::Death { unit: unit as u8, killer: shown(killer, &sees) });
                    }
                }
                // Champions' respawns and levels are on the scoreboard: everyone's.
                Event::Respawn { unit } => self.queue(WireEvent::Respawn { unit: unit as u8 }),
                Event::LevelUp { unit, level } => self.queue(WireEvent::LevelUp { unit: unit as u8, level }),
                // An area: its own team's, or where the team can see it land.
                Event::Area { at, radius, team: owner } => {
                    if owner == team || Map::lit(&m.vision.grid[t], at) {
                        self.queue(WireEvent::Area { at: pos_code(at), radius: radius as u16, team: owner });
                    }
                }
                // Relics are on the map for all; who took one, only if seen.
                Event::RelicTaken { pad, unit } => self.queue(WireEvent::Relic {
                    pad,
                    beam: false,
                    unit: if sees(unit) { unit as u8 } else { HIDDEN },
                }),
                Event::RelicBeam { pad } => self.queue(WireEvent::Relic { pad, beam: true, unit: HIDDEN }),
                // Augments are the team's own.
                Event::Augment { unit, augment } => {
                    if team_of_seat(usize::from(unit)) == team {
                        self.queue(WireEvent::Augment { unit: unit as u8, augment });
                    }
                }
                Event::Outcome { winner } => self.queue(WireEvent::Outcome { winner }),
                // A team's word is the team's: its pings (naming a unit only while it's seen) and its
                // vote.
                Event::Ping { seat, kind, at, unit } => {
                    if team_of_seat(usize::from(seat)) == team {
                        let unit = if sees(unit) { unit as u8 } else { HIDDEN };
                        self.queue(WireEvent::Ping { seat, kind, at: pos_code(at), unit });
                    }
                }
                Event::Vote { team: voters, yes, no, open, passed } => {
                    if voters == team {
                        self.queue(WireEvent::Vote { team: voters, yes, no, open, passed });
                    }
                }
                // Missiles are told below, from what the team sees of each.
                Event::MissileSpawn { .. } | Event::MissileEnd { .. } => {}
            }
        }
        // Missiles: told when the team first sees one (where it is then: a missile from a caster it
        // can't see names no caster), and told when it ends.
        let missiles = self.missiles;
        for_each(&missiles, |i| {
            let ms = &m.missiles[i];
            if !ms.alive || ms.generation != self.missile_gen[i] {
                self.queue(WireEvent::MissileEnd {
                    id: i as u16,
                    generation: self.missile_gen[i],
                    at: pos_code(ms.pos),
                });
                set(&mut self.missiles, i, false);
            }
        });
        for i in m.vision.missiles[t].iter() {
            let ms = &m.missiles[i];
            if ms.alive && !bit(&self.missiles, i) {
                let owner = usize::from(ms.owner.idx);
                let owner = if owner < MAX_UNITS && seen.get(owner) { owner as u8 } else { HIDDEN };
                let target = usize::from(ms.target.idx);
                let target =
                    if ms.homing && target < MAX_UNITS && seen.get(target) { target as u8 } else { HIDDEN };
                let travelled = (ms.pos - ms.origin).length();
                self.queue(WireEvent::MissileSpawn {
                    id: i as u16,
                    generation: ms.generation,
                    team: ms.team,
                    source: ms.source.code() as u16,
                    owner,
                    origin: pos_code(ms.pos),
                    t0: now,
                    target,
                    dir: facing_code(el_sim::math::angle(ms.dir)),
                    speed: ms.speed as u16,
                    range: if ms.homing { 0 } else { (ms.range - travelled).max(0.0) as u16 },
                    width: ms.width.min(255.0) as u8,
                });
                set(&mut self.missiles, i, true);
                self.missile_gen[i] = ms.generation;
            }
        }
    }

    /// Builds this tick's datagrams for `seat` into `out` (`[u16 len][datagram]` frames).
    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &mut self,
        m: &Match,
        net: &MatchNet,
        seat: usize,
        s: &SeatNet,
        now_us: u64,
        buf: &mut [u8],
        out: &mut rtrb::Producer<u8>,
        metrics: &Metrics,
    ) {
        let team = team_of_seat(seat);
        let now = m.tick;
        let seen = &m.vision.units[usize::from(team)];
        let me = &m.units[seat];
        let eye = if me.alive { me.pos } else { el_proto::quant::pos_from_code(s.view()) };
        // Candidates: what the team sees (but the seat's own champion), with news to tell.
        let mut cand = [(0u32, 0u8); MAX_UNITS];
        let mut repeat = [0u64; 4];
        let mut n = 0;
        for i in seen.iter() {
            if i == seat || !m.units[i].alive {
                continue;
            }
            let u = &m.units[i];
            let weight = match u.kind {
                Kind::Champion if u.team != team => 4.0,
                Kind::Champion => 2.5,
                k if k.is_structure() => 1.0,
                _ => 1.0,
            };
            let d = el_sim::math::distance(u.pos, eye);
            self.priority[i] += weight / (1.0 + d / 800.0);
            match self.told(net, i, team, now) {
                Told::Yes => {
                    // Nothing the client hasn't been told: nothing to say.
                    self.priority[i] = 0.0;
                    continue;
                }
                Told::Once => set(&mut repeat, i, true),
                Told::No => {}
            }
            cand[n] = ((self.priority[i] * 1_000.0) as u32, i as u8);
            n += 1;
        }
        let cand = &mut cand[..n];
        cand.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let limit = usize::from(s.max_datagram).min(el_proto::MAX_DATAGRAM);
        let mut next = 0;
        for part in 0..MAX_PARTS {
            if part > 0 && next >= cand.len() {
                break;
            }
            let mut w = BitWriter::with_limit(buf, limit);
            let mut flags = 0;
            if part == 0 {
                flags |= snap_flags::OWN;
            }
            if m.outcome.is_some() {
                flags |= snap_flags::OVER;
            }
            let hold = now_us.saturating_sub(s.echo.1) / 1_000;
            SnapshotHeader {
                tick: now,
                seq: self.seq as u16,
                stream: self.stream,
                order_ack: s.next_seq,
                depth: s.depth(now).min(63) as u8,
                missed: s.missed() as u8,
                time_echo: s.echo.0,
                hold_ms: hold.min(1_023) as u16,
                flags,
            }
            .write(&mut w);
            let mut events_end = self.out_first;
            if part == 0 {
                let own = m.own_state(seat);
                let (b, base) = (self.own_base.0, &self.own_base.1);
                let base = (b != 0 && now - b < OWN_BASELINES as u32).then_some((now - b, base));
                own.write(&mut w, base, &net.own_widths);
                self.own_sent[now as usize % OWN_BASELINES] = (now, own);
                // Reliable events, oldest first, as many as fit in half of what's left.
                let mut list = [WireEvent::Nothing; MAX_LIST];
                let pending = usize::from(self.out_next.wrapping_sub(self.out_first)).min(MAX_LIST);
                let room = w.bits_remaining() / 2;
                let mut k = 0;
                let mut bits = event::list_bits(now, true, &[]);
                while k < pending {
                    let e = self.out[usize::from(self.out_first.wrapping_add(k as u16)) % OUTBOX];
                    if bits + e.bits(now) > room {
                        break;
                    }
                    bits += e.bits(now);
                    list[k] = e;
                    k += 1;
                }
                event::write_list(&mut w, now, Some(self.out_first), &list[..k]);
                events_end = self.out_first.wrapping_add(k as u16);
                // This tick's other events, while they fit in a quarter.
                let room = w.bits_remaining() / 4;
                let mut k = 0;
                let mut bits = event::list_bits(now, false, &[]);
                while k < self.n_once && bits + self.once[k].bits(now) <= room {
                    bits += self.once[k].bits(now);
                    k += 1;
                }
                event::write_list(&mut w, now, None, &self.once[..k]);
            } else {
                event::write_list(&mut w, now, Some(self.out_first), &[]);
                event::write_list(&mut w, now, None, &[]);
            }
            // Units, most pressing first, while they fit (a bit kept for the list's end).
            let mut slots = [0u64; 4];
            while next < cand.len() {
                let i = usize::from(cand[next].1);
                let q = net.get(now, i, team);
                let plan = self.plan(q, i, now);
                let bits = plan.map_or(q.whole_bits(), |d| d.bits());
                if w.bits_remaining() < bits + 2 {
                    break;
                }
                w.write_bool(true);
                w.write_u8(i as u8);
                match plan {
                    Some(d) => {
                        w.write_bool(true);
                        d.write(&mut w);
                        Metrics::add(&metrics.deltas, 1);
                    }
                    None => {
                        w.write_bool(false);
                        q.write_whole(&mut w);
                    }
                }
                Metrics::add(&metrics.records, 1);
                Metrics::add(&metrics.record_bits, bits as u64);
                set(&mut slots, i, true);
                set(&mut self.told, i, true);
                set(&mut self.lost, i, false);
                set(&mut self.single, i, !bit(&repeat, i));
                self.told_gen[i] = q.generation;
                self.last_sent[i] = now;
                self.since[i] = self.since[i].saturating_add(1);
                self.priority[i] = 0.0;
                next += 1;
            }
            w.write_bool(false);
            let bytes = w.bytes_written();
            // Kept as sent whatever becomes of it: one that never leaves is lost like any other, so
            // what it held goes again.
            self.sent[self.seq as usize % SENT] =
                Sent { seq: self.seq, tick: now, slots, events_end, own: part == 0, valid: true };
            self.seq += 1;
            // Length and payload in one commit: the egress thread reads while this writes.
            if w.overflowed() || !crate::queues::write_packet(out, &buf[..bytes]) {
                Metrics::add(&metrics.out_drops, 1);
                return;
            }
            Metrics::add(&metrics.snapshots, 1);
            Metrics::add(&metrics.snapshot_bytes, bytes as u64);
            Metrics::max(&metrics.snapshot_max_bytes, bytes as u64);
        }
    }

    /// Unit `i`'s record now, `q`, as a delta against the newest the client acked, if one may be:
    /// one the client still keeps.
    fn plan(&self, q: &QUnit, i: usize, now: u32) -> Option<Delta> {
        if !self.has_base(i, now) {
            return None;
        }
        Delta::plan(q, &self.base[i], now - self.base_tick[i])
    }

    /// Whether unit `i`'s acked record may be a baseline now.
    fn has_base(&self, i: usize, now: u32) -> bool {
        let b = self.base_tick[i];
        b != 0 && b < now && now - b <= BACK_MAX && usize::from(self.since[i]) < KEPT
    }

    /// What the client has (or soon will have) of what unit `i`'s record says now.
    fn told(&self, net: &MatchNet, i: usize, team: u8, now: u32) -> Told {
        if !bit(&self.told, i) || bit(&self.lost, i) {
            return Told::No;
        }
        let q = net.get(now, i, team);
        let sent = self.last_sent[i];
        if sent > self.base_tick[i] {
            let back = now - sent;
            if !(1..KEPT as u32).contains(&back) || !q.adds_nothing_to(net.get(sent, i, team), back) {
                Told::No
            } else if bit(&self.single, i) {
                Told::Once
            } else {
                Told::Yes
            }
        } else if self.has_base(i, now) && q.adds_nothing_to(&self.base[i], now - self.base_tick[i]) {
            Told::Yes
        } else {
            Told::No
        }
    }
}
