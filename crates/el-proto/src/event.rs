//! Events on the wire: what happened that a team may know. Reliable ones (a unit leaving sight, a
//! missile's start and end, deaths, casts, levels) carry consecutive ids and repeat in every
//! datagram until one holding them is acked; the rest (damage numbers, a swing, a blast's flash)
//! go once, in the next datagram, and a lost one is simply gone.

use crate::quant::{POS_X_BITS, POS_Y_BITS};
use crate::{BitReader, BitWriter, DecodeError};

const KIND_BITS: u32 = 5;
/// An event's tick (when a unit left sight, where a missile was): how many ticks before its
/// snapshot's, in [`AGE_BITS`] after a bit, or else whole.
const AGE_BITS: u32 = 5;

fn tick_bits(t: u32, now: u32) -> u32 {
    1 + if now.wrapping_sub(t) < 1 << AGE_BITS { AGE_BITS } else { 32 }
}

fn write_tick(w: &mut BitWriter<'_>, t: u32, now: u32) {
    let age = now.wrapping_sub(t);
    let short = age < 1 << AGE_BITS;
    w.write_bool(short);
    if short {
        w.write_bits(age, AGE_BITS);
    } else {
        w.write_u32(t);
    }
}

fn read_tick(r: &mut BitReader<'_>, now: u32) -> u32 {
    if r.read_bool() { now.wrapping_sub(r.read_bits(AGE_BITS)) } else { r.read_u32() }
}

/// A tick either side of the snapshot's (a zone's next pulse, or its end): within
/// ±2^([`WHEN_BITS`] − 1) of it in [`WHEN_BITS`] after a bit, or else whole.
const WHEN_BITS: u32 = 10;
const WHEN_HALF: i64 = 1 << (WHEN_BITS - 1);

fn when_near(t: u32, now: u32) -> bool {
    (-WHEN_HALF..WHEN_HALF).contains(&(i64::from(t) - i64::from(now)))
}

fn when_bits(t: u32, now: u32) -> u32 {
    1 + if when_near(t, now) { WHEN_BITS } else { 32 }
}

fn write_when(w: &mut BitWriter<'_>, t: u32, now: u32) {
    let near = when_near(t, now);
    w.write_bool(near);
    if near {
        w.write_bits((i64::from(t) - i64::from(now) + WHEN_HALF) as u32, WHEN_BITS);
    } else {
        w.write_u32(t);
    }
}

fn read_when(r: &mut BitReader<'_>, now: u32) -> u32 {
    if r.read_bool() {
        (i64::from(now) + i64::from(r.read_bits(WHEN_BITS)) - WHEN_HALF) as u32
    } else {
        r.read_u32()
    }
}

