//! Orders: what a MOBA player tells their champion. Move, attack, cast, level, pick, buy; and
//! what they tell their team: a ping, a vote to surrender.
//!
//! An order is sent with a sequence number, repeated in every input packet until the server has
//! acked it, and applied by the server exactly once (`docs/NETCODE.md`, "Inputs"). Its positions go
//! through [`Order::quantized`] before the client predicts with it or sends it, so the server and
//! the owner's prediction act on the same numbers to the bit.

use glam::Vec2;

use crate::quant::{quantize_pos, read_pos, write_pos};
use crate::{BitReader, BitWriter, DecodeError};

/// A unit named on the wire: its slot and the low byte of its generation, so an order aimed at a
/// unit that died (and whose slot was reused) is aimed at nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct UnitRef {
    pub slot: u8,
    pub generation: u8,
}

impl UnitRef {
    pub const BITS: u32 = 16;

    pub fn write(self, w: &mut BitWriter<'_>) {
        w.write_bits(u32::from(self.slot), 8);
        w.write_bits(u32::from(self.generation), 8);
    }

    pub fn read(r: &mut BitReader<'_>) -> Self {
        Self { slot: r.read_bits(8) as u8, generation: r.read_bits(8) as u8 }
    }
}

/// What a cast is aimed at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Aim {
    /// Self-cast, or the ability needs no target.
    #[default]
    None,
    /// A point on the map (skillshots aim at it as a direction from the caster).
    Point(Vec2),
    /// A unit.
    Unit(UnitRef),
}

/// The ability slots: Q, W, E, R, then the two summoner spells D and F.
pub mod slot {
    pub const Q: u8 = 0;
    pub const W: u8 = 1;
    pub const E: u8 = 2;
    pub const R: u8 = 3;
    pub const D: u8 = 4;
    pub const F: u8 = 5;
    /// Q, W, E, R, D, F.
    pub const COUNT: usize = 6;
}

/// What a ping says ([`Order::Ping`]).
pub mod ping {
    /// Look here.
    pub const LOOK: u8 = 0;
    /// Careful: danger here.
    pub const DANGER: u8 = 1;
    /// On my way.
    pub const COMING: u8 = 2;
    /// An enemy is missing from where it was.
    pub const GONE: u8 = 3;
    /// Go in: attack this.
    pub const ATTACK: u8 = 4;
    /// Fall back.
    pub const BACK: u8 = 5;
    pub const COUNT: u8 = 6;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Order {
    /// Walk to a point, and stand there.
    Move { to: Vec2 },
    /// Walk to a point, fighting the nearest enemy met on the way.
    AttackMove { to: Vec2 },
    /// Attack a unit until it dies or the champion is told otherwise.
    Attack { unit: UnitRef },
    /// Cast an ability ([`slot`]).
    Cast { slot: u8, aim: Aim },
    /// Stop what you're doing.
    #[default]
    Stop,
    /// Stand and fight what comes into range, without moving.
    Hold,
    /// Spend a skill point on Q, W, E or R.
    LevelSkill { slot: u8 },
    /// Take an offer on the open augment screen (0, 1 or 2).
    PickAugment { card: u8 },
    /// Reroll an offer on the open augment screen.
    Reroll { card: u8 },
    /// Buy an item (its index in the shop's catalogue).
    Buy { item: u8 },
    /// Sell what's in an inventory slot.
    Sell { inv: u8 },
    /// Tell the team something ([`ping`]) about a place, or a unit the team sees there.
    Ping { kind: u8, at: Vec2, unit: Option<UnitRef> },
    /// Vote to surrender (or not): a yes opens a vote when none is.
    Surrender { yes: bool },
}

const TAG_BITS: u32 = 4;

impl Order {
    /// The order as the server will read it: every position on the wire's grid.
    pub fn quantized(self) -> Self {
        match self {
            Order::Move { to } => Order::Move { to: quantize_pos(to) },
            Order::AttackMove { to } => Order::AttackMove { to: quantize_pos(to) },
            Order::Cast { slot, aim: Aim::Point(p) } => {
                Order::Cast { slot, aim: Aim::Point(quantize_pos(p)) }
            }
            Order::Ping { kind, at, unit } => Order::Ping { kind, at: quantize_pos(at), unit },
            o => o,
        }
    }

    fn tag(&self) -> u32 {
        match self {
            Order::Move { .. } => 0,
            Order::AttackMove { .. } => 1,
            Order::Attack { .. } => 2,
            Order::Cast { .. } => 3,
            Order::Stop => 4,
            Order::Hold => 5,
            Order::LevelSkill { .. } => 6,
            Order::PickAugment { .. } => 7,
            Order::Reroll { .. } => 8,
            Order::Buy { .. } => 9,
            Order::Sell { .. } => 10,
            Order::Ping { .. } => 11,
            Order::Surrender { .. } => 12,
        }
    }

