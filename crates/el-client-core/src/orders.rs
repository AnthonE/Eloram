//! The orders this client gave, each with its sequence number and the tick it was given for. Every
//! input packet repeats the ones the server hasn't acked yet (each snapshot says how far it has
//! applied them), and the server applies each exactly once (`docs/NETCODE.md`, "Inputs").

use el_proto::input::{MAX_ORDERS, ORDER_BACK_BITS};
use el_proto::{InputPacket, Order, StampedOrder};

/// Orders that may be outstanding. A client this far ahead of the server's acks is cut off: the
/// order is refused, not queued.
pub const OUTSTANDING: usize = 64;

#[derive(Clone, Debug)]
pub struct Orders {
    ring: [StampedOrder; OUTSTANDING],
    /// The next order's sequence number.
    pub next_seq: u16,
    /// The next the server expects: every one before it has been applied.
    pub acked: u16,
    /// Orders given, and refused for being too many outstanding.
    pub given: u64,
    pub refused: u64,
}

impl Default for Orders {
    fn default() -> Self {
        Self { ring: [StampedOrder::default(); OUTSTANDING], next_seq: 0, acked: 0, given: 0, refused: 0 }
    }
}

impl Orders {
    /// Forgets them all (a new seat: the server counts from 0 again).
    pub fn clear(&mut self) {
        *self = Orders { given: self.given, refused: self.refused, ..Orders::default() };
    }

    /// Gives `order` for `tick`; its sequence number, or `None` if too many are outstanding.
    pub fn give(&mut self, order: Order, tick: u32) -> Option<u16> {
        if self.outstanding() >= OUTSTANDING {
            self.refused += 1;
            return None;
        }
        let seq = self.next_seq;
        self.ring[usize::from(seq) % OUTSTANDING] = StampedOrder { seq, tick, order: order.quantized() };
        self.next_seq = seq.wrapping_add(1);
        self.given += 1;
        Some(seq)
    }

    /// Stamps every outstanding order from `from` on (none of them sent yet) for `tick` at the
    /// earliest: after the client's clock jumped, they're for the ticks it's on now.
    pub fn restamp(&mut self, from: u16, tick: u32) {
        let mut seq = from;
        while seq != self.next_seq && usize::from(seq.wrapping_sub(self.acked)) < self.outstanding() {
            let o = &mut self.ring[usize::from(seq) % OUTSTANDING];
            o.tick = o.tick.max(tick);
            seq = seq.wrapping_add(1);
        }
    }

    /// A snapshot says the server expects `next` now.
    pub fn on_ack(&mut self, next: u16) {
        // Only forward, and never past what was given.
        let ahead = next.wrapping_sub(self.acked);
        if ahead != 0 && usize::from(ahead) <= self.outstanding() {
            self.acked = next;
        }
    }

    pub fn outstanding(&self) -> usize {
        usize::from(self.next_seq.wrapping_sub(self.acked))
    }

    /// Order `seq`, if it's outstanding.
    pub fn get(&self, seq: u16) -> Option<StampedOrder> {
        (usize::from(seq.wrapping_sub(self.acked)) < self.outstanding())
            .then(|| self.ring[usize::from(seq) % OUTSTANDING])
    }

    /// Puts the oldest outstanding orders in `p`, its newest command already set. One stamped
    /// further back than a packet can say goes as the oldest tick it can: the server has run that
    /// tick already, so it applies it at once either way.
    pub fn fill(&self, p: &mut InputPacket) {
        let newest = p.cmds[0].tick;
        let floor = newest.saturating_sub((1 << ORDER_BACK_BITS) - 1);
        let n = self.outstanding().min(MAX_ORDERS);
        for k in 0..n {
            let mut o = self.ring[usize::from(self.acked.wrapping_add(k as u16)) % OUTSTANDING];
            o.tick = o.tick.clamp(floor, newest);
            p.orders[k] = o;
        }
        p.n_orders = n as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::InputCmd;

    #[test]
    fn orders_repeat_until_acked_and_are_numbered_through_the_wrap() {
        let mut o = Orders { next_seq: 65_534, acked: 65_534, ..Orders::default() };
        for t in 0..5 {
            o.give(Order::Stop, 100 + t).unwrap();
        }
        let mut p = InputPacket { n_cmds: 1, ..InputPacket::default() };
        p.cmds[0] = InputCmd { tick: 110, ..InputCmd::default() };
        o.fill(&mut p);
        assert_eq!(p.n_orders, 5);
        assert_eq!(p.orders().iter().map(|s| s.seq).collect::<Vec<_>>(), [65_534, 65_535, 0, 1, 2]);
        o.on_ack(1);
        assert_eq!(o.outstanding(), 2);
        // An ack from the past, or from beyond what was given, moves nothing.
        o.on_ack(65_535);
        o.on_ack(9);
        assert_eq!(o.acked, 1);
        assert!(p.encode(&mut [0u8; 256]).is_some());
    }

    #[test]
    fn an_order_long_unacked_still_fits_a_packet() {
        let mut o = Orders::default();
        o.give(Order::Stop, 10).unwrap();
        let mut p = InputPacket { n_cmds: 1, ..InputPacket::default() };
        p.cmds[0] = InputCmd { tick: 500, ..InputCmd::default() };
        o.fill(&mut p);
        assert_eq!(p.orders[0].tick, 500 - 63);
        assert!(p.encode(&mut [0u8; 256]).is_some());
    }

    #[test]
    fn only_orders_not_yet_sent_are_stamped_again() {
        let mut o = Orders::default();
        o.give(Order::Stop, 10).unwrap();
        o.give(Order::Hold, 11).unwrap();
        o.restamp(1, 90);
        assert_eq!((o.get(0).unwrap().tick, o.get(1).unwrap().tick), (10, 90));
        // Never earlier than it was.
        o.restamp(1, 50);
        assert_eq!(o.get(1).unwrap().tick, 90);
    }

    #[test]
    fn too_many_outstanding_are_refused() {
        let mut o = Orders::default();
        for _ in 0..OUTSTANDING {
            assert!(o.give(Order::Stop, 1).is_some());
        }
        assert_eq!(o.give(Order::Stop, 1), None);
        assert_eq!(o.refused, 1);
    }
}
