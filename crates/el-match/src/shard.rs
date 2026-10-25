//! A shard: a pool of matches stepped together each tick on one thread, each seat's inputs drained
//! and its snapshots queued, with nothing allocated or locked after [`crate::build`]
//! (`docs/NETCODE.md`, "Many matches a process").

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use el_proto::MAX_DATAGRAM;
use el_sim::game::SeatInput;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::record;
use el_sim::storage::boxed;
use el_sim::units::SEATS;
use el_sim::{Match, bot};

use crate::metrics::Metrics;
use crate::queues::{Control, MatchState, Outcome, ShardEnds, ShardShared};
use crate::replicate::{MatchNet, Replica};
use crate::seat::SeatNet;

#[derive(Clone, Copy)]
pub struct ShardConfig {
    /// Matches the shard holds.
    pub matches: usize,
    /// The map every match plays on.
    pub map: &'static Map,
    /// Marks each tick a hot region (`el_alloc::set_hot`), so an allocation in one is counted.
    pub hot_guard: Option<fn(bool)>,
}

pub struct Shard {
    cfg: ShardConfig,
    shared: Arc<ShardShared>,
    ends: ShardEnds,
    pub matches: Box<[Match]>,
    nets: Box<[MatchNet]>,
    pub seats: Box<[SeatNet]>,
    pub replicas: Box<[Replica]>,
    running: Box<[bool]>,
    reported: Box<[bool]>,
    scratch: PathScratch,
    buf: Box<[u8]>,
    /// A record frame being written.
    frame: Box<[u8]>,
}

impl Shard {
    pub(crate) fn new(cfg: ShardConfig, shared: Arc<ShardShared>, ends: ShardEnds) -> Self {
        let n = cfg.matches;
        Self {
            cfg,
            shared,
            ends,
            matches: (0..n).map(|_| Match::new(cfg.map, 0, [0; SEATS])).collect(),
            nets: (0..n).map(|_| MatchNet::new()).collect(),
            seats: (0..n * SEATS).map(|_| SeatNet::default()).collect(),
            replicas: (0..n * SEATS).map(|_| Replica::default()).collect(),
            running: boxed(n, false),
            reported: boxed(n, false),
            scratch: PathScratch::new(),
            buf: boxed(MAX_DATAGRAM, 0u8),
            frame: boxed(record::MAX_FRAME, 0u8),
        }
    }

    pub fn shared(&self) -> &Arc<ShardShared> {
        &self.shared
    }

    pub fn config(&self) -> &ShardConfig {
        &self.cfg
    }

    /// One tick of every running match.
    pub fn tick(&mut self) {
        let now_us = self.shared.now_us();
        self.tick_at(now_us);
    }

    /// [`tick`](Self::tick) with the clock given (µs on the clock the inputs' `recv_us` are on):
    /// what a test on virtual time calls.
    pub fn tick_at(&mut self, now_us: u64) {
        while let Some(c) = self.shared.control.pop() {
            self.control(c);
        }
        let mut live = 0;
        for m in 0..self.cfg.matches {
            if self.running[m] {
                self.tick_match(m, now_us);
                live += 1;
            }
        }
        Metrics::set(&self.shared.metrics.matches, live);
        self.shared.tick.fetch_add(1, Ordering::Release);
    }

    /// Puts the frame in `self.frame[..n]` on match `m`'s record ring (or counts it lost).
    fn record(&mut self, m: usize, n: usize) {
        let ring = &mut self.ends.records[m];
        if n == 0 || ring.slots() < n || ring.push_entire_slice(&self.frame[..n]).is_err() {
            Metrics::add(&self.shared.metrics.record_drops, 1);
            return;
        }
        Metrics::add(&self.shared.metrics.record_bytes, n as u64);
    }

