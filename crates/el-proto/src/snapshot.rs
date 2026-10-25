//! Server → client: a snapshot datagram. Its header (the tick, its place in the stream, what the
//! server has of the client's inputs and orders, the round trip's echo), then the owner's own
//! champion at full precision (`el_sim::own`), its events, and the units its team sees, nearest
//! and most changed first, as many as fit ([`MAX_DATAGRAM`](crate::MAX_DATAGRAM)).

use crate::{BitReader, BitWriter, DecodeError, PACKET_KIND_BITS, PacketKind};

/// [`SnapshotHeader::flags`].
pub mod snap_flags {
    /// Another datagram of the same tick follows (more units than one holds).
    pub const MORE: u32 = 1 << 0;
    /// This datagram carries the owner's own champion.
    pub const OWN: u32 = 1 << 1;
    /// The match is over.
    pub const OVER: u32 = 1 << 2;
}

pub const FLAG_BITS: u32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotHeader {
    /// The tick the state is at: the server has stepped every tick before it.
    pub tick: u32,
    /// The datagram's sequence number on its stream (`crate::ack`).
    pub seq: u16,
    pub stream: u8,
    /// The next order sequence number the server expects: every order before it has been applied.
    pub order_ack: u16,
    /// Commands the server holds for this seat at and after `tick` (Gates' `buffered_depth`): what
    /// the client's clock steers on, aiming at two.
    pub depth: u8,
    /// Ticks of the last thirty the server stood in for a command that didn't come in time.
    pub missed: u8,
    /// The newest input packet's `client_time_ms`, and how long the server held it before this
    /// datagram left (ms): the round trip is the echo's age less the hold.
    pub time_echo: u16,
    pub hold_ms: u16,
    pub flags: u32,
}

pub const DEPTH_BITS: u32 = 6;
pub const MISSED_BITS: u32 = 5;
pub const HOLD_BITS: u32 = 10;

impl SnapshotHeader {
    pub const BITS: usize =
        (PACKET_KIND_BITS + 32 + 16 + 8 + 16 + DEPTH_BITS + MISSED_BITS + 16 + HOLD_BITS + FLAG_BITS)
            as usize;

    pub fn write(&self, w: &mut BitWriter<'_>) {
        w.write_bits(PacketKind::Snapshot as u32, PACKET_KIND_BITS);
        w.write_u32(self.tick);
        w.write_u16(self.seq);
        w.write_u8(self.stream);
        w.write_u16(self.order_ack);
        w.write_bits(u32::from(self.depth).min((1 << DEPTH_BITS) - 1), DEPTH_BITS);
        w.write_bits(u32::from(self.missed).min((1 << MISSED_BITS) - 1), MISSED_BITS);
        w.write_u16(self.time_echo);
        w.write_bits(u32::from(self.hold_ms).min((1 << HOLD_BITS) - 1), HOLD_BITS);
        w.write_bits(self.flags, FLAG_BITS);
    }

    pub fn read(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        if r.read_bits(PACKET_KIND_BITS) != PacketKind::Snapshot as u32 {
            return Err(DecodeError::WrongKind);
        }
        let h = SnapshotHeader {
            tick: r.read_u32(),
            seq: r.read_u16(),
            stream: r.read_u8(),
            order_ack: r.read_u16(),
            depth: r.read_bits(DEPTH_BITS) as u8,
            missed: r.read_bits(MISSED_BITS) as u8,
            time_echo: r.read_u16(),
            hold_ms: r.read_bits(HOLD_BITS) as u16,
            flags: r.read_bits(FLAG_BITS),
        };
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(h) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_round_trips_in_its_bits() {
        let h = SnapshotHeader {
            tick: 123_456,
            seq: 65_000,
            stream: 9,
            order_ack: 77,
            depth: 2,
            missed: 3,
            time_echo: 4_321,
            hold_ms: 12,
            flags: snap_flags::OWN | snap_flags::MORE,
        };
        let mut buf = [0u8; 32];
        let mut w = BitWriter::new(&mut buf);
        h.write(&mut w);
        assert_eq!(w.bits_written(), SnapshotHeader::BITS);
        assert_eq!(SnapshotHeader::read(&mut BitReader::new(&buf)).unwrap(), h);
    }
}
