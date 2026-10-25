//! Events on the wire: what happened that a team may know. Reliable ones (a unit leaving sight, a
//! missile's start and end, deaths, casts, levels) carry consecutive ids and repeat in every
//! datagram until one holding them is acked; the rest (damage numbers, a swing, a blast's flash)
//! go once, in the next datagram, and a lost one is simply gone.

use crate::quant::{POS_X_BITS, POS_Y_BITS};
use crate::{BitReader, BitWriter, DecodeError};

const KIND_BITS: u32 = 4;
/// Events a list carries at most.
pub const MAX_LIST: usize = 63;
const COUNT_BITS: u32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WireEvent {
    /// A unit the team saw left its sight, or died, as of `tick`: forget it unless a record newer
    /// than that says it's back.
    Leave {
        slot: u8,
        generation: u8,
        tick: u32,
    },
    /// A missile the team sees: where it is at `t0` (its start, or where it came into sight), and
    /// how it flies. A straight one goes `dir` (a facing code) at `speed` until `range` from
    /// `origin`; a homing one follows `target`. `owner` is its caster's slot, or 255 when the team
    /// can't see the caster (so it can't give it away).
    MissileSpawn {
        id: u16,
        generation: u8,
        team: u8,
        source: u16,
        owner: u8,
        origin: [u32; 2],
        t0: u32,
        target: u8,
        dir: u32,
        speed: u16,
        range: u16,
        width: u8,
    },
    MissileEnd {
        id: u16,
        generation: u8,
        at: [u32; 2],
    },
    /// A champion began a cast: its slot, where it aimed.
    Cast {
        unit: u8,
        slot: u8,
        at: [u32; 2],
    },
    Death {
        unit: u8,
        killer: u8,
    },
    Respawn {
        unit: u8,
    },
    LevelUp {
        unit: u8,
        level: u8,
    },
    Augment {
        unit: u8,
        augment: u8,
    },
    Relic {
        pad: u8,
        beam: bool,
        unit: u8,
    },
    Outcome {
        winner: u8,
    },
    // Unreliable.
    Damage {
        target: u8,
        source: u8,
        amount: u16,
    },
    Heal {
        target: u8,
        amount: u16,
    },
    Attack {
        unit: u8,
        target: u8,
    },
    Area {
        at: [u32; 2],
        radius: u16,
        team: u8,
    },
    Blink {
        unit: u8,
        to: [u32; 2],
    },
    #[default]
    Nothing,
}

impl WireEvent {
    /// Whether it must arrive (else it may be lost).
    pub fn reliable(&self) -> bool {
        !matches!(
            self,
            WireEvent::Damage { .. }
                | WireEvent::Heal { .. }
                | WireEvent::Attack { .. }
                | WireEvent::Area { .. }
                | WireEvent::Blink { .. }
                | WireEvent::Nothing
        )
    }

    fn kind(&self) -> u32 {
        match self {
            WireEvent::Leave { .. } => 0,
            WireEvent::MissileSpawn { .. } => 1,
            WireEvent::MissileEnd { .. } => 2,
            WireEvent::Cast { .. } => 3,
            WireEvent::Death { .. } => 4,
            WireEvent::Respawn { .. } => 5,
            WireEvent::LevelUp { .. } => 6,
            WireEvent::Augment { .. } => 7,
            WireEvent::Relic { .. } => 8,
            WireEvent::Outcome { .. } => 9,
            WireEvent::Damage { .. } => 10,
            WireEvent::Heal { .. } => 11,
            WireEvent::Attack { .. } => 12,
            WireEvent::Area { .. } => 13,
            WireEvent::Blink { .. } => 14,
            WireEvent::Nothing => 15,
        }
    }

    /// Bits [`write`](Self::write) takes.
    pub fn bits(&self) -> usize {
        let pos = POS_X_BITS + POS_Y_BITS;
        (KIND_BITS
            + match self {
                WireEvent::Leave { .. } => 8 + 8 + 32,
                WireEvent::MissileSpawn { .. } => 9 + 8 + 1 + 12 + 8 + pos + 32 + 8 + 10 + 12 + 13 + 8,
                WireEvent::MissileEnd { .. } => 9 + 8 + pos,
                WireEvent::Cast { .. } => 8 + 3 + pos,
                WireEvent::Death { .. } => 16,
                WireEvent::Respawn { .. } => 8,
                WireEvent::LevelUp { .. } => 8 + 5,
                WireEvent::Augment { .. } => 8 + 8,
                WireEvent::Relic { .. } => 2 + 1 + 8,
                WireEvent::Outcome { .. } => 1,
                WireEvent::Damage { .. } => 8 + 8 + 14,
                WireEvent::Heal { .. } => 8 + 14,
                WireEvent::Attack { .. } => 16,
                WireEvent::Area { .. } => pos + 11 + 1,
                WireEvent::Blink { .. } => 8 + pos,
                WireEvent::Nothing => 0,
            }) as usize
    }

