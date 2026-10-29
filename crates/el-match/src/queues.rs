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
    /// `practice`: a practice match (`el_sim::practice`), its record saying so.
    Start { m: u16, seed: u64, picks: [u8; SEATS], humans: u16, secret: u64, practice: bool },
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
    /// Match `m` panicked at `tick`: it was stopped where it stood, the rest of the shard going on.
    Failed { m: u16, tick: u32 },
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
/// them, and the shard's checkpoints (`[u16 match][u32 len][el_sim::snapshot]`), for the recorder
/// and its referee.
pub struct RecordEnds {
    pub rings: Vec<rtrb::Consumer<u8>>,
    pub checkpoints: rtrb::Consumer<u8>,
}

/// The shard thread's ends.
pub(crate) struct ShardEnds {
    pub inputs: Vec<rtrb::Consumer<InputMsg>>,
    pub outputs: Vec<rtrb::Producer<u8>>,
    pub records: Vec<rtrb::Producer<u8>>,
    pub checkpoints: rtrb::Producer<u8>,
}

/// Bytes of a seat's outbound queue (about thirty full datagrams).
pub const OUT_RING_BYTES: usize = 32 * 1024;
/// Input messages a seat buffers.
pub const IN_RING: usize = 64;
/// Bytes of a match's record ring: a few seconds of its busiest ticks.
pub const RECORD_RING_BYTES: usize = 64 * 1024;

/// Ticks between a match's checkpoints, unless a shard is told otherwise: a minute
/// (`docs/NETCODE.md`, "The record").
pub const CHECKPOINT_EVERY: u32 = 60 * el_sim::TICK_HZ;
const _: () = assert!(CHECKPOINT_EVERY.is_multiple_of(el_sim::record::HASH_EVERY));

/// `secs` between checkpoints, as ticks: a whole number of hashes, at least one (0: none).
pub fn checkpoint_ticks(secs: f64) -> u32 {
    if secs <= 0.0 {
        return 0;
    }
    let h = el_sim::record::HASH_EVERY;
    ((secs * f64::from(el_sim::TICK_HZ) / f64::from(h)).round() as u32).max(1) * h
}
/// Checkpoints a shard's ring holds: matches' checkpoints are a hash apart, so the recorder
/// (every tenth of a second) takes each long before the next.
const CHECKPOINTS_HELD: usize = 3;
/// A checkpoint frame's head: the match, and the snapshot's length.
pub const CHECKPOINT_HEAD: usize = 6;

/// Whether match `m` takes a checkpoint at `tick` (just after that tick's hash), one every
/// `every` ticks (a whole number of hashes; 0: never), each match a hash later than the one
/// before it, so a shard writes one a tick at most. Never at tick 0 (the header is the start).
pub fn checkpoint_due(m: usize, tick: u32, every: u32) -> bool {
    if every == 0 {
        return false;
    }
    let at = (m as u32 * el_sim::record::HASH_EVERY) % every;
    tick > 0 && tick % every == at
}

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
    let probe = el_sim::Match::new(cfg.map, 0, [0; SEATS]);
    let (checkpoints, checkpoint_ring) =
        rtrb::RingBuffer::<u8>::new(CHECKPOINTS_HELD * (CHECKPOINT_HEAD + el_sim::snapshot::len(&probe)));
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
    let shard = crate::Shard::new(cfg, shared.clone(), ShardEnds { inputs, outputs, records, checkpoints });
    let record_ends = RecordEnds { rings: record_rings, checkpoints: checkpoint_ring };
    (shard, shared, leases, EgressEnds { rings }, record_ends)
}

/// Puts one `[u16 len][payload]` frame on an egress ring in a single commit, so the egress thread
/// never sees a length without its payload. False (nothing written) if it doesn't fit.
pub fn write_packet(ring: &mut rtrb::Producer<u8>, payload: &[u8]) -> bool {
    let Ok(len) = u16::try_from(payload.len()) else { return false };
    let Ok(mut chunk) = ring.write_chunk(payload.len() + 2) else { return false };
    let (one, two) = chunk.as_mut_slices();
    for (dst, src) in one.iter_mut().chain(two.iter_mut()).zip(len.to_le_bytes().iter().chain(payload)) {
        *dst = *src;
    }
    chunk.commit_all();
    true
}

/// Reads one `[u16 len][payload]` frame from an egress ring into `buf`. Returns the payload length,
/// or `None` until a whole frame is there: a frame is only taken once all of it can be.
pub fn read_packet(ring: &mut rtrb::Consumer<u8>, buf: &mut [u8]) -> Option<usize> {
    loop {
        let n = {
            let chunk = ring.read_chunk(2).ok()?;
            let (a, b) = chunk.as_slices();
            let mut len = [0u8; 2];
            for (dst, src) in len.iter_mut().zip(a.iter().chain(b)) {
                *dst = *src;
            }
            u16::from_le_bytes(len) as usize
        };
        if ring.slots() < n + 2 {
            return None;
        }
        let mut len = [0u8; 2];
        ring.pop_entire_slice(&mut len).ok()?;
        if n <= buf.len() {
            ring.pop_entire_slice(&mut buf[..n]).ok()?;
            return Some(n);
        }
        // Bigger than any datagram: can't be written, but never panic or lose the framing on the
        // egress path; skip it whole.
        ring.read_chunk(n).ok()?.commit_all();
    }
}

