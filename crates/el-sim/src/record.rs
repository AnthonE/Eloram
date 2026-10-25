//! The record (`.elr`): a match as what it was given, tick by tick (`docs/NETCODE.md`, "The
//! record"). Its seed and champions, then every tick's applied input for all ten seats (bots' and
//! people's alike), and the state's hash every [`HASH_EVERY`] ticks. Stepping a fresh match through
//! it gives the match again, to the bit: that is a replay, a spectator's feed, and the referee's
//! check, which fails at the first hash that differs.
//!
//! A record is frames, `[u8 kind][u16 length][payload]`, so a live one can be cut at any frame and
//! read as far as it goes. Writing a frame allocates nothing ([`write_tick`] and the rest write into
//! a caller's buffer): the shard thread writes them as it steps.

use el_proto::{BitReader, BitWriter, DecodeError, Order};

use crate::game::{MAX_ORDERS_TICK, Match, SeatInput};
use crate::map::Map;
use crate::path::PathScratch;
use crate::units::SEATS;

/// The file's first bytes.
pub const MAGIC: [u8; 4] = *b"ELR1";
/// Bumped whenever the content (champions, items, augments, the map, the rules) changes what a
/// match does with the same inputs: a record names the content it was played on.
pub const CONTENT_VERSION: u32 = 1;
/// Ticks between the state's hashes.
pub const HASH_EVERY: u32 = 30;
/// The longest frame (a tick of ten seats, each with its most orders, fits with room to spare).
pub const MAX_FRAME: usize = 512;

/// A frame's kind.
pub mod kind {
    pub const HEADER: u8 = 1;
    pub const TICK: u8 = 2;
    pub const HASH: u8 = 3;
    pub const OUTCOME: u8 = 4;
    pub const SEAT: u8 = 5;
}

/// What a record is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub protocol: u16,
    pub content: u32,
    pub seed: u64,
    pub picks: [u8; SEATS],
}

impl Header {
    pub fn new(seed: u64, picks: [u8; SEATS]) -> Self {
        Self { protocol: el_proto::PROTOCOL_VERSION, content: CONTENT_VERSION, seed, picks }
    }
}

/// One frame, read. (A tick's frame is large; boxing it would allocate, and frames are read one at
/// a time anyway.)
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Frame {
    Header(Header),
    /// The inputs the match was stepped with at `tick`.
    Tick {
        tick: u32,
        inputs: [SeatInput; SEATS],
    },
    /// The state's hash once the match had stepped every tick before `tick`.
    Hash {
        tick: u32,
        hash: u64,
    },
    Outcome {
        tick: u32,
        winner: u8,
    },
    /// A person took the seat (`joined`) or left it to the bots, from `tick`.
    Seat {
        tick: u32,
        seat: u8,
        joined: bool,
    },
}