    fn control(&mut self, c: Control) {
        match c {
            Control::Start { m, seed, picks, humans, secret } => {
                let m = usize::from(m);
                if m >= self.cfg.matches {
                    return;
                }
                self.matches[m].reset(seed, picks);
                if secret != 0 {
                    self.matches[m].set_offer_secret(secret);
                }
                let n = record::write_header(&mut self.frame, &record::Header::new(seed, picks));
                self.record(m, n);
                for seat in 0..SEATS {
                    let g = m * SEATS + seat;
                    self.seats[g] = SeatNet::default();
                    self.seats[g].human = humans & (1 << seat) != 0;
                    self.replicas[g].reset();
                }
                self.running[m] = true;
                self.reported[m] = false;
                self.shared.states[m].store(MatchState::Running as u32, Ordering::Release);
            }
            Control::Seat { m, seat, max_datagram } => {
                let g = usize::from(m) * SEATS + usize::from(seat);
                if g < self.seats.len() {
                    // What the seat's last player sent and the shard hasn't taken goes: their
                    // orders aren't this player's.
                    while self.ends.inputs[g].pop().is_ok() {}
                    self.seats[g].seat(max_datagram);
                    self.replicas[g].reset();
                    let (m, tick) = (usize::from(m), self.matches[usize::from(m)].tick);
                    let n = record::write_seat(&mut self.frame, tick, seat, true);
                    self.record(m, n);
                }
            }
            Control::Unseat { m, seat } => {
                let g = usize::from(m) * SEATS + usize::from(seat);
                if g < self.seats.len() {
                    self.seats[g].human = false;
                    let (m, tick) = (usize::from(m), self.matches[usize::from(m)].tick);
                    let n = record::write_seat(&mut self.frame, tick, seat, false);
                    self.record(m, n);
                }
            }
            Control::End { m } => {
                let m = usize::from(m);
                if m < self.cfg.matches {
                    self.running[m] = false;
                    self.shared.states[m].store(MatchState::Free as u32, Ordering::Release);
                }
            }
        }
    }

    fn tick_match(&mut self, mi: usize, now_us: u64) {
        let started = Instant::now();
        let next = self.matches[mi].tick;
        // Inputs: every packet that came, its commands, orders and acks.
        for seat in 0..SEATS {
            let g = mi * SEATS + seat;
            while let Ok(msg) = self.ends.inputs[g].pop() {
                Metrics::add(&self.shared.metrics.inputs, 1);
                if !self.seats[g].human {
                    continue;
                }
                self.seats[g].take(&msg.packet, msg.recv_us, next);
                let p = &msg.packet;
                self.replicas[g].on_ack(p.ack_stream, p.ack_seq, p.ack_bits);
            }
        }
        let mut inputs = [SeatInput::default(); SEATS];
        for (seat, input) in inputs.iter_mut().enumerate() {
            let g = mi * SEATS + seat;
            if self.seats[g].human {
                *input = self.seats[g].input_for(next);
            } else {
                bot::think(&self.matches[mi], seat, input);
            }
        }
        // The record: what the match is stepped with, and now and then what it came to.
        let n = record::write_tick(&mut self.frame, next, &inputs);
        self.record(mi, n);
        self.matches[mi].step(&inputs, &mut self.scratch);
        // The augment keys the tick drew: after its step, the first anyone else hears of them.
        for k in 0..self.matches[mi].keys_drawn().len() {
            let (seat, screen, key) = self.matches[mi].keys_drawn()[k];
            let n = record::write_key(&mut self.frame, next, seat, screen, key);
            self.record(mi, n);
        }
        if self.matches[mi].tick.is_multiple_of(record::HASH_EVERY) {
            let m = &self.matches[mi];
            let n = record::write_hash(&mut self.frame, m.tick, m.hash());
            self.record(mi, n);
        }
        let stepped = Instant::now();
        self.shared.metrics.sim_hist.record((stepped - started).as_micros() as u64);
        // Replication: each seat a player holds.
        let m = &self.matches[mi];
        self.nets[mi].record(m);
        for seat in 0..SEATS {
            let g = mi * SEATS + seat;
            if !self.seats[g].human || !self.replicas[g].has_stream() {
                continue;
            }
            let team = el_sim::units::team_of_seat(seat);
            self.replicas[g].gather(m, team);
            self.replicas[g].send(
                m,
                &self.nets[mi],
                seat,
                &self.seats[g],
                now_us,
                &mut self.buf,
                &mut self.ends.outputs[g],
                &self.shared.metrics,
            );
        }
        self.shared.metrics.replicate_hist.record(stepped.elapsed().as_micros() as u64);
        self.shared.ticks[mi].store(m.tick, Ordering::Release);
        if let Some(winner) = m.outcome
            && !self.reported[mi]
        {
            self.reported[mi] = true;
            let (tick, hash) = (m.tick, m.hash());
            let _ = self.shared.outcomes.push(Outcome::Over { m: mi as u16, winner, tick, hash });
            self.shared.states[mi].store(MatchState::Over as u32, Ordering::Release);
            let n = record::write_outcome(&mut self.frame, tick, winner);
            self.record(mi, n);
        }
    }
}