/// Bits a source's code takes (`el_sim::missile::Source::code`: an auto, a turret, an ability of a
/// champion's, an augment...).
pub const SOURCE_BITS: u32 = 12;
/// Bits a zone's slot takes (the match keeps at most 2^this).
pub const ZONE_BITS: u32 = 6;
/// Bits an amount of damage, healing or shield takes.
const AMOUNT_BITS: u32 = 14;
const AMOUNT_MAX: u32 = (1 << AMOUNT_BITS) - 1;
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
    /// A teammate's ping ([`crate::order::ping`]): where, and the unit it names (255: none).
    Ping {
        seat: u8,
        kind: u8,
        at: [u32; 2],
        unit: u8,
    },
    /// The team's surrender vote as it stands: who said yes and who no (a bit a seat of the team,
    /// in seat order), whether it's still open, and whether it passed.
    Vote {
        team: u8,
        yes: u8,
        no: u8,
        open: bool,
        passed: bool,
    },
    /// A zone the team sees the ground of (an area that lands late, or a field): where, how
    /// wide, what it is, when it next fires, every how many ticks after that (0: it fires once),
    /// and its last tick. `owner` is its caster's slot, or 255 when the team can't see them; a zone
    /// that `follows` its owner goes only to a team that sees them.
    ZoneSpawn {
        id: u8,
        generation: u8,
        team: u8,
        source: u16,
        owner: u8,
        follows: bool,
        at: [u32; 2],
        radius: u16,
        next: u32,
        period: u16,
        end: u32,
    },
    /// A zone told of is gone (it ended, or it follows an owner the team no longer sees).
    ZoneEnd {
        id: u8,
        generation: u8,
    },
    // Unreliable.
    /// `source` (255: unseen) hurt `target` with `kind` (0 physical, 1 magic, 2 true), by way of
    /// `from` (a source's code).
    Damage {
        target: u8,
        source: u8,
        amount: u16,
        kind: u8,
        from: u16,
    },
    Heal {
        target: u8,
        amount: u16,
        from: u16,
    },
    /// A shield went up on `target`.
    Shield {
        target: u8,
        amount: u16,
        from: u16,
    },
    Attack {
        unit: u8,
        target: u8,
    },
    /// An area's effect landed: zone `zone`'s (a delayed blast's, a field's pulse), its
    /// `source`'s, its `owner`'s (255: unseen, never for one that `follows` them).
    Area {
        zone: u8,
        generation: u8,
        at: [u32; 2],
        radius: u16,
        team: u8,
        source: u16,
        owner: u8,
        follows: bool,
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
                | WireEvent::Shield { .. }
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
            WireEvent::Ping { .. } => 15,
            WireEvent::Vote { .. } => 16,
            WireEvent::ZoneSpawn { .. } => 17,
            WireEvent::ZoneEnd { .. } => 18,
            WireEvent::Shield { .. } => 19,
            WireEvent::Nothing => 31,
        }
    }

    /// Bits [`write`](Self::write) takes in a snapshot of tick `now`.
    pub fn bits(&self, now: u32) -> usize {
        let pos = POS_X_BITS + POS_Y_BITS;
        (KIND_BITS
            + match *self {
                WireEvent::Leave { tick, .. } => 8 + 8 + tick_bits(tick, now),
                WireEvent::MissileSpawn { t0, .. } => {
                    9 + 8 + 1 + SOURCE_BITS + 8 + pos + tick_bits(t0, now) + 8 + 10 + 12 + 13 + 8
                }
                WireEvent::MissileEnd { .. } => 9 + 8 + pos,
                WireEvent::Cast { .. } => 8 + 3 + pos,
                WireEvent::Death { .. } => 16,
                WireEvent::Respawn { .. } => 8,
                WireEvent::LevelUp { .. } => 8 + 5,
                WireEvent::Augment { .. } => 8 + 8,
                WireEvent::Relic { .. } => 2 + 1 + 8,
                WireEvent::Outcome { .. } => 1,
                WireEvent::ZoneSpawn { next, end, .. } => {
                    ZONE_BITS
                        + 8
                        + 1
                        + SOURCE_BITS
                        + 8
                        + 1
                        + pos
                        + 11
                        + when_bits(next, now)
                        + 10
                        + when_bits(end, now)
                }
                WireEvent::ZoneEnd { .. } => ZONE_BITS + 8,
                WireEvent::Damage { .. } => 8 + 8 + AMOUNT_BITS + 2 + SOURCE_BITS,
                WireEvent::Heal { .. } | WireEvent::Shield { .. } => 8 + AMOUNT_BITS + SOURCE_BITS,
                WireEvent::Attack { .. } => 16,
                WireEvent::Area { .. } => ZONE_BITS + 8 + pos + 11 + 1 + SOURCE_BITS + 8 + 1,
                WireEvent::Blink { .. } => 8 + pos,
                WireEvent::Ping { .. } => 4 + 3 + pos + 8,
                WireEvent::Vote { .. } => 1 + 5 + 5 + 2,
                WireEvent::Nothing => 0,
            }) as usize
    }

    /// Writes it in a snapshot of tick `now`.
    pub fn write(&self, w: &mut BitWriter<'_>, now: u32) {
        let pos = |w: &mut BitWriter<'_>, p: [u32; 2]| {
            w.write_bits(p[0], POS_X_BITS);
            w.write_bits(p[1], POS_Y_BITS);
        };
        w.write_bits(self.kind(), KIND_BITS);
        match *self {
            WireEvent::Leave { slot, generation, tick } => {
                w.write_u8(slot);
                w.write_u8(generation);
                write_tick(w, tick, now);
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
                w.write_bits(u32::from(source), SOURCE_BITS);
                w.write_u8(owner);
                pos(w, origin);
                write_tick(w, t0, now);
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
            WireEvent::ZoneSpawn {
                id,
                generation,
                team,
                source,
                owner,
                follows,
                at,
                radius,
                next,
                period,
                end,
            } => {
                w.write_bits(u32::from(id), ZONE_BITS);
                w.write_u8(generation);
                w.write_bits(u32::from(team), 1);
                w.write_bits(u32::from(source), SOURCE_BITS);
                w.write_u8(owner);
                w.write_bool(follows);
                pos(w, at);
                w.write_bits(u32::from(radius).min(2_047), 11);
                write_when(w, next, now);
                w.write_bits(u32::from(period).min(1_023), 10);
                write_when(w, end, now);
            }
            WireEvent::ZoneEnd { id, generation } => {
                w.write_bits(u32::from(id), ZONE_BITS);
                w.write_u8(generation);
            }
            WireEvent::Damage { target, source, amount, kind, from } => {
                w.write_u8(target);
                w.write_u8(source);
                w.write_bits(u32::from(amount).min(AMOUNT_MAX), AMOUNT_BITS);
                w.write_bits(u32::from(kind), 2);
                w.write_bits(u32::from(from), SOURCE_BITS);
            }
            WireEvent::Heal { target, amount, from } | WireEvent::Shield { target, amount, from } => {
                w.write_u8(target);
                w.write_bits(u32::from(amount).min(AMOUNT_MAX), AMOUNT_BITS);
                w.write_bits(u32::from(from), SOURCE_BITS);
            }
            WireEvent::Attack { unit, target } => {
                w.write_u8(unit);
                w.write_u8(target);
            }
            WireEvent::Area { zone, generation, at, radius, team, source, owner, follows } => {
                w.write_bits(u32::from(zone), ZONE_BITS);
                w.write_u8(generation);
                pos(w, at);
                w.write_bits(u32::from(radius).min(2_047), 11);
                w.write_bits(u32::from(team), 1);
                w.write_bits(u32::from(source), SOURCE_BITS);
                w.write_u8(owner);
                w.write_bool(follows);
            }
            WireEvent::Blink { unit, to } => {
                w.write_u8(unit);
                pos(w, to);
            }
            WireEvent::Ping { seat, kind, at, unit } => {
                w.write_bits(u32::from(seat), 4);
                w.write_bits(u32::from(kind), 3);
                pos(w, at);
                w.write_u8(unit);
            }
            WireEvent::Vote { team, yes, no, open, passed } => {
                w.write_bits(u32::from(team), 1);
                w.write_bits(u32::from(yes), 5);
                w.write_bits(u32::from(no), 5);
                w.write_bool(open);
                w.write_bool(passed);
            }
            WireEvent::Nothing => {}
        }
    }

    /// Reads one from a snapshot of tick `now`.
    pub fn read(r: &mut BitReader<'_>, now: u32) -> Result<Self, DecodeError> {
        let pos = |r: &mut BitReader<'_>| [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)];
        let e = match r.read_bits(KIND_BITS) {
            0 => WireEvent::Leave { slot: r.read_u8(), generation: r.read_u8(), tick: read_tick(r, now) },
            1 => WireEvent::MissileSpawn {
                id: r.read_bits(9) as u16,
                generation: r.read_u8(),
                team: r.read_bits(1) as u8,
                source: r.read_bits(SOURCE_BITS) as u16,
                owner: r.read_u8(),
                origin: pos(r),
                t0: read_tick(r, now),
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
            10 => WireEvent::Damage {
                target: r.read_u8(),
                source: r.read_u8(),
                amount: r.read_bits(AMOUNT_BITS) as u16,
                kind: r.read_bits(2) as u8,
                from: r.read_bits(SOURCE_BITS) as u16,
            },
            11 => WireEvent::Heal {
                target: r.read_u8(),
                amount: r.read_bits(AMOUNT_BITS) as u16,
                from: r.read_bits(SOURCE_BITS) as u16,
            },
            12 => WireEvent::Attack { unit: r.read_u8(), target: r.read_u8() },
            13 => WireEvent::Area {
                zone: r.read_bits(ZONE_BITS) as u8,
                generation: r.read_u8(),
                at: pos(r),
                radius: r.read_bits(11) as u16,
                team: r.read_bits(1) as u8,
                source: r.read_bits(SOURCE_BITS) as u16,
                owner: r.read_u8(),
                follows: r.read_bool(),
            },
            14 => WireEvent::Blink { unit: r.read_u8(), to: pos(r) },
            15 => WireEvent::Ping {
                seat: r.read_bits(4) as u8,
                kind: r.read_bits(3) as u8,
                at: pos(r),
                unit: r.read_u8(),
            },
            16 => WireEvent::Vote {
                team: r.read_bits(1) as u8,
                yes: r.read_bits(5) as u8,
                no: r.read_bits(5) as u8,
                open: r.read_bool(),
                passed: r.read_bool(),
            },
            17 => WireEvent::ZoneSpawn {
                id: r.read_bits(ZONE_BITS) as u8,
                generation: r.read_u8(),
                team: r.read_bits(1) as u8,
                source: r.read_bits(SOURCE_BITS) as u16,
                owner: r.read_u8(),
                follows: r.read_bool(),
                at: pos(r),
                radius: r.read_bits(11) as u16,
                next: read_when(r, now),
                period: r.read_bits(10) as u16,
                end: read_when(r, now),
            },
            18 => WireEvent::ZoneEnd { id: r.read_bits(ZONE_BITS) as u8, generation: r.read_u8() },
            19 => WireEvent::Shield {
                target: r.read_u8(),
                amount: r.read_bits(AMOUNT_BITS) as u16,
                from: r.read_bits(SOURCE_BITS) as u16,
            },
            _ => WireEvent::Nothing,
        };
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(e) }
    }
}

