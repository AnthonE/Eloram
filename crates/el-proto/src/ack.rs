//! Which snapshot datagrams a client has, as its input packets tell the server.
//!
//! Every snapshot datagram carries a sequence number counted per seat by the match that sends it,
//! and a stream: a new one each time the client is seated (a match, a reconnect), so a count
//! started again is never taken for the old one. A client takes a datagram only if it is newer than
//! every one it took on that stream, or on another stream and of a newer tick: so what it took on a
//! stream is taken in order, and the server can trust that nothing it sent before a datagram the
//! client acked arrives after it. Each input packet acks the newest it took and the 32 before it,
//! as bits (Gaffer On Games' reliable-ordered header; Gates' `ack_bits`; Before Colony's `ack`),
//! so one packet lost costs nothing.

/// The highest stream; they go round from 1 to this (0 is no stream).
pub const MAX_STREAM: u8 = u8::MAX;

/// The stream after `stream`: a client seated again is never on the stream it had.
pub fn next_stream(stream: u8) -> u8 {
    if stream == MAX_STREAM { 1 } else { stream + 1 }
}

/// The full sequence number nearest `near` whose low 16 bits are `seq`.
pub fn expand(seq: u16, near: u32) -> u32 {
    let candidate = (near & !0xFFFF) | u32::from(seq);
    if candidate >= near.saturating_add(0x8000) && candidate >= 0x1_0000 {
        candidate - 0x1_0000
    } else if candidate.saturating_add(0x8000) < near {
        candidate.saturating_add(0x1_0000)
    } else {
        candidate
    }
}

/// What a client makes of a datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Older than (or the same as) one it has: dropped, and not acked.
    Stale,
    /// News on the stream in hand, with its full sequence number.
    Next(u32),
    /// The first of another stream (a new seat): what was held of the last is to be forgotten.
    NewStream(u32),
    /// A datagram with no stream (0): taken by its tick, never acked.
    Unsequenced,
}

/// The client's side: the stream in hand, the newest datagram taken on it and the 63 before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AckWindow {
    stream: u8,
    newest: u32,
    /// Bit `i`: datagram `newest - i` was taken (bit 0 is `newest` itself).
    mask: u64,
    /// The newest tick of any datagram taken.
    tick: u32,
    any: bool,
}

impl AckWindow {
    /// Whether the datagram `seq` of `stream`, of the snapshot of `tick`, is to be taken.
    pub fn check(&self, stream: u8, seq: u16, tick: u32) -> Verdict {
        let newer_tick = !self.any || tick > self.tick;
        if stream == 0 {
            return if newer_tick { Verdict::Unsequenced } else { Verdict::Stale };
        }
        if stream == self.stream && self.mask != 0 {
            let s = expand(seq, self.newest);
            return if s > self.newest { Verdict::Next(s) } else { Verdict::Stale };
        }
        if newer_tick { Verdict::NewStream(u32::from(seq)) } else { Verdict::Stale }
    }

    /// Takes a datagram [`check`](Self::check) let through.
    pub fn commit(&mut self, stream: u8, verdict: Verdict, tick: u32) {
        match verdict {
            Verdict::Stale => return,
            Verdict::Unsequenced => {}
            Verdict::NewStream(s) => {
                self.stream = stream;
                self.newest = s;
                self.mask = 1;
            }
            Verdict::Next(s) => {
                let gap = s - self.newest;
                self.mask = if gap >= 64 { 1 } else { self.mask << gap | 1 };
                self.newest = s;
            }
        }
        self.tick = self.tick.max(tick);
        self.any = true;
    }

    /// The ack an input packet carries: the stream, the newest datagram taken on it, and bit `i`
    /// set if `newest - 1 - i` was taken too. All zero before anything was taken on a stream.
    pub fn ack(&self) -> (u8, u16, u32) {
        if self.mask == 0 {
            return (0, 0, 0);
        }
        (self.stream, self.newest as u16, (self.mask >> 1) as u32)
    }