/// Writes a frame's prefix and the payload `body` writes; its length, or 0 if `out` is too small.
fn frame(out: &mut [u8], kind: u8, body: impl FnOnce(&mut BitWriter<'_>)) -> usize {
    if out.len() < 3 {
        return 0;
    }
    let (head, rest) = out.split_at_mut(3);
    let mut w = BitWriter::new(rest);
    body(&mut w);
    if w.overflowed() {
        return 0;
    }
    let n = w.bytes_written();
    head[0] = kind;
    head[1..3].copy_from_slice(&(n as u16).to_le_bytes());
    n + 3
}

fn write_u64(w: &mut BitWriter<'_>, v: u64) {
    w.write_u32(v as u32);
    w.write_u32((v >> 32) as u32);
}

fn read_u64(r: &mut BitReader<'_>) -> u64 {
    u64::from(r.read_u32()) | u64::from(r.read_u32()) << 32
}

pub fn write_header(out: &mut [u8], h: &Header) -> usize {
    frame(out, kind::HEADER, |w| {
        for b in MAGIC {
            w.write_u8(b);
        }
        w.write_u16(h.protocol);
        w.write_u32(h.content);
        write_u64(w, h.seed);
        for p in h.picks {
            w.write_u8(p);
        }
    })
}

/// The inputs of `tick`: a bit a seat for whether it did anything, then each one's steer and orders.
pub fn write_tick(out: &mut [u8], tick: u32, inputs: &[SeatInput; SEATS]) -> usize {
    frame(out, kind::TICK, |w| {
        w.write_u32(tick);
        let mut mask = 0u32;
        for (s, i) in inputs.iter().enumerate() {
            if i.steer != 0 || i.n > 0 {
                mask |= 1 << s;
            }
        }
        w.write_bits(mask, SEATS as u32);
        for i in inputs.iter().filter(|i| i.steer != 0 || i.n > 0) {
            w.write_bits(u32::from(i.steer), 4);
            w.write_bits(u32::from(i.n), 3);
            for o in i.orders() {
                o.write(w);
            }
        }
    })
}

pub fn write_hash(out: &mut [u8], tick: u32, hash: u64) -> usize {
    frame(out, kind::HASH, |w| {
        w.write_u32(tick);
        write_u64(w, hash);
    })
}

pub fn write_outcome(out: &mut [u8], tick: u32, winner: u8) -> usize {
    frame(out, kind::OUTCOME, |w| {
        w.write_u32(tick);
        w.write_u8(winner);
    })
}

pub fn write_seat(out: &mut [u8], tick: u32, seat: u8, joined: bool) -> usize {
    frame(out, kind::SEAT, |w| {
        w.write_u32(tick);
        w.write_u8(seat);
        w.write_bool(joined);
    })
}

/// Reads the frame at the start of `bytes`: it and its length, `Ok(None)` if `bytes` holds only
/// part of one (a live record cut mid-frame).
pub fn read_frame(bytes: &[u8]) -> Result<Option<(Frame, usize)>, DecodeError> {
    if bytes.len() < 3 {
        return Ok(None);
    }
    let len = usize::from(u16::from_le_bytes([bytes[1], bytes[2]]));
    if bytes.len() < 3 + len {
        return Ok(None);
    }
    let mut r = BitReader::new(&bytes[3..3 + len]);
    let f = match bytes[0] {
        kind::HEADER => {
            let magic = [r.read_u8(), r.read_u8(), r.read_u8(), r.read_u8()];
            if magic != MAGIC {
                return Err(DecodeError::WrongKind);
            }
            let protocol = r.read_u16();
            let content = r.read_u32();
            let seed = read_u64(&mut r);
            let mut picks = [0u8; SEATS];
            for p in &mut picks {
                *p = r.read_u8();
            }
            Frame::Header(Header { protocol, content, seed, picks })
        }
        kind::TICK => {
            let tick = r.read_u32();
            let mask = r.read_bits(SEATS as u32);
            let mut inputs = [SeatInput::default(); SEATS];
            for (s, input) in inputs.iter_mut().enumerate() {
                if mask & (1 << s) == 0 {
                    continue;
                }
                input.steer = r.read_bits(4) as u8;
                let n = r.read_bits(3) as usize;
                if n > MAX_ORDERS_TICK || input.steer > 8 {
                    return Err(DecodeError::Invalid);
                }
                for _ in 0..n {
                    input.push(Order::read(&mut r)?);
                }
            }
            Frame::Tick { tick, inputs }
        }
        kind::HASH => Frame::Hash { tick: r.read_u32(), hash: read_u64(&mut r) },
        kind::OUTCOME => Frame::Outcome { tick: r.read_u32(), winner: r.read_u8() },
        kind::SEAT => Frame::Seat { tick: r.read_u32(), seat: r.read_u8(), joined: r.read_bool() },
        _ => return Err(DecodeError::WrongKind),
    };
    if r.overflowed() {
        return Err(DecodeError::Truncated);
    }
    Ok(Some((f, 3 + len)))
}

/// The frames of a record, in order, as far as it goes whole.
pub struct Frames<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Where reading stopped on a frame that didn't decode.
    pub error: Option<(usize, DecodeError)>,
}

impl<'a> Frames<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0, error: None }
    }

    /// Bytes read so far.
    pub fn read(&self) -> usize {
        self.pos
    }
}

impl Iterator for Frames<'_> {
    type Item = Frame;

    fn next(&mut self) -> Option<Frame> {
        if self.error.is_some() {
            return None;
        }
        match read_frame(&self.bytes[self.pos..]) {
            Ok(Some((f, used))) => {
                self.pos += used;
                Some(f)
            }
            Ok(None) => None,
            Err(e) => {
                self.error = Some((self.pos, e));
                None
            }
        }
    }
}

/// Where a replay and the record part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The state's hash at `tick` isn't the one recorded: something between the last hash that
    /// matched and this one stepped differently.
    Hash { tick: u32, recorded: u64, replayed: u64 },
    /// A tick's inputs came for another tick than the match was at (the record skips or repeats).
    Gap { expected: u32, got: u32 },
    /// The record was made by another protocol or other content.
    Version { protocol: u16, content: u32 },
    /// A frame came before the header.
    NoHeader,
}

/// Steps a match through a record, frame by frame, and checks every hash on the way: a replay,
/// a spectator's feed, and the referee are all this.
pub struct Referee {
    pub m: Match,
    scratch: PathScratch,
    /// Hashes that matched.
    pub checked: u32,
    pub outcome: Option<(u32, u8)>,
    started: bool,
}