    pub fn write(&self, w: &mut BitWriter<'_>) {
        w.write_bits(self.tag(), TAG_BITS);
        match *self {
            Order::Move { to } | Order::AttackMove { to } => write_pos(w, to),
            Order::Attack { unit } => unit.write(w),
            Order::Cast { slot, aim } => {
                w.write_bits(u32::from(slot), 3);
                match aim {
                    Aim::None => w.write_bits(0, 2),
                    Aim::Point(p) => {
                        w.write_bits(1, 2);
                        write_pos(w, p);
                    }
                    Aim::Unit(u) => {
                        w.write_bits(2, 2);
                        u.write(w);
                    }
                }
            }
            Order::Stop | Order::Hold => {}
            Order::LevelSkill { slot } => w.write_bits(u32::from(slot), 2),
            Order::PickAugment { card } | Order::Reroll { card } => w.write_bits(u32::from(card), 2),
            Order::Buy { item } => w.write_bits(u32::from(item), 6),
            Order::Sell { inv } => w.write_bits(u32::from(inv), 3),
            Order::Ping { kind, at, unit } => {
                w.write_bits(u32::from(kind), 3);
                write_pos(w, at);
                w.write_bool(unit.is_some());
                if let Some(u) = unit {
                    u.write(w);
                }
            }
            Order::Surrender { yes } => w.write_bool(yes),
        }
    }

    pub fn read(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let o = match r.read_bits(TAG_BITS) {
            0 => Order::Move { to: read_pos(r) },
            1 => Order::AttackMove { to: read_pos(r) },
            2 => Order::Attack { unit: UnitRef::read(r) },
            3 => {
                let slot = r.read_bits(3) as u8;
                if usize::from(slot) >= slot::COUNT {
                    return Err(DecodeError::Invalid);
                }
                let aim = match r.read_bits(2) {
                    0 => Aim::None,
                    1 => Aim::Point(read_pos(r)),
                    2 => Aim::Unit(UnitRef::read(r)),
                    _ => return Err(DecodeError::Invalid),
                };
                Order::Cast { slot, aim }
            }
            4 => Order::Stop,
            5 => Order::Hold,
            6 => Order::LevelSkill { slot: r.read_bits(2) as u8 },
            7 => Order::PickAugment { card: Self::card(r)? },
            8 => Order::Reroll { card: Self::card(r)? },
            9 => Order::Buy { item: r.read_bits(6) as u8 },
            10 => Order::Sell { inv: r.read_bits(3) as u8 },
            11 => {
                let kind = r.read_bits(3) as u8;
                if kind >= ping::COUNT {
                    return Err(DecodeError::Invalid);
                }
                let at = read_pos(r);
                let unit = r.read_bool().then(|| UnitRef::read(r));
                Order::Ping { kind, at, unit }
            }
            12 => Order::Surrender { yes: r.read_bool() },
            _ => return Err(DecodeError::Invalid),
        };
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(o) }
    }

    fn card(r: &mut BitReader<'_>) -> Result<u8, DecodeError> {
        let c = r.read_bits(2) as u8;
        if c > 2 { Err(DecodeError::Invalid) } else { Ok(c) }
    }

    /// The bits [`write`](Self::write) takes.
    pub fn bits(&self) -> u32 {
        TAG_BITS
            + match self {
                Order::Move { .. } | Order::AttackMove { .. } => 32,
                Order::Attack { .. } => UnitRef::BITS,
                Order::Cast { aim, .. } => {
                    5 + match aim {
                        Aim::None => 0,
                        Aim::Point(_) => 32,
                        Aim::Unit(_) => UnitRef::BITS,
                    }
                }
                Order::Stop | Order::Hold => 0,
                Order::LevelSkill { .. } | Order::PickAugment { .. } | Order::Reroll { .. } => 2,
                Order::Buy { .. } => 6,
                Order::Sell { .. } => 3,
                Order::Ping { unit, .. } => 3 + 32 + 1 + if unit.is_some() { UnitRef::BITS } else { 0 },
                Order::Surrender { .. } => 1,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> [Order; 16] {
        let u = UnitRef { slot: 37, generation: 200 };
        let p = Vec2::new(7_123.4, 1_901.2);
        [
            Order::Move { to: p },
            Order::AttackMove { to: p },
            Order::Attack { unit: u },
            Order::Cast { slot: slot::Q, aim: Aim::Point(p) },
            Order::Cast { slot: slot::R, aim: Aim::Unit(u) },
            Order::Cast { slot: slot::F, aim: Aim::None },
            Order::Stop,
            Order::Hold,
            Order::LevelSkill { slot: slot::E },
            Order::PickAugment { card: 2 },
            Order::Reroll { card: 1 },
            Order::Buy { item: 63 },
            Order::Sell { inv: 6 },
            Order::Ping { kind: ping::DANGER, at: p, unit: None },
            Order::Ping { kind: ping::BACK, at: p, unit: Some(u) },
            Order::Surrender { yes: true },
        ]
    }

    #[test]
    fn every_order_round_trips_quantized_and_says_its_size() {
        for o in every() {
            let q = o.quantized();
            assert_eq!(q.quantized(), q, "quantizing twice changes nothing");
            let mut buf = [0u8; 16];
            let mut w = BitWriter::new(&mut buf);
            q.write(&mut w);
            assert_eq!(w.bits_written() as u32, q.bits(), "{q:?}");
            let back = Order::read(&mut BitReader::new(&buf)).unwrap();
            assert_eq!(back, q);
        }
    }

    #[test]
    fn hostile_orders_are_refused_not_panicked_on() {
        for raw in 0u32..(1 << 12) {
            let mut buf = [0u8; 2];
            buf.copy_from_slice(&(raw as u16).to_le_bytes());
            let _ = Order::read(&mut BitReader::new(&buf));
        }
        let mut buf = [0u8; 8];
        let mut w = BitWriter::new(&mut buf);
        w.write_bits(3, TAG_BITS);
        w.write_bits(7, 3);
        assert_eq!(Order::read(&mut BitReader::new(&buf)), Err(DecodeError::Invalid));
    }
}
