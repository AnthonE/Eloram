//! What the runtime's tests share: the map, a lossy link in virtual time (Before Colony's `Link`),
//! and a rig that runs a shard with clients on the far side of links, each driven by a brain.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros, dead_code)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, OnceLock};

use el_client_core::{Client, Correction};
use el_match::{
    Control, EgressEnds, InputMsg, RecordEnds, SeatLease, Shard, ShardConfig, ShardShared, build, read_packet,
};
use el_proto::quant::pos_code;
use el_proto::{InputPacket, Order};
use el_sim::game::SeatInput;
use el_sim::map::{self, Map};
use el_sim::math::Rng;
use el_sim::units::{SEATS, team_of_seat};
use el_sim::{Match, bot};
use glam::Vec2;

pub fn map() -> &'static Map {
    static M: OnceLock<&'static Map> = OnceLock::new();
    M.get_or_init(Map::leaked)
}

/// Each seat's champion for `seed`: the pool dealt round.
pub fn picks(seed: u64) -> [u8; SEATS] {
    std::array::from_fn(|s| ((seed as usize + s * 3) % el_sim::content::champions::CHAMPIONS.len()) as u8)
}

/// A client for `seat` of the match of `seed`, told what the lobby tells a player (not the seed).
pub fn client(seat: usize, seed: u64) -> Client {
    Client::new(map(), seat, picks(seed), el_sim::game::tiers_for(seed), el_sim::game::headliner_for(seed))
}

#[derive(Clone, Copy, Debug)]
pub struct LinkSpec {
    /// One way, s.
    pub base: f64,
    pub jitter: f64,
    pub loss: f32,
    /// Everything sent in this window (s) is lost.
    pub outage: Option<(f64, f64)>,
}

/// 100 ms round trip, ±20 ms of jitter each way, 5 % loss.
pub const BAD: LinkSpec = LinkSpec { base: 0.05, jitter: 0.02, loss: 0.05, outage: None };
/// The same delays with nothing lost.
pub const CLEAN: LinkSpec = LinkSpec { loss: 0.0, ..BAD };

/// One direction of a link: datagrams delayed, reordered by jitter, some lost.
pub struct Link {
    rng: Rng,
    spec: LinkSpec,
    queue: BinaryHeap<Reverse<(u64, u64, Vec<u8>)>>, // (deliver µs, seq, bytes)
    seq: u64,
}

impl Link {
    pub fn new(seed: u64, spec: LinkSpec) -> Self {
        Self { rng: Rng::new(seed), spec, queue: BinaryHeap::new(), seq: 0 }
    }

    pub fn send(&mut self, now: f64, bytes: Vec<u8>) {
        if self.rng.next_f32() < self.spec.loss
            || self.spec.outage.is_some_and(|(from, to)| (from..to).contains(&now))
        {
            return;
        }
        let at = now + self.spec.base + f64::from(self.rng.next_f32() * 2.0 - 1.0) * self.spec.jitter;
        self.seq += 1;
        self.queue.push(Reverse(((at * 1e6) as u64, self.seq, bytes)));
    }

    pub fn deliver(&mut self, now: f64) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(Reverse((at, _, _))) = self.queue.peek() {
            if *at as f64 / 1e6 > now {
                break;
            }
            out.push(self.queue.pop().unwrap().0.2);
        }
        out
    }
}

/// What plays a seat from its client: given the prediction, the seat and the tick, its steer and
/// orders for that tick.
pub type Brain = Box<dyn FnMut(&Match, usize, u32, &mut SeatInput)>;

/// The scripted bot, playing from the client's view of the match.
pub fn bot_brain() -> Brain {
    Box::new(|m, seat, _, out| bot::think(m, seat, out))
}

/// Walks its own half of the lane: a click somewhere every second or two, now and then a stop, a
/// hold, or a stretch on the keys (WASD). Never fights of its own accord.
pub fn walker(seed: u64) -> Brain {
    let mut rng = Rng::new(seed);
    let mut next = 0u32;
    let mut steer = 0u8;
    Box::new(move |m, seat, k, out| {
        out.steer = steer;
        if k < next || m.outcome.is_some() {
            return;
        }
        next = k + 30 + rng.below(30);
        steer = 0;
        let team = team_of_seat(seat);
        match rng.below(10) {
            0 => {
                out.push(Order::Stop);
            }
            1 => {
                out.push(Order::Hold);
            }
            2 | 3 => steer = 1 + rng.below(8) as u8,
            _ => {
                let dawn = Vec2::new(
                    1_600.0 + rng.next_f32() * 3_200.0,
                    map::MID_Y + (rng.next_f32() - 0.5) * 900.0,
                );
                out.push(Order::Move { to: map::for_team(team, dawn) });
            }
        }
        out.steer = steer;
    })
}

pub struct Player {
    pub seat: usize,
    pub client: Client,
    up: Link,
    down: Link,
    brain: Brain,
    next_poll: f64,
    /// Seconds between polls (a browser frame).
    pub period: f64,
    pub corrections: Vec<Correction>,
    pub max_datagram: usize,
    pub sent_bytes: u64,
    pub up_bytes: u64,
    /// Orders this client gave, and the tick each was given for, in order.
    pub given: Vec<(u16, u32)>,
}

/// Called with every datagram as it leaves the server: the shard (at the tick it was made), the
/// seat, the bytes.
pub type Tap = Box<dyn FnMut(&Shard, usize, &[u8])>;

