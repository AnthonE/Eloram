//! A seat's inputs: the per-tick commands, through the jitter buffer; the orders, each applied
//! exactly once, at the tick its client stamped it for (`docs/NETCODE.md`, "Inputs").

use el_proto::input::StampedOrder;
use el_proto::{InputCmd, InputPacket};
use el_sim::game::{MAX_ORDERS_TICK, SeatInput};

use crate::jitter::{JitterBuffer, Ticked};

impl Ticked for InputCmd {
    fn tick(&self) -> u32 {
        self.tick
    }
}

/// How many orders past the next expected the server holds while an earlier one is missing.
pub const ORDER_WINDOW: usize = 32;
/// An order stamped further ahead than this is applied now (the client's clock has run away).
const ORDER_AHEAD: u32 = 120;

/// One seat's network state on the shard thread.
pub struct SeatNet {
    /// A player holds the seat (else a bot plays it).
    pub human: bool,
    jitter: JitterBuffer<InputCmd>,
    last: InputCmd,
    missed_run: u32,
    /// The last thirty ticks, a bit each: 1 where the server stood in for a missing command.
    missed_bits: u32,
    orders: [Option<StampedOrder>; ORDER_WINDOW],
    /// The next order expected; every one before it has been applied.
    pub next_seq: u16,
    /// The newest input packet's timestamp, and when it arrived (µs).
    pub echo: (u16, u64),
    pub max_datagram: u16,
    /// Orders applied, all told (for the metrics and the tests).
    pub orders_applied: u64,
}

impl Default for SeatNet {
    fn default() -> Self {
        Self {
            human: false,
            jitter: JitterBuffer::default(),
            last: InputCmd::default(),
            missed_run: 0,
            missed_bits: 0,
            orders: [None; ORDER_WINDOW],
            next_seq: 0,
            echo: (0, 0),
            max_datagram: el_proto::MAX_DATAGRAM as u16,
            orders_applied: 0,
        }
    }
}

impl SeatNet {
    /// A player (re)takes the seat: their client counts its orders from 0 again.
    pub fn seat(&mut self, max_datagram: u16) {
        *self = SeatNet {
            human: true,
            max_datagram: max_datagram.clamp(256, el_proto::MAX_DATAGRAM as u16),
            orders_applied: self.orders_applied,
            ..SeatNet::default()
        };
    }

    /// Takes an input packet that arrived at `recv_us`, the match about to step `next`.
    pub fn take(&mut self, p: &InputPacket, recv_us: u64, next: u32) {
        for c in p.cmds() {
            self.jitter.insert(*c, next);
        }
        for o in p.orders() {
            let ahead = o.seq.wrapping_sub(self.next_seq);
            if usize::from(ahead) < ORDER_WINDOW {
                let k = usize::from(o.seq) % ORDER_WINDOW;
                if self.orders[k].is_none_or(|held| held.seq != o.seq) {
                    self.orders[k] = Some(*o);
                }
            }
        }
        self.echo = (p.client_time_ms, recv_us);
    }

    /// The seat's input for tick `t`: its command (or a stand-in), and the orders due.
    pub fn input_for(&mut self, t: u32) -> SeatInput {
        let cmd = match self.jitter.take(t) {
            Some(c) => {
                self.missed_run = 0;
                self.missed_bits <<= 1;
                c
            }
            None => {
                self.missed_run += 1;
                self.missed_bits = self.missed_bits << 1 | 1;
                InputCmd::stand_in(&self.last, t, self.missed_run - 1)
            }
        };
        self.last = cmd;
        let mut input = SeatInput { steer: cmd.steer, ..SeatInput::default() };
        while usize::from(input.n) < MAX_ORDERS_TICK {
            let k = usize::from(self.next_seq) % ORDER_WINDOW;
            match self.orders[k] {
                Some(o) if o.seq == self.next_seq && (o.tick <= t || o.tick > t + ORDER_AHEAD) => {
                    input.push(o.order);
                    self.orders[k] = None;
                    self.next_seq = self.next_seq.wrapping_add(1);
                    self.orders_applied += 1;
                }
                _ => break,
            }
        }
        input
    }

    /// Commands held for `t` and after: the gauge the client's clock steers on.
    pub fn depth(&self, t: u32) -> u32 {
        self.jitter.depth(t)
    }

    /// Ticks of the last thirty stood in for.
    pub fn missed(&self) -> u32 {
        (self.missed_bits & ((1 << 30) - 1)).count_ones()
    }

    /// The camera's centre the client last said (for priority).
    pub fn view(&self) -> [u32; 2] {
        self.last.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::Order;

    fn packet(tick: u32, orders: &[(u16, u32)]) -> InputPacket {
        let mut p = InputPacket { n_cmds: 1, ..InputPacket::default() };
        p.cmds[0] = InputCmd { tick, steer: 0, view: [0, 0] };
        for (k, &(seq, at)) in orders.iter().enumerate() {
            p.orders[k] = StampedOrder { seq, tick: at, order: Order::Stop };
        }
        p.n_orders = orders.len() as u8;
        p
    }

    #[test]
    fn an_order_applies_once_at_its_tick_however_often_it_arrives() {
        let mut s = SeatNet::default();
        s.seat(1100);
        // Order 0 for tick 12 arrives three times, early; order 1 arrives before order 0 once.
        s.take(&packet(12, &[(1, 13)]), 0, 10);
        assert_eq!(s.input_for(10).n, 0, "order 1 waits for order 0");
        s.take(&packet(12, &[(0, 12), (1, 13)]), 0, 11);
        s.take(&packet(12, &[(0, 12), (1, 13)]), 0, 11);
        assert_eq!(s.input_for(11).n, 0, "not yet its tick");
        assert_eq!(s.input_for(12).n, 1);
        assert_eq!(s.input_for(13).n, 1);
        s.take(&packet(14, &[(0, 12), (1, 13)]), 0, 14);
        assert_eq!(s.input_for(14).n, 0, "never twice");
        assert_eq!((s.next_seq, s.orders_applied), (2, 2));
        // Late: stamped for a tick already run, it applies at once.
        s.take(&packet(20, &[(2, 15)]), 0, 20);
        assert_eq!(s.input_for(20).n, 1);
    }

    #[test]
    fn a_missing_command_is_stood_in_for_and_counted() {
        let mut s = SeatNet::default();
        s.seat(1100);
        s.take(&packet(5, &[]), 0, 5);
        s.input_for(5);
        s.input_for(6);
        s.input_for(7);
        assert_eq!(s.missed(), 2);
    }
}