    pub fn write(&self, w: &mut BitWriter<'_>) {
        let pos = |w: &mut BitWriter<'_>, p: [u32; 2]| {
            w.write_bits(p[0], POS_X_BITS);
            w.write_bits(p[1], POS_Y_BITS);
        };
        w.write_bits(self.kind(), KIND_BITS);
        match *self {
            WireEvent::Leave { slot, generation, tick } => {
                w.write_u8(slot);
                w.write_u8(generation);
                w.write_u32(tick);
            }
            WireEvent::MissileSpawn {
                id,
                generation,
                team,
                source,
                owner,
                origin,
                t0,
                target,
                dir,
                speed,
                range,
                width,
            } => {
                w.write_bits(u32::from(id), 9);
                w.write_u8(generation);
                w.write_bits(u32::from(team), 1);
                w.write_bits(u32::from(source), 12);
                w.write_u8(owner);
                pos(w, origin);
                w.write_u32(t0);
                w.write_u8(target);
                w.write_bits(dir, 10);
                w.write_bits(u32::from(speed).min(4_095), 12);
                w.write_bits(u32::from(range).min(8_191), 13);
                w.write_u8(width);
            }
            WireEvent::MissileEnd { id, generation, at } => {
                w.write_bits(u32::from(id), 9);
                w.write_u8(generation);
                pos(w, at);
            }
            WireEvent::Cast { unit, slot, at } => {
                w.write_u8(unit);
                w.write_bits(u32::from(slot), 3);
                pos(w, at);
            }
            WireEvent::Death { unit, killer } => {
                w.write_u8(unit);
                w.write_u8(killer);
            }
            WireEvent::Respawn { unit } => w.write_u8(unit),
            WireEvent::LevelUp { unit, level } => {
                w.write_u8(unit);
                w.write_bits(u32::from(level), 5);
            }
            WireEvent::Augment { unit, augment } => {
                w.write_u8(unit);
                w.write_u8(augment);
            }
            WireEvent::Relic { pad, beam, unit } => {
                w.write_bits(u32::from(pad), 2);
                w.write_bool(beam);
                w.write_u8(unit);
            }
            WireEvent::Outcome { winner } => w.write_bits(u32::from(winner), 1),
            WireEvent::Damage { target, source, amount } => {
                w.write_u8(target);
                w.write_u8(source);
                w.write_bits(u32::from(amount).min(16_383), 14);
            }
            WireEvent::Heal { target, amount } => {
                w.write_u8(target);
                w.write_bits(u32::from(amount).min(16_383), 14);
            }
            WireEvent::Attack { unit, target } => {
                w.write_u8(unit);
                w.write_u8(target);
            }
            WireEvent::Area { at, radius, team } => {
                pos(w, at);
                w.write_bits(u32::from(radius).min(2_047), 11);
                w.write_bits(u32::from(team), 1);
            }
            WireEvent::Blink { unit, to } => {
                w.write_u8(unit);
                pos(w, to);
            }
            WireEvent::Nothing => {}
        }
    }

    pub fn read(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let pos = |r: &mut BitReader<'_>| [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)];
        let e = match r.read_bits(KIND_BITS) {
            0 => WireEvent::Leave { slot: r.read_u8(), generation: r.read_u8(), tick: r.read_u32() },
            1 => WireEvent::MissileSpawn {
                id: r.read_bits(9) as u16,
                generation: r.read_u8(),
                team: r.read_bits(1) as u8,
                source: r.read_bits(12) as u16,
                owner: r.read_u8(),
                origin: pos(r),
                t0: r.read_u32(),
                target: r.read_u8(),
                dir: r.read_bits(10),
                speed: r.read_bits(12) as u16,
                range: r.read_bits(13) as u16,
                width: r.read_u8(),
            },
            2 => WireEvent::MissileEnd { id: r.read_bits(9) as u16, generation: r.read_u8(), at: pos(r) },
            3 => WireEvent::Cast { unit: r.read_u8(), slot: r.read_bits(3) as u8, at: pos(r) },
            4 => WireEvent::Death { unit: r.read_u8(), killer: r.read_u8() },
            5 => WireEvent::Respawn { unit: r.read_u8() },
            6 => WireEvent::LevelUp { unit: r.read_u8(), level: r.read_bits(5) as u8 },
            7 => WireEvent::Augment { unit: r.read_u8(), augment: r.read_u8() },
            8 => WireEvent::Relic { pad: r.read_bits(2) as u8, beam: r.read_bool(), unit: r.read_u8() },
            9 => WireEvent::Outcome { winner: r.read_bits(1) as u8 },
            10 => {
                WireEvent::Damage { target: r.read_u8(), source: r.read_u8(), amount: r.read_bits(14) as u16 }
            }
            11 => WireEvent::Heal { target: r.read_u8(), amount: r.read_bits(14) as u16 },
            12 => WireEvent::Attack { unit: r.read_u8(), target: r.read_u8() },
            13 => WireEvent::Area { at: pos(r), radius: r.read_bits(11) as u16, team: r.read_bits(1) as u8 },
            14 => WireEvent::Blink { unit: r.read_u8(), to: pos(r) },
            _ => WireEvent::Nothing,
        };
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(e) }
    }
}