pub struct Rig {
    pub shard: Shard,
    pub shared: Arc<ShardShared>,
    leases: Vec<SeatLease>,
    egress: EgressEnds,
    records: RecordEnds,
    /// Match 0's record so far.
    pub record: Vec<u8>,
    pub players: Vec<Player>,
    pub now: f64,
    next_tick: f64,
    pub seed: u64,
    buf: Vec<u8>,
    pub tap: Option<Tap>,
    pub ticks: u64,
    /// The brains are let be: the clients keep sending, with nothing new to say.
    pub quiet: bool,
    /// Heap operations inside the shard's ticks (counted by `el_alloc`, when the test binary
    /// installs its allocator), and the ticks counted.
    pub heap_ops: u64,
}

impl Rig {
    /// Match 0 of a one-match shard, `seed`, with these seats played over links; bots on the server
    /// play the rest.
    pub fn new(seed: u64, humans: Vec<(usize, LinkSpec, Brain)>) -> Rig {
        let (shard, shared, leases, egress, records) =
            build(ShardConfig { matches: 1, map: map(), hot_guard: None });
        let mask = humans.iter().fold(0u16, |m, (s, _, _)| m | 1 << s);
        // A secret for the offers, as the live server's matches have.
        let secret = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        shared.control.push(Control::Start { m: 0, seed, picks: picks(seed), humans: mask, secret }).unwrap();
        let mut players = Vec::new();
        for (seat, spec, brain) in humans {
            shared.control.push(Control::Seat { m: 0, seat: seat as u8, max_datagram: 1_100 }).unwrap();
            players.push(Player {
                seat,
                client: client(seat, seed),
                up: Link::new(seed.wrapping_mul(31) + seat as u64, spec),
                down: Link::new(seed.wrapping_mul(17) + seat as u64 + 1_000, spec),
                brain,
                next_poll: 0.0,
                period: 1.0 / 60.0,
                corrections: Vec::new(),
                max_datagram: 0,
                sent_bytes: 0,
                up_bytes: 0,
                given: Vec::new(),
            });
        }
        Rig {
            shard,
            shared,
            leases,
            egress,
            records,
            record: Vec::new(),
            players,
            now: 0.0,
            next_tick: 0.0,
            seed,
            buf: vec![0; 2_048],
            tap: None,
            ticks: 0,
            quiet: false,
            heap_ops: 0,
        }
    }

    /// The match.
    pub fn m(&self) -> &Match {
        &self.shard.matches[0]
    }

    /// Runs `secs` of virtual time, a millisecond at a time.
    pub fn run(&mut self, secs: f64) {
        let end = self.now + secs;
        while self.now < end {
            self.step_ms();
        }
    }

    /// Quiets the brains and runs until every order given is acked (or `secs` pass).
    pub fn settle(&mut self, secs: f64) {
        self.quiet = true;
        self.run_until(secs, |r| r.players.iter().all(|p| p.client.orders.outstanding() == 0));
        self.quiet = false;
    }

    /// Runs until `done` says so (or `secs` pass).
    pub fn run_until(&mut self, secs: f64, mut done: impl FnMut(&Rig) -> bool) {
        let end = self.now + secs;
        while self.now < end && !done(self) {
            self.step_ms();
        }
    }

    fn step_ms(&mut self) {
        self.now += 0.001;
        let now = self.now;
        let now_us = (now * 1e6) as u64;
        // Client to server.
        for p in &mut self.players {
            for bytes in p.up.deliver(now) {
                let packet = InputPacket::decode(&bytes).expect("a packet a client encoded decodes");
                let _ = self.leases[p.seat].input.push(InputMsg { packet, recv_us: now_us });
            }
        }
        // The server's tick, and what it sends.
        if now >= self.next_tick {
            self.next_tick += 1.0 / 30.0;
            let shard = &mut self.shard;
            let ((), ops) = el_alloc::count(|| shard.tick_at(now_us));
            self.heap_ops += ops;
            self.ticks += 1;
            while let Ok(b) = self.records.rings[0].pop() {
                self.record.push(b);
            }
            for p in &mut self.players {
                while let Some(n) = read_packet(&mut self.egress.rings[p.seat], &mut self.buf) {
                    let bytes = self.buf[..n].to_vec();
                    p.max_datagram = p.max_datagram.max(n);
                    p.sent_bytes += n as u64;
                    if let Some(tap) = &mut self.tap {
                        tap(&self.shard, p.seat, &bytes);
                    }
                    p.down.send(now, bytes);
                }
            }
        }
        // Server to client; the clients' frames.
        for p in &mut self.players {
            for bytes in p.down.deliver(now) {
                p.client.on_datagram(&bytes, now).expect("every datagram decodes");
                if let Some(c) = p.client.correction.take() {
                    p.corrections.push(c);
                }
            }
            if now >= p.next_poll {
                p.next_poll += p.period;
                let seat = p.seat;
                let view = pos_code(p.client.pred.units[seat].pos);
                let before = p.client.orders.next_seq;
                let brain = &mut p.brain;
                let quiet = self.quiet;
                let packet = p.client.poll(now, view, &mut |m, k, out| {
                    if !quiet {
                        brain(m, seat, k, out)
                    }
                });
                for seq in before..p.client.orders.next_seq {
                    let o = p.client.orders.get(seq).expect("just given");
                    p.given.push((seq, o.tick));
                }
                for packet in packet.iter().flat_map(|ps| ps.iter()) {
                    let mut b = [0u8; 256];
                    let n = packet.encode(&mut b).expect("a packet fits");
                    p.up_bytes += n as u64;
                    p.up.send(now, b[..n].to_vec());
                }
            }
        }
    }
}

/// The `q`-quantile of `xs` (sorted in place).
pub fn quantile(xs: &mut [f32], q: f64) -> f32 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(f32::total_cmp);
    xs[((xs.len() - 1) as f64 * q).round() as usize]
}