/// Puts match `m`'s checkpoint on the ring in one commit, or nothing if it doesn't fit.
pub fn write_checkpoint(ring: &mut rtrb::Producer<u8>, m: u16, snapshot: &[u8]) -> bool {
    let Ok(len) = u32::try_from(snapshot.len()) else { return false };
    let Ok(mut chunk) = ring.write_chunk(CHECKPOINT_HEAD + snapshot.len()) else { return false };
    let mut head = [0u8; CHECKPOINT_HEAD];
    head[..2].copy_from_slice(&m.to_le_bytes());
    head[2..].copy_from_slice(&len.to_le_bytes());
    let (one, two) = chunk.as_mut_slices();
    for (dst, src) in one.iter_mut().chain(two.iter_mut()).zip(head.iter().chain(snapshot)) {
        *dst = *src;
    }
    chunk.commit_all();
    true
}

/// Takes the next whole checkpoint off the ring into `buf`: its match and its length, or `None`
/// until one is all there. One longer than `buf` is passed over.
pub fn read_checkpoint(ring: &mut rtrb::Consumer<u8>, buf: &mut [u8]) -> Option<(u16, usize)> {
    loop {
        let (m, n) = {
            let chunk = ring.read_chunk(CHECKPOINT_HEAD).ok()?;
            let (a, b) = chunk.as_slices();
            let mut head = [0u8; CHECKPOINT_HEAD];
            for (dst, src) in head.iter_mut().zip(a.iter().chain(b)) {
                *dst = *src;
            }
            (
                u16::from_le_bytes([head[0], head[1]]),
                u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize,
            )
        };
        if ring.slots() < CHECKPOINT_HEAD + n {
            return None;
        }
        ring.read_chunk(CHECKPOINT_HEAD).ok()?.commit_all();
        if n <= buf.len() {
            ring.pop_entire_slice(&mut buf[..n]).ok()?;
            return Some((m, n));
        }
        ring.read_chunk(n).ok()?.commit_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_read_only_once_all_of_it_is_there() {
        let (mut p, mut c) = rtrb::RingBuffer::<u8>::new(64);
        let mut buf = [0u8; 32];
        // Half a frame (as a writer committing in two steps would leave it): nothing is taken.
        p.push_entire_slice(&5u16.to_le_bytes()).unwrap();
        assert_eq!(read_packet(&mut c, &mut buf), None);
        p.push_entire_slice(&[1, 2, 3]).unwrap();
        assert_eq!(read_packet(&mut c, &mut buf), None);
        p.push_entire_slice(&[4, 5]).unwrap();
        assert_eq!(read_packet(&mut c, &mut buf), Some(5));
        assert_eq!(&buf[..5], &[1, 2, 3, 4, 5]);
        // Frames across the ring's end keep their framing.
        for k in 0..40u8 {
            assert!(write_packet(&mut p, &[k; 7]));
            assert_eq!(read_packet(&mut c, &mut buf), Some(7));
            assert_eq!(&buf[..7], &[k; 7]);
        }
        // One too big for the reader is skipped, and the next still reads.
        assert!(write_packet(&mut p, &[9; 40]));
        assert!(write_packet(&mut p, &[8; 3]));
        assert_eq!(read_packet(&mut c, &mut buf), Some(3));
        assert_eq!(&buf[..3], &[8; 3]);
        // One that doesn't fit isn't written at all.
        assert!(!write_packet(&mut p, &[0; 63]));
        assert_eq!(read_packet(&mut c, &mut buf), None);
    }

    #[test]
    fn a_reader_racing_the_writer_never_loses_the_framing() {
        let (mut p, mut c) = rtrb::RingBuffer::<u8>::new(256);
        let writer = std::thread::spawn(move || {
            let mut k = 0u32;
            while k < 200_000 {
                let len = 1 + (k % 50) as usize;
                let mut frame = [0u8; 64];
                frame[..4].copy_from_slice(&k.to_le_bytes());
                if write_packet(&mut p, &frame[..len.max(4)]) {
                    k += 1;
                }
            }
        });
        let mut buf = [0u8; 64];
        let mut want = 0u32;
        while want < 200_000 {
            if let Some(n) = read_packet(&mut c, &mut buf) {
                assert_eq!(n, (1 + (want % 50) as usize).max(4));
                assert_eq!(u32::from_le_bytes(buf[..4].try_into().unwrap()), want);
                want += 1;
            }
        }
        writer.join().unwrap();
    }
}