/// Writes a list of events: their count, then each (a reliable list first writes its first id).
pub fn write_list(w: &mut BitWriter<'_>, first_id: Option<u16>, events: &[WireEvent]) {
    let n = events.len().min(MAX_LIST);
    w.write_bits(n as u32, COUNT_BITS);
    if let Some(id) = first_id {
        w.write_u16(id);
    }
    for e in &events[..n] {
        e.write(w);
    }
}

/// Bits [`write_list`] takes for these events.
pub fn list_bits(reliable: bool, events: &[WireEvent]) -> usize {
    COUNT_BITS as usize + if reliable { 16 } else { 0 } + events.iter().map(WireEvent::bits).sum::<usize>()
}

/// Reads a list into `out`: the first id (reliable lists) and how many.
pub fn read_list(
    r: &mut BitReader<'_>,
    reliable: bool,
    out: &mut [WireEvent; MAX_LIST],
) -> Result<(u16, usize), DecodeError> {
    let n = r.read_bits(COUNT_BITS) as usize;
    let first = if reliable { r.read_u16() } else { 0 };
    for e in out.iter_mut().take(n) {
        *e = WireEvent::read(r)?;
    }
    if r.overflowed() { Err(DecodeError::Truncated) } else { Ok((first, n)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_round_trips_and_says_its_size() {
        let all = [
            WireEvent::Leave { slot: 40, generation: 3, tick: 99_999 },
            WireEvent::MissileSpawn {
                id: 511,
                generation: 200,
                team: 1,
                source: 0x123,
                owner: 255,
                origin: [70_000, 20_000],
                t0: 1_234_567,
                target: 255,
                dir: 700,
                speed: 2_000,
                range: 3_000,
                width: 120,
            },
            WireEvent::MissileEnd { id: 3, generation: 4, at: [5, 6] },
            WireEvent::Cast { unit: 9, slot: 5, at: [7, 8] },
            WireEvent::Death { unit: 1, killer: 30 },
            WireEvent::Respawn { unit: 2 },
            WireEvent::LevelUp { unit: 3, level: 18 },
            WireEvent::Augment { unit: 4, augment: 17 },
            WireEvent::Relic { pad: 3, beam: true, unit: 255 },
            WireEvent::Outcome { winner: 1 },
            WireEvent::Damage { target: 1, source: 2, amount: 1_234 },
            WireEvent::Heal { target: 1, amount: 50 },
            WireEvent::Attack { unit: 30, target: 2 },
            WireEvent::Area { at: [100, 200], radius: 350, team: 0 },
            WireEvent::Blink { unit: 4, to: [1, 2] },
        ];
        for e in all {
            let mut buf = [0u8; 64];
            let mut w = BitWriter::new(&mut buf);
            e.write(&mut w);
            assert_eq!(w.bits_written(), e.bits(), "{e:?}");
            assert_eq!(WireEvent::read(&mut BitReader::new(&buf)).unwrap(), e);
        }
        let mut buf = [0u8; 512];
        let mut w = BitWriter::new(&mut buf);
        write_list(&mut w, Some(65_530), &all);
        assert_eq!(w.bits_written(), list_bits(true, &all));
        let mut out = [WireEvent::Nothing; MAX_LIST];
        let (first, n) = read_list(&mut BitReader::new(&buf), true, &mut out).unwrap();
        assert_eq!((first, n), (65_530, all.len()));
        assert_eq!(&out[..n], &all);
    }
}