impl Referee {
    /// A referee for records of matches on `map` (its match is set up by the record's header).
    pub fn new(map: &'static Map) -> Self {
        Self {
            m: Match::new(map, 0, [0; SEATS]),
            scratch: PathScratch::new(),
            checked: 0,
            outcome: None,
            started: false,
        }
    }

    /// Takes the next frame.
    pub fn take(&mut self, f: &Frame) -> Result<(), Divergence> {
        match *f {
            Frame::Header(h) => {
                if h.protocol != el_proto::PROTOCOL_VERSION || h.content != CONTENT_VERSION {
                    return Err(Divergence::Version { protocol: h.protocol, content: h.content });
                }
                self.m.reset(h.seed, h.picks);
                self.checked = 0;
                self.outcome = None;
                self.started = true;
            }
            _ if !self.started => return Err(Divergence::NoHeader),
            Frame::Tick { tick, ref inputs } => {
                if tick != self.m.tick {
                    return Err(Divergence::Gap { expected: self.m.tick, got: tick });
                }
                self.m.step(inputs, &mut self.scratch);
            }
            Frame::Hash { tick, hash } => {
                if tick != self.m.tick {
                    return Err(Divergence::Gap { expected: self.m.tick, got: tick });
                }
                let replayed = self.m.hash();
                if replayed != hash {
                    return Err(Divergence::Hash { tick, recorded: hash, replayed });
                }
                self.checked += 1;
            }
            Frame::Outcome { tick, winner } => self.outcome = Some((tick, winner)),
            Frame::Seat { .. } => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::{Aim, UnitRef};
    use glam::Vec2;

    #[test]
    fn every_frame_reads_back_as_it_was_written() {
        let mut buf = [0u8; MAX_FRAME];
        let h = Header::new(0xDEAD_BEEF_0123_4567, [0, 1, 2, 3, 4, 4, 3, 2, 1, 0]);
        let n = write_header(&mut buf, &h);
        assert_eq!(read_frame(&buf[..n]).unwrap(), Some((Frame::Header(h), n)));

        // The most a tick can say: every seat steering and giving its most orders.
        let mut inputs = [SeatInput::default(); SEATS];
        for (s, i) in inputs.iter_mut().enumerate() {
            i.steer = (s % 9) as u8;
            for k in 0..MAX_ORDERS_TICK {
                let o = match k {
                    0 => Order::Move { to: Vec2::new(1_000.0 + s as f32, 2_000.0) }.quantized(),
                    1 => Order::Cast { slot: 3, aim: Aim::Unit(UnitRef { slot: 40, generation: 7 }) },
                    2 => Order::Cast { slot: 0, aim: Aim::Point(Vec2::new(5_000.0, 1_000.0)) }.quantized(),
                    _ => Order::Buy { item: 13 },
                };
                i.push(o);
            }
        }
        inputs[3] = SeatInput::default();
        let n = write_tick(&mut buf, 123_456, &inputs);
        assert!(n > 0 && n < MAX_FRAME, "{n} bytes");
        assert_eq!(read_frame(&buf[..n]).unwrap(), Some((Frame::Tick { tick: 123_456, inputs }, n)));
        // Cut anywhere, it's a frame still to come, never a wrong one.
        for cut in 0..n {
            assert_eq!(read_frame(&buf[..cut]).unwrap(), None);
        }
        let n = write_hash(&mut buf, 30, u64::MAX - 5);
        assert_eq!(read_frame(&buf[..n]).unwrap(), Some((Frame::Hash { tick: 30, hash: u64::MAX - 5 }, n)));
        let n = write_outcome(&mut buf, 99, 1);
        assert_eq!(read_frame(&buf[..n]).unwrap(), Some((Frame::Outcome { tick: 99, winner: 1 }, n)));
        let n = write_seat(&mut buf, 7, 9, true);
        assert_eq!(read_frame(&buf[..n]).unwrap(), Some((Frame::Seat { tick: 7, seat: 9, joined: true }, n)));
        // An idle tick: the prefix, the tick and a bit a seat.
        assert_eq!(write_tick(&mut buf, 5, &[SeatInput::default(); SEATS]), 9);
    }

    #[test]
    fn a_bad_frame_stops_the_reading() {
        let mut buf = [0u8; 64];
        let n = write_hash(&mut buf, 30, 1);
        buf[0] = 99;
        let mut frames = Frames::new(&buf[..n]);
        assert_eq!(frames.next(), None);
        assert_eq!(frames.error, Some((0, DecodeError::WrongKind)));
    }
}
