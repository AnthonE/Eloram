//! The units' records the client took, kept so that a delta against one of them can be undone
//! (`el_proto::unit`, `docs/NETCODE.md`, "The wire"). Before Colony's `baselines.rs`, at a fixed
//! size: a slot for every unit, sized once.

use el_proto::unit::{KEPT, QUnit};
use el_sim::units::MAX_UNITS;

/// A unit's newest [`KEPT`] records, however old: the server takes a delta only against one it
/// knows the client took and of which it sent fewer than [`KEPT`] newer, so no newer one can have
/// pushed it out. A unit that says nothing new is not sent again, so its baseline can be minutes
/// old.
#[derive(Clone, Copy)]
struct Ring {
    at: [(u32, QUnit); KEPT],
    /// Records taken, all told: the next goes at `n % KEPT`.
    n: u32,
}

/// Every unit's kept records, by slot. They are kept whatever becomes of the unit (out of sight, a
/// new life in its slot): the server names them by slot and tick, and only ones it knows the client
/// took. A new stream (another seat, another match) starts them all over.
pub struct Baselines {
    slots: Box<[Ring]>,
}

impl Default for Baselines {
    fn default() -> Self {
        Self { slots: vec![Ring { at: [(0, QUnit::default()); KEPT], n: 0 }; MAX_UNITS].into_boxed_slice() }
    }
}

impl Baselines {
    /// Forgets everything (a new stream).
    pub fn clear(&mut self) {
        for r in self.slots.iter_mut() {
            r.n = 0;
        }
    }

    /// The record of `slot` taken at `tick`, if it's kept.
    pub fn get(&self, slot: u8, tick: u32) -> Option<QUnit> {
        let ring = &self.slots[usize::from(slot)];
        let held = (ring.n as usize).min(KEPT);
        ring.at[..held].iter().find(|(t, _)| *t == tick).map(|&(_, q)| q)
    }

    /// Keeps a record taken at `tick` (a datagram's records go in once it's taken whole; a unit
    /// has one record a tick at most).
    pub fn put(&mut self, tick: u32, q: QUnit) {
        let ring = &mut self.slots[usize::from(q.slot)];
        ring.at[ring.n as usize % KEPT] = (tick, q);
        ring.n = ring.n.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_of_each_unit_however_old() {
        let mut b = Baselines::default();
        let q = |slot| QUnit { slot, ..QUnit::default() };
        for t in 0..40 {
            b.put(t, QUnit { flags: t, ..q(7) });
        }
        b.put(5, q(200));
        assert_eq!(b.get(7, 39).map(|e| e.flags), Some(39));
        assert_eq!(b.get(7, 40 - KEPT as u32).map(|e| e.flags), Some(40 - KEPT as u32));
        assert_eq!(b.get(7, 39 - KEPT as u32), None);
        assert_eq!(b.get(200, 5), Some(q(200)));
        assert_eq!(b.get(201, 5), None);
        // A record taken long ago stays while fewer than KEPT newer came.
        b.put(9_000, QUnit { flags: 1, ..q(200) });
        assert_eq!(b.get(200, 5), Some(q(200)));
        b.clear();
        assert_eq!(b.get(7, 39), None);
    }
}