    /// The newest tick of a datagram taken.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// The stream in hand (0 before any).
    pub fn stream(&self) -> u8 {
        self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_seat_never_keeps_its_stream() {
        let mut s = next_stream(0);
        assert_eq!(s, 1);
        for _ in 0..600 {
            let next = next_stream(s);
            assert_ne!(next, s);
            assert_ne!(next, 0);
            s = next;
        }
    }

    #[test]
    fn sequence_numbers_expand_across_the_wrap() {
        assert_eq!(expand(5, 3), 5);
        assert_eq!(expand(0xFFFF, 0x1_0002), 0xFFFF);
        assert_eq!(expand(2, 0xFFFE), 0x1_0002);
        assert_eq!(expand(0x8000, 0x1_0000), 0x8000);
        assert_eq!(expand(7, 0), 7);
        assert_eq!(expand(0xFFF0, 5), 0xFFF0);
    }

    #[test]
    fn a_stream_is_taken_in_order_and_acked_as_bits() {
        let mut a = AckWindow::default();
        assert_eq!(a.ack(), (0, 0, 0));
        let v = a.check(9, 0, 100);
        assert_eq!(v, Verdict::NewStream(0));
        a.commit(9, v, 100);
        for (seq, tick) in [(1, 101), (3, 102), (4, 102)] {
            let v = a.check(9, seq, tick);
            assert_eq!(v, Verdict::Next(u32::from(seq)));
            a.commit(9, v, tick);
        }
        // 2 came late: after 3 it is stale, and so is a repeat.
        assert_eq!(a.check(9, 2, 101), Verdict::Stale);
        assert_eq!(a.check(9, 4, 102), Verdict::Stale);
        // 4 newest; 3 (bit 0), 1 (bit 2) and 0 (bit 3) before it.
        assert_eq!(a.ack(), (9, 4, 0b1101));
    }

    #[test]
    fn a_new_stream_starts_over_only_with_a_newer_tick() {
        let mut a = AckWindow::default();
        let v = a.check(9, 40, 100);
        a.commit(9, v, 100);
        // The old seat's datagram of the same tick doesn't take over.
        assert_eq!(a.check(70, 0, 100), Verdict::Stale);
        let v = a.check(70, 0, 101);
        assert_eq!(v, Verdict::NewStream(0));
        a.commit(70, v, 101);
        assert_eq!(a.ack(), (70, 0, 0));
        // The old stream is stale now, unless it is newer still.
        assert_eq!(a.check(9, 41, 101), Verdict::Stale);
        assert_eq!(a.check(9, 41, 102), Verdict::NewStream(41));
    }

    #[test]
    fn unsequenced_datagrams_go_by_their_tick_and_ack_nothing() {
        let mut a = AckWindow::default();
        let v = a.check(0, 0, 41);
        assert_eq!(v, Verdict::Unsequenced);
        a.commit(0, v, 41);
        assert_eq!(a.check(0, 0, 41), Verdict::Stale);
        assert_eq!(a.check(0, 0, 90), Verdict::Unsequenced);
        assert_eq!(a.ack(), (0, 0, 0));
    }

    #[test]
    fn a_long_gap_keeps_only_the_newest() {
        let mut a = AckWindow::default();
        let v = a.check(3, 10, 1);
        a.commit(3, v, 1);
        let v = a.check(3, 200, 2);
        a.commit(3, v, 2);
        assert_eq!(a.ack(), (3, 200, 0));
        // Across the 16-bit wrap.
        let mut a = AckWindow::default();
        let v = a.check(3, 0xFFFE, 1);
        a.commit(3, v, 1);
        let v = a.check(3, 1, 2);
        assert_eq!(v, Verdict::Next(0x1_0001));
        a.commit(3, v, 2);
        assert_eq!(a.ack(), (3, 1, 0b100));
    }
}
