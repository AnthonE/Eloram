//! The lock-free plumbing between a shard thread and the network side, allocated once.
//!
//! A seat is `match × 10 + seat` across the shard. Its session holds a [`SeatLease`] (the producer
//! end of the seat's input ring); the egress thread holds the consumer end of its output ring;
//! the shard thread holds the other ends. Control goes in on one `ArrayQueue`, results come out
//! on another. Nothing here locks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;
use el_proto::InputPacket;
use el_sim::units::SEATS;

use crate::metrics::Metrics;

/// An input datagram, decoded on the network side.
#[derive(Clone, Copy, Debug)]
pub struct InputMsg {
    pub packet: InputPacket,
    /// The shard's clock (µs since it started) when it arrived: lets a snapshot say how long the
    /// server held the client's timestamp.
    pub recv_us: u64,
}

/// Network → shard.
#[derive(Clone, Copy, Debug)]
pub enum Control {
    /// Starts match `m` over with `seed` and these champions. Seats in `humans` (a bit a seat)
    /// wait for their players; the rest are bots. `secret` draws its augment offers (0: the
    /// seed's), and goes nowhere else: the record gets each key as its screen opens.
    Start { m: u16, seed: u64, picks: [u8; SEATS], humans: u16, secret: u64 },
    /// A player takes (or takes back) their seat: a new stream, everything resent whole.
    Seat { m: u16, seat: u8, max_datagram: u16 },
    /// A player left their seat: a bot plays it until they're back.
    Unseat { m: u16, seat: u8 },
    /// Match `m` is done with: its place is free.
    End { m: u16 },
}

/// Shard → network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Match `m` ended: `winner` won, at `tick`; `hash` is its state then.
    Over { m: u16, winner: u8, tick: u32, hash: u64 },
}

/// The right to send one seat's inputs: owned by exactly one session at a time.
pub struct SeatLease {
    /// The seat across the shard (`match × 10 + seat`).
    pub g: u32,
    pub input: rtrb::Producer<InputMsg>,
}

/// A match's state, as the network side sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MatchState {
    Free = 0,
    Running = 1,
    Over = 2,
}

pub struct ShardShared {
    pub control: ArrayQueue<Control>,
    pub outcomes: ArrayQueue<Outcome>,
    pub metrics: Metrics,
    /// Each match's tick, and state ([`MatchState`]).
    pub ticks: Box<[AtomicU32]>,
    pub states: Box<[AtomicU32]>,
    /// The shard's own tick count.
    pub tick: AtomicU64,
    pub stop: AtomicBool,
    pub matches: usize,
    started: std::time::Instant,
}

impl ShardShared {
    /// Microseconds since the shard was built (the clock the round trip's hold is measured on).
    pub fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    pub fn state(&self, m: usize) -> MatchState {
        match self.states[m].load(Ordering::Acquire) {
            1 => MatchState::Running,
            2 => MatchState::Over,
            _ => MatchState::Free,
        }
    }
}

/// Egress ends: one byte ring a seat, `[u16 len][datagram]` frames, for the egress thread.
pub struct EgressEnds {
    pub rings: Vec<rtrb::Consumer<u8>>,
}

/// Record ends: one byte ring a match, its record's frames (`el_sim::record`) as the shard writes
/// them, for the recorder.
pub struct RecordEnds {
    pub rings: Vec<rtrb::Consumer<u8>>,
}

/// The shard thread's ends.
pub(crate) struct ShardEnds {
    pub inputs: Vec<rtrb::Consumer<InputMsg>>,
    pub outputs: Vec<rtrb::Producer<u8>>,
    pub records: Vec<rtrb::Producer<u8>>,
}

/// Bytes of a seat's outbound queue (about thirty full datagrams).
pub const OUT_RING_BYTES: usize = 32 * 1024;
/// Input messages a seat buffers.
pub const IN_RING: usize = 64;
/// Bytes of a match's record ring: a few seconds of its busiest ticks.
pub const RECORD_RING_BYTES: usize = 64 * 1024;

/// Allocates every queue of a shard of `matches` matches: the shard itself, its shared state, the
/// seats' leases (index `match × 10 + seat`), the egress ends and the record ends. The only place the runtime
/// allocates.
#[allow(clippy::disallowed_methods, clippy::disallowed_macros)]
pub fn build(
    cfg: crate::ShardConfig,
) -> (crate::Shard, Arc<ShardShared>, Vec<SeatLease>, EgressEnds, RecordEnds) {
    let seats = cfg.matches * SEATS;
    let mut inputs = Vec::with_capacity(seats);
    let mut outputs = Vec::with_capacity(seats);
    let mut leases = Vec::with_capacity(seats);
    let mut rings = Vec::with_capacity(seats);
    for g in 0..seats {
        let (p, c) = rtrb::RingBuffer::<InputMsg>::new(IN_RING);
        inputs.push(c);
        leases.push(SeatLease { g: g as u32, input: p });
        let (op, oc) = rtrb::RingBuffer::<u8>::new(OUT_RING_BYTES);
        outputs.push(op);
        rings.push(oc);
    }
    let (records, record_rings): (Vec<_>, Vec<_>) =
        (0..cfg.matches).map(|_| rtrb::RingBuffer::<u8>::new(RECORD_RING_BYTES)).unzip();
    let shared = Arc::new(ShardShared {
        control: ArrayQueue::new(cfg.matches * 16 + 16),
        outcomes: ArrayQueue::new(cfg.matches + 16),
        metrics: Metrics::default(),
        ticks: (0..cfg.matches).map(|_| AtomicU32::new(0)).collect(),
        states: (0..cfg.matches).map(|_| AtomicU32::new(MatchState::Free as u32)).collect(),
        tick: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        matches: cfg.matches,
        started: std::time::Instant::now(),
    });
    let shard = crate::Shard::new(cfg, shared.clone(), ShardEnds { inputs, outputs, records });
    (shard, shared, leases, EgressEnds { rings }, RecordEnds { rings: record_rings })
}

/// Reads one `[u16 len][payload]` frame from an egress ring into `buf`. Returns the payload length.
pub fn read_packet(ring: &mut rtrb::Consumer<u8>, buf: &mut [u8]) -> Option<usize> {
    if ring.slots() < 2 {
        return None;
    }
    let mut len = [0u8; 2];
    ring.pop_entire_slice(&mut len).ok()?;
    let n = u16::from_le_bytes(len) as usize;
    if n > buf.len() || ring.pop_entire_slice(&mut buf[..n]).is_err() {
        // Corrupt framing can't happen with one producer, but never panic on the egress path.
        return None;
    }
    Some(n)
}
