//! Client → server: the input packet. The last few per-tick commands (so one lost packet costs
//! nothing), the orders the server hasn't acked yet (so each arrives, and is applied once), and
//! the acks of the snapshots the client has.

use crate::order::Order;
use crate::quant::{POS_X_BITS, POS_Y_BITS};
use crate::{BitReader, BitWriter, DecodeError, PACKET_KIND_BITS, PacketKind};

/// Commands a packet carries: the newest and the ones before it.
pub const MAX_CMDS: usize = 4;
/// Unacked orders a packet carries, oldest first.
pub const MAX_ORDERS: usize = 8;
/// How far back an order's tick may be from the packet's newest command.
pub const ORDER_BACK_BITS: u32 = 6;

/// One tick of what the player holds. What they *decide* (move here, cast that) are [`Order`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputCmd {
    /// The tick this command is for.
    pub tick: u32,
    /// WASD movement: 0 none, 1 to 8 the eight directions, 1 along +x and counter-clockwise in
    /// steps of 45°. A state, never a press: a lost packet can't leave a key down.
    pub steer: u8,
    /// Where the camera looks, as position codes ([`crate::quant::pos_code`]): what the server
    /// sends first, never what the player is allowed to see. A packet carries the newest
    /// command's only, and every command it carries reads as looking there.
    pub view: [u32; 2],
}

impl InputCmd {
    /// What the server runs on a tick whose command never came: the last one's steer for a few
    /// ticks, then hands off (Gates' decay, Before Colony's `stand_in`). Orders are never repeated.
    pub fn stand_in(last: &InputCmd, tick: u32, missed: u32) -> InputCmd {
        const KEEP_STEER: u32 = 4;
        InputCmd { tick, steer: if missed < KEEP_STEER { last.steer } else { 0 }, view: last.view }
    }
}

/// An order with its sequence number and the tick the client stamped it for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StampedOrder {
    pub seq: u16,
    pub tick: u32,
    pub order: Order,
}

/// What a client sends each time it sends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputPacket {
    /// The snapshots it has: the stream, the newest and the 32 before it as bits (`ack`).
    pub ack_stream: u8,
    pub ack_seq: u16,
    pub ack_bits: u32,
    /// The client's clock when it sent this (ms, wrapping): echoed back for the round trip.
    pub client_time_ms: u16,
    /// Newest first: `cmds[i]` is for tick `cmds[0].tick - i`.
    pub cmds: [InputCmd; MAX_CMDS],
    pub n_cmds: u8,
    /// Oldest first, consecutive sequence numbers.
    pub orders: [StampedOrder; MAX_ORDERS],
    pub n_orders: u8,
}

impl InputPacket {
    pub fn cmds(&self) -> &[InputCmd] {
        &self.cmds[..usize::from(self.n_cmds)]
    }

    pub fn orders(&self) -> &[StampedOrder] {
        &self.orders[..usize::from(self.n_orders)]
    }