/// Writes a list of events in a snapshot of tick `now`: their count, then each (a reliable list
/// first writes its first id).
pub fn write_list(w: &mut BitWriter<'_>, now: u32, first_id: Option<u16>, events: &[WireEvent]) {
    let n = events.len().min(MAX_LIST);
    w.write_bits(n as u32, COUNT_BITS);
    if let Some(id) = first_id {
        w.write_u16(id);
    }
    for e in &events[..n] {
        e.write(w, now);
    }
}

/// Bits [`write_list`] takes for these events.
pub fn list_bits(now: u32, reliable: bool, events: &[WireEvent]) -> usize {
    COUNT_BITS as usize + if reliable { 16 } else { 0 } + events.iter().map(|e| e.bits(now)).sum::<usize>()
}

/// Reads a list into `out`: the first id (reliable lists) and how many.
pub fn read_list(
    r: &mut BitReader<'_>,
    now: u32,
    reliable: bool,
    out: &mut [WireEvent; MAX_LIST],
) -> Result<(u16, usize), DecodeError> {
    let n = r.read_bits(COUNT_BITS) as usize;
    let first = if reliable { r.read_u16() } else { 0 };
    for e in out.iter_mut().take(n) {
        *e = WireEvent::read(r, now)?;
    }
    if r.overflowed() { Err(DecodeError::Truncated) } else { Ok((first, n)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_234_567;

    #[test]
    fn every_event_round_trips_and_says_its_size() {
        let all = [
            WireEvent::Leave { slot: 40, generation: 3, tick: 99_999 },
            WireEvent::Leave { slot: 41, generation: 3, tick: NOW - 31 },
            WireEvent::Leave { slot: 42, generation: 3, tick: NOW - 32 },
            WireEvent::Leave { slot: 43, generation: 3, tick: NOW + 1 },
            WireEvent::MissileSpawn {
                id: 511,
                generation: 200,
                team: 1,
                source: 0x123,
                owner: 255,
                origin: [70_000, 20_000],
                t0: NOW - 2,
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
            WireEvent::ZoneSpawn {
                id: 63,
                generation: 9,
                team: 1,
                source: 0x2FF,
                owner: 255,
                follows: true,
                at: [70_000, 20_000],
                radius: 400,
                next: NOW + 45,
                period: 15,
                end: NOW + 511,
            },
            WireEvent::ZoneSpawn {
                id: 0,
                generation: 1,
                team: 0,
                source: 0x1A3,
                owner: 4,
                follows: false,
                at: [1, 2],
                radius: 2_047,
                next: NOW - 512,
                period: 0,
                end: NOW + 512,
            },
            WireEvent::ZoneEnd { id: 63, generation: 255 },
            WireEvent::Damage { target: 1, source: 2, amount: 1_234, kind: 2, from: 0x19D },
            WireEvent::Heal { target: 1, amount: 50, from: 0 },
            WireEvent::Shield { target: 3, amount: 16_383, from: 0x205 },
            WireEvent::Attack { unit: 30, target: 2 },
            WireEvent::Area {
                zone: 12,
                generation: 3,
                at: [100, 200],
                radius: 350,
                team: 0,
                source: 0x101,
                owner: 255,
                follows: false,
            },
            WireEvent::Blink { unit: 4, to: [1, 2] },
            WireEvent::Ping { seat: 9, kind: 5, at: [3, 4], unit: 255 },
            WireEvent::Vote { team: 1, yes: 0b10110, no: 0b00001, open: true, passed: false },
        ];
        for e in all {
            let mut buf = [0u8; 64];
            let mut w = BitWriter::new(&mut buf);
            e.write(&mut w, NOW);
            assert_eq!(w.bits_written(), e.bits(NOW), "{e:?}");
            assert_eq!(WireEvent::read(&mut BitReader::new(&buf), NOW).unwrap(), e);
        }
        let mut buf = [0u8; 512];
        let mut w = BitWriter::new(&mut buf);
        write_list(&mut w, NOW, Some(65_530), &all);
        assert_eq!(w.bits_written(), list_bits(NOW, true, &all));
        let mut out = [WireEvent::Nothing; MAX_LIST];
        let (first, n) = read_list(&mut BitReader::new(&buf), NOW, true, &mut out).unwrap();
        assert_eq!((first, n), (65_530, all.len()));
        assert_eq!(&out[..n], &all);
    }
}