    /// Encodes into `buf`; returns the bytes written, or `None` if it didn't fit (or there was no
    /// command, or the orders aren't consecutive, or one is stamped too far back).
    pub fn encode(&self, buf: &mut [u8]) -> Option<usize> {
        let n = usize::from(self.n_cmds);
        if n == 0 || n > MAX_CMDS || usize::from(self.n_orders) > MAX_ORDERS {
            return None;
        }
        let newest = self.cmds[0].tick;
        let mut w = BitWriter::new(buf);
        w.write_bits(PacketKind::Input as u32, PACKET_KIND_BITS);
        w.write_bits(u32::from(self.ack_stream), 8);
        w.write_bits(u32::from(self.ack_seq), 16);
        w.write_u32(self.ack_bits);
        w.write_bits(u32::from(self.client_time_ms), 16);
        w.write_u32(newest);
        w.write_bits((n - 1) as u32, 2);
        w.write_bits(self.cmds[0].view[0], POS_X_BITS);
        w.write_bits(self.cmds[0].view[1], POS_Y_BITS);
        for (i, c) in self.cmds().iter().enumerate() {
            if c.tick != newest.wrapping_sub(i as u32) || c.steer > 8 {
                return None;
            }
            w.write_bits(u32::from(c.steer), 4);
        }
        let orders = self.orders();
        w.write_bits(u32::from(orders.first().map_or(0, |o| o.seq)), 16);
        w.write_bits(orders.len() as u32, 4);
        for (i, o) in orders.iter().enumerate() {
            let back = newest.wrapping_sub(o.tick);
            if o.seq != orders[0].seq.wrapping_add(i as u16) || back >= 1 << ORDER_BACK_BITS {
                return None;
            }
            w.write_bits(back, ORDER_BACK_BITS);
            o.order.write(&mut w);
        }
        if w.overflowed() { None } else { Some(w.bytes_written()) }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = BitReader::new(bytes);
        if r.read_bits(PACKET_KIND_BITS) != PacketKind::Input as u32 {
            return Err(DecodeError::WrongKind);
        }
        let mut p = InputPacket {
            ack_stream: r.read_bits(8) as u8,
            ack_seq: r.read_bits(16) as u16,
            ack_bits: r.read_u32(),
            client_time_ms: r.read_bits(16) as u16,
            ..InputPacket::default()
        };
        let newest = r.read_u32();
        let n = r.read_bits(2) as usize + 1;
        let view = [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)];
        for i in 0..n {
            let steer = r.read_bits(4) as u8;
            if steer > 8 {
                return Err(DecodeError::Invalid);
            }
            p.cmds[i] = InputCmd { tick: newest.wrapping_sub(i as u32), steer, view };
        }
        p.n_cmds = n as u8;
        let first_seq = r.read_bits(16) as u16;
        let n_orders = r.read_bits(4) as usize;
        if n_orders > MAX_ORDERS {
            return Err(DecodeError::Invalid);
        }
        for i in 0..n_orders {
            let back = r.read_bits(ORDER_BACK_BITS);
            let order = Order::read(&mut r)?;
            p.orders[i] = StampedOrder {
                seq: first_seq.wrapping_add(i as u16),
                tick: newest.wrapping_sub(back),
                order,
            };
        }
        p.n_orders = n_orders as u8;
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(p) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{Aim, UnitRef, slot};
    use crate::quant::pos_code;
    use glam::Vec2;

    fn packet() -> InputPacket {
        let view = pos_code(Vec2::new(7_000.0, 1_900.0));
        let mut p = InputPacket {
            ack_stream: 3,
            ack_seq: 0xBEEF,
            ack_bits: 0xF0F0_0F0F,
            client_time_ms: 4321,
            n_cmds: 3,
            n_orders: 3,
            ..InputPacket::default()
        };
        p.cmds[0] = InputCmd { tick: 1000, steer: 3, view };
        p.cmds[1] = InputCmd { tick: 999, steer: 3, view };
        p.cmds[2] = InputCmd { tick: 998, steer: 0, view };
        p.orders[0] = StampedOrder {
            seq: 65_535,
            tick: 990,
            order: Order::Move { to: Vec2::new(6_000.0, 2_000.0) }.quantized(),
        };
        p.orders[1] = StampedOrder {
            seq: 0,
            tick: 999,
            order: Order::Cast { slot: slot::Q, aim: Aim::Unit(UnitRef { slot: 4, generation: 9 }) },
        };
        p.orders[2] = StampedOrder { seq: 1, tick: 1000, order: Order::Stop };
        p
    }

    #[test]
    fn a_packet_round_trips_and_fits_easily() {
        let p = packet();
        let mut buf = [0u8; 256];
        let n = p.encode(&mut buf).unwrap();
        assert!(n < 48, "{n} bytes");
        assert_eq!(InputPacket::decode(&buf[..n]).unwrap(), p);
        // The newest command's view is every command's.
        let mut q = p;
        q.cmds[2].view = pos_code(Vec2::new(10.0, 20.0));
        let n = q.encode(&mut buf).unwrap();
        assert_eq!(InputPacket::decode(&buf[..n]).unwrap(), p);
    }

    #[test]
    fn a_packet_that_lies_is_refused() {
        let mut p = packet();
        p.orders[2].seq = 7;
        assert_eq!(p.encode(&mut [0u8; 256]), None, "orders not consecutive");
        let mut p = packet();
        p.orders[0].tick = 900;
        assert_eq!(p.encode(&mut [0u8; 256]), None, "an order stamped too far back");
        let mut buf = [0u8; 256];
        let n = packet().encode(&mut buf).unwrap();
        for cut in 0..n {
            assert!(InputPacket::decode(&buf[..cut]).is_err(), "cut at {cut}");
        }
        buf[0] = 0x02;
        assert_eq!(InputPacket::decode(&buf[..n]), Err(DecodeError::WrongKind));
    }

    #[test]
    fn a_stand_in_keeps_the_steer_a_little_then_lets_go() {
        let last = InputCmd { tick: 10, steer: 5, view: [1, 2] };
        assert_eq!(InputCmd::stand_in(&last, 11, 0).steer, 5);
        assert_eq!(InputCmd::stand_in(&last, 15, 4).steer, 0);
    }
}
