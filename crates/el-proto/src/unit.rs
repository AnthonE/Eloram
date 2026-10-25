//! A unit's record on the wire, as the integer codes it travels as ([`QUnit`]): whole, or as a
//! delta against an earlier record of the same unit the client acked (Before Colony's
//! `entity.rs`, after Gaffer On Games' snapshot compression; `docs/NETCODE.md`, "The wire").
//!
//! Both ends hold codes, never floats, so a delta taken on the server is undone on the client to
//! the bit: what the client rebuilds is exactly what a whole record would have said.

use crate::bits::{BitSink, Packer};
use crate::quant::{POS_X_BITS, POS_Y_BITS};
use crate::{BitReader, BitWriter, DecodeError, MAP_H, MAP_W};

/// A delta record's age, how many ticks before the snapshot's its baseline is: a bit, then 1 to
/// [`BACK_SHORT`] in [`BACK_SHORT_BITS`] (a baseline a round trip old), or else up to [`BACK_MAX`]
/// in [`BACK_LONG_BITS`] (a unit that said nothing new for a while).
pub const BACK_SHORT_BITS: u32 = 4;
pub const BACK_SHORT: u32 = 1 << BACK_SHORT_BITS;
pub const BACK_LONG_BITS: u32 = 16;
pub const BACK_MAX: u32 = (1 << BACK_LONG_BITS) - 1;
/// The records of each unit the client keeps: the newest it took, however old. The server takes a
/// delta only against a record the client acked and of which it has sent fewer than this many
/// newer, so it is always one the client still holds; and it keeps every unit's records this many
/// ticks, to know what an ack acked.
pub const KEPT: usize = 16;
/// A unit's velocity: how far it moved in its last tick, over ±[`VEL_MAX`] units at [`VEL_BITS`]
/// on the centered grid (zero is exact: what stands, stands).
pub const VEL_BITS: u32 = 11;
pub const VEL_MAX: f32 = 64.0;
pub const FACING_BITS: u32 = crate::quant::FACING_BITS;
pub const HP_BITS: u32 = 10;
pub const MAX_HP_BITS: u32 = 14;
pub const MANA_BITS: u32 = 7;
pub const SHIELD_BITS: u32 = 11;
pub const LEVEL_BITS: u32 = 5;
pub const FLAG_BITS: u32 = 10;
pub const CAST_BITS: u32 = 3;
/// "No target".
pub const NO_TARGET: u8 = u8::MAX;

/// What a unit shows, a bit each.
pub mod flags {
    pub const STUNNED: u32 = 1 << 0;
    pub const ROOTED: u32 = 1 << 1;
    pub const SILENCED: u32 = 1 << 2;
    pub const AIRBORNE: u32 = 1 << 3;
    pub const SLOWED: u32 = 1 << 4;
    pub const DASHING: u32 = 1 << 5;
    /// Winding up an attack.
    pub const ATTACKING: u32 = 1 << 6;
    pub const SHIELDED: u32 = 1 << 7;
    /// Seen by its enemies whatever their sight says.
    pub const REVEALED: u32 = 1 << 8;
    /// Immune to crowd control (Combo Breaker).
    pub const IMMUNE: u32 = 1 << 9;
}

/// A unit's record as codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QUnit {
    pub slot: u8,
    /// The low byte of the unit's generation.
    pub generation: u8,
    pub kind: u8,
    pub team: u8,
    pub pos: [u32; 2],
    pub vel: [i32; 2],
    pub facing: u32,
    /// Health as a share of the maximum, 0 to 1023.
    pub hp: u32,
    pub max_hp: u32,
    /// Mana as a share of the maximum, 0 to 127 (0 for what has none).
    pub mana: u32,
    pub shield: u32,
    pub level: u32,
    pub flags: u32,
    /// The ability being cast: 0 none, else its slot + 1.
    pub cast: u32,
    /// What it's attacking: a slot, or [`NO_TARGET`].
    pub target: u8,
    /// The corner of its path it's walking to, if it has one: what lets a client's extrapolation
    /// turn the corner rather than run through a wall (League's waypoints, at one field).
    pub corner: Option<[u32; 2]>,
}

fn max_code(bits: u32) -> u32 {
    (1u32 << bits) - 1
}

fn signed_lim(bits: u32) -> i64 {
    (1i64 << (bits - 1)) - 1
}

impl QUnit {
    /// Bits of the whole record, the slot and the delta bit included.
    pub fn whole_bits(&self) -> usize {
        (8 + 1 + 8 + 4 + 1 + POS_X_BITS + POS_Y_BITS + 2 * VEL_BITS + FACING_BITS + HP_BITS + MAX_HP_BITS)
            as usize
            + (MANA_BITS + SHIELD_BITS + LEVEL_BITS + FLAG_BITS + CAST_BITS + 8 + 1) as usize
            + if self.corner.is_some() { (POS_X_BITS + POS_Y_BITS) as usize } else { 0 }
    }

    /// Writes the record after its slot and delta bit (`0`): everything.
    pub fn write_whole(&self, w: &mut BitWriter<'_>) {
        let mut p = Packer::new(w);
        p.write_bits(u32::from(self.generation), 8);
        p.write_bits(u32::from(self.kind), 4);
        p.write_bits(u32::from(self.team), 1);
        p.write_bits(self.pos[0], POS_X_BITS);
        p.write_bits(self.pos[1], POS_Y_BITS);
        p.write_i32(self.vel[0], VEL_BITS);
        p.write_i32(self.vel[1], VEL_BITS);
        p.write_bits(self.facing, FACING_BITS);
        p.write_bits(self.hp, HP_BITS);
        write_misc(&mut p, self);
        write_corner_value(&mut p, self.corner);
        p.finish();
    }

    /// Reads a whole record of `slot`, after its delta bit.
    pub fn read_whole(r: &mut BitReader<'_>, slot: u8) -> Self {
        let mut q = QUnit {
            slot,
            generation: r.read_bits(8) as u8,
            kind: r.read_bits(4) as u8,
            team: r.read_bits(1) as u8,
            pos: [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)],
            vel: [r.read_i32(VEL_BITS), r.read_i32(VEL_BITS)],
            facing: r.read_bits(FACING_BITS),
            hp: r.read_bits(HP_BITS),
            ..QUnit::default()
        };
        read_misc(r, &mut q);
        q.corner = read_corner_value(r);
        q
    }

    /// Its position codes `back` ticks on at its velocity: where a client takes a unit to be when
    /// no newer record of it came (one at rest, or moving straight, is not sent again).
    pub fn carried(&self, back: u32) -> [u32; 2] {
        let at = carried(self, self.vel, back);
        [at[0] as u32, at[1] as u32]
    }

    /// Whether it says nothing that `base`, `back` ticks older, didn't (what [`Delta::is_same`]
    /// says of a delta between them), with the cheap comparisons first.
    pub fn adds_nothing_to(&self, base: &QUnit, back: u32) -> bool {
        (1..=BACK_MAX).contains(&back)
            && self.same_unit(base)
            && self.vel == base.vel
            && self.facing == base.facing
            && self.hp == base.hp
            && self.corner == base.corner
            && misc_same(self, base)
            && carried(base, self.vel, back) == [i64::from(self.pos[0]), i64::from(self.pos[1])]
    }

    /// Whether a delta can be taken against `base`: the same unit, the same life.
    pub fn same_unit(&self, base: &QUnit) -> bool {
        self.slot == base.slot
            && self.generation == base.generation
            && self.kind == base.kind
            && self.team == base.team
    }
}

fn write_misc(w: &mut impl BitSink, q: &QUnit) {
    w.write_bits(q.max_hp.min(max_code(MAX_HP_BITS)), MAX_HP_BITS);
    w.write_bits(q.mana, MANA_BITS);
    w.write_bits(q.shield.min(max_code(SHIELD_BITS)), SHIELD_BITS);
    w.write_bits(q.level, LEVEL_BITS);
    w.write_bits(q.flags, FLAG_BITS);
    w.write_bits(q.cast, CAST_BITS);
    w.write_bits(u32::from(q.target), 8);
}

fn read_misc(r: &mut BitReader<'_>, q: &mut QUnit) {
    q.max_hp = r.read_bits(MAX_HP_BITS);
    q.mana = r.read_bits(MANA_BITS);
    q.shield = r.read_bits(SHIELD_BITS);
    q.level = r.read_bits(LEVEL_BITS);
    q.flags = r.read_bits(FLAG_BITS);
    q.cast = r.read_bits(CAST_BITS);
    q.target = r.read_bits(8) as u8;
}

fn misc_same(a: &QUnit, b: &QUnit) -> bool {
    (a.max_hp, a.mana, a.shield, a.level, a.flags, a.cast, a.target)
        == (b.max_hp, b.mana, b.shield, b.level, b.flags, b.cast, b.target)
}

fn write_corner_value(w: &mut impl BitSink, c: Option<[u32; 2]>) {
    w.write_bool(c.is_some());
    if let Some([x, y]) = c {
        w.write_bits(x, POS_X_BITS);
        w.write_bits(y, POS_Y_BITS);
    }
}

fn read_corner_value(r: &mut BitReader<'_>) -> Option<[u32; 2]> {
    r.read_bool().then(|| [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)])
}

// ---------------------------------------------------------------------------------------------
// Deltas
// ---------------------------------------------------------------------------------------------

/// How a delta carries a group of codes: unchanged, as small or medium differences, or whole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Same,
    Small,
    Medium,
    Whole,
}

impl Mode {
    fn from_code(c: u32) -> Self {
        match c & 3 {
            0 => Mode::Same,
            1 => Mode::Small,
            2 => Mode::Medium,
            _ => Mode::Whole,
        }
    }

    /// The narrowest mode `diffs` fit, given the small and medium widths in bits.
    fn of(diffs: &[i64; 2], small: u32, medium: u32) -> Self {
        let fits = |bits: u32| diffs.iter().all(|&d| (-signed_lim(bits)..=signed_lim(bits)).contains(&d));
        if diffs.iter().all(|&d| d == 0) {
            Mode::Same
        } else if fits(small) {
            Mode::Small
        } else if fits(medium) {
            Mode::Medium
        } else {
            Mode::Whole
        }
    }

    fn bits(self, n: u32, small: u32, medium: u32, whole: u32) -> u32 {
        2 + match self {
            Mode::Same => 0,
            Mode::Small => n * small,
            Mode::Medium => n * medium,
            Mode::Whole => whole,
        }
    }
}

const VEL_SMALL: u32 = 5;
const VEL_MEDIUM: u32 = 8;
const POS_SMALL: u32 = 6;
const POS_MEDIUM: u32 = 11;

/// Integer division rounding half away from zero.
fn div_round(n: i64, d: i64) -> i64 {
    if n >= 0 { (n + d / 2) / d } else { -((-n + d / 2) / d) }
}

/// Where `base`'s position codes come to after `back` ticks at the mean of its velocity and
/// `vel`, clamped to the grid. Integers only, alike on both ends.
fn carried(base: &QUnit, vel: [i32; 2], back: u32) -> [i64; 2] {
    let mv = i64::from(max_code(VEL_BITS - 1));
    let extents = [(MAP_W as i64, POS_X_BITS), (MAP_H as i64, POS_Y_BITS)];
    let mut out = [0i64; 2];
    for k in 0..2 {
        let (extent, bits) = extents[k];
        let mp = i64::from(max_code(bits));
        // A velocity code is VEL_MAX / mv units a tick; a position code extent / mp units. The mean
        // of the two velocities over `back` ticks: (a + b) / 2 × back.
        let sum = i64::from(base.vel[k]) + i64::from(vel[k]);
        let num = sum * (VEL_MAX as i64) * i64::from(back) * mp;
        let den = 2 * mv * extent;
        out[k] = (i64::from(base.pos[k]) + div_round(num, den)).clamp(0, mp);
    }
    out
}

/// A record's delta against a baseline, worked out once: what [`bits`](Self::bits) counts is what
/// [`write`](Self::write) writes.
#[derive(Clone, Copy, Debug)]
pub struct Delta {
    back: u16,
    vel: (Mode, [i64; 2]),
    pos: (Mode, [i64; 2]),
    cur: QUnit,
    facing_same: bool,
    hp_same: bool,
    misc_same: bool,
    corner: CornerChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CornerChange {
    Same,
    Gone,
    New,
}

impl Delta {
    /// `cur` against `base`, `back` ticks older (1 to [`BACK_MAX`]). `None` if it can't be: another
    /// unit or life, or a baseline too old.
    pub fn plan(cur: &QUnit, base: &QUnit, back: u32) -> Option<Self> {
        if !(1..=BACK_MAX).contains(&back) || !cur.same_unit(base) {
            return None;
        }
        let dv =
            [i64::from(cur.vel[0]) - i64::from(base.vel[0]), i64::from(cur.vel[1]) - i64::from(base.vel[1])];
        let at = carried(base, cur.vel, back);
        let dp = [i64::from(cur.pos[0]) - at[0], i64::from(cur.pos[1]) - at[1]];
        let corner = match (cur.corner, base.corner) {
            (a, b) if a == b => CornerChange::Same,
            (None, Some(_)) => CornerChange::Gone,
            _ => CornerChange::New,
        };
        Some(Self {
            back: back as u16,
            vel: (Mode::of(&dv, VEL_SMALL, VEL_MEDIUM), dv),
            pos: (Mode::of(&dp, POS_SMALL, POS_MEDIUM), dp),
            cur: *cur,
            facing_same: cur.facing == base.facing,
            hp_same: cur.hp == base.hp,
            misc_same: misc_same(cur, base),
            corner,
        })
    }

    /// Bits of the record, the slot and the delta bit included.
    pub fn bits(&self) -> usize {
        let misc = MAX_HP_BITS + MANA_BITS + SHIELD_BITS + LEVEL_BITS + FLAG_BITS + CAST_BITS + 8;
        (8 + 1
            + back_bits(u32::from(self.back))
            + self.vel.0.bits(2, VEL_SMALL, VEL_MEDIUM, 2 * VEL_BITS)
            + self.pos.0.bits(2, POS_SMALL, POS_MEDIUM, POS_X_BITS + POS_Y_BITS)
            + 1
            + if self.facing_same { 0 } else { FACING_BITS }
            + 1
            + if self.hp_same { 0 } else { HP_BITS }
            + 1
            + if self.misc_same { 0 } else { misc }
            + 2
            + if self.corner == CornerChange::New { POS_X_BITS + POS_Y_BITS } else { 0 }) as usize
    }

    /// Whether the record says nothing its baseline didn't (a unit at rest, unchanged).
    pub fn is_same(&self) -> bool {
        self.vel.0 == Mode::Same
            && self.pos.0 == Mode::Same
            && self.facing_same
            && self.hp_same
            && self.misc_same
            && self.corner == CornerChange::Same
    }

    /// Writes the record after its slot and delta bit (`1`).
    pub fn write(&self, w: &mut BitWriter<'_>) {
        let c = &self.cur;
        let mut packer = Packer::new(w);
        let w = &mut packer;
        let back = u32::from(self.back);
        w.write_bool(back > BACK_SHORT);
        if back > BACK_SHORT {
            w.write_bits(back, BACK_LONG_BITS);
        } else {
            w.write_bits(back - 1, BACK_SHORT_BITS);
        }
        write_group(w, self.vel, VEL_SMALL, VEL_MEDIUM, |w| {
            w.write_i32(c.vel[0], VEL_BITS);
            w.write_i32(c.vel[1], VEL_BITS);
        });
        write_group(w, self.pos, POS_SMALL, POS_MEDIUM, |w| {
            w.write_bits(c.pos[0], POS_X_BITS);
            w.write_bits(c.pos[1], POS_Y_BITS);
        });
        w.write_bool(self.facing_same);
        if !self.facing_same {
            w.write_bits(c.facing, FACING_BITS);
        }
        w.write_bool(self.hp_same);
        if !self.hp_same {
            w.write_bits(c.hp, HP_BITS);
        }
        w.write_bool(self.misc_same);
        if !self.misc_same {
            write_misc(w, c);
        }
        match self.corner {
            CornerChange::Same => w.write_bits(0, 2),
            CornerChange::Gone => w.write_bits(1, 2),
            CornerChange::New => {
                w.write_bits(2, 2);
                let [x, y] = c.corner.unwrap_or_default();
                w.write_bits(x, POS_X_BITS);
                w.write_bits(y, POS_Y_BITS);
            }
        }
        packer.finish();
    }

    /// Reads a delta record's age, after its slot and delta bit (0 only from a hostile writer).
    pub fn read_back(r: &mut BitReader<'_>) -> u32 {
        if r.read_bool() { r.read_bits(BACK_LONG_BITS) } else { r.read_bits(BACK_SHORT_BITS) + 1 }
    }

    /// Reads the rest of a delta record against `base`, `back` ticks older.
    pub fn read(r: &mut BitReader<'_>, base: &QUnit, back: u32) -> Result<QUnit, DecodeError> {
        let mut q = *base;
        let signed = |r: &mut BitReader<'_>, bits: u32| i64::from(r.read_i32(bits));
        let vlim = signed_lim(VEL_BITS);
        match Mode::from_code(r.read_bits(2)) {
            Mode::Same => {}
            Mode::Small => q
                .vel
                .iter_mut()
                .for_each(|v| *v = (i64::from(*v) + signed(r, VEL_SMALL)).clamp(-vlim, vlim) as i32),
            Mode::Medium => q
                .vel
                .iter_mut()
                .for_each(|v| *v = (i64::from(*v) + signed(r, VEL_MEDIUM)).clamp(-vlim, vlim) as i32),
            Mode::Whole => q.vel = [r.read_i32(VEL_BITS), r.read_i32(VEL_BITS)],
        }
        let at = carried(base, q.vel, back);
        let tops = [i64::from(max_code(POS_X_BITS)), i64::from(max_code(POS_Y_BITS))];
        match Mode::from_code(r.read_bits(2)) {
            Mode::Same => q.pos = [at[0] as u32, at[1] as u32],
            Mode::Small => {
                for k in 0..2 {
                    q.pos[k] = (at[k] + signed(r, POS_SMALL)).clamp(0, tops[k]) as u32;
                }
            }
            Mode::Medium => {
                for k in 0..2 {
                    q.pos[k] = (at[k] + signed(r, POS_MEDIUM)).clamp(0, tops[k]) as u32;
                }
            }
            Mode::Whole => q.pos = [r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)],
        }
        if !r.read_bool() {
            q.facing = r.read_bits(FACING_BITS);
        }
        if !r.read_bool() {
            q.hp = r.read_bits(HP_BITS);
        }
        if !r.read_bool() {
            read_misc(r, &mut q);
        }
        match r.read_bits(2) {
            0 => {}
            1 => q.corner = None,
            2 => q.corner = Some([r.read_bits(POS_X_BITS), r.read_bits(POS_Y_BITS)]),
            _ => return Err(DecodeError::Invalid),
        }
        if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(q) }
    }
}

/// Bits of a delta record's age.
fn back_bits(back: u32) -> u32 {
    1 + if back > BACK_SHORT { BACK_LONG_BITS } else { BACK_SHORT_BITS }
}

fn write_group(
    w: &mut Packer<'_, '_>,
    g: (Mode, [i64; 2]),
    small: u32,
    medium: u32,
    whole: impl FnOnce(&mut Packer<'_, '_>),
) {
    w.write_bits(g.0 as u32, 2);
    match g.0 {
        Mode::Same => {}
        Mode::Small => g.1.iter().for_each(|&d| w.write_i32(d as i32, small)),
        Mode::Medium => g.1.iter().for_each(|&d| w.write_i32(d as i32, medium)),
        Mode::Whole => whole(w),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quant::pos_code;
    use glam::Vec2;

    fn unit(x: f32, vx: i32) -> QUnit {
        QUnit {
            slot: 30,
            generation: 7,
            kind: 1,
            team: 1,
            pos: pos_code(Vec2::new(x, 1_920.0)),
            vel: [vx, 0],
            facing: 512,
            hp: 1023,
            max_hp: 477,
            level: 1,
            target: NO_TARGET,
            corner: Some(pos_code(Vec2::new(9_000.0, 1_900.0))),
            ..QUnit::default()
        }
    }

    fn round_trip(cur: &QUnit, base: &QUnit, back: u32) -> usize {
        let d = Delta::plan(cur, base, back).unwrap();
        let mut buf = [0u8; 64];
        let mut w = BitWriter::new(&mut buf);
        d.write(&mut w);
        assert_eq!(w.bits_written() + 9, d.bits(), "the plan counts what's written");
        let mut r = BitReader::new(&buf);
        let back_read = Delta::read_back(&mut r);
        assert_eq!(back_read, back);
        assert_eq!(&Delta::read(&mut r, base, back).unwrap(), cur, "rebuilt to the bit");
        d.bits()
    }

    #[test]
    fn a_whole_record_round_trips_and_says_its_size() {
        let q = unit(5_000.0, 81);
        let mut buf = [0u8; 64];
        let mut w = BitWriter::new(&mut buf);
        q.write_whole(&mut w);
        assert_eq!(w.bits_written() + 9, q.whole_bits());
        assert_eq!(QUnit::read_whole(&mut BitReader::new(&buf), 30), q);
    }

    #[test]
    fn a_unit_walking_steadily_costs_a_few_bits_and_one_at_rest_fewer() {
        // ~10.8 units a tick (325 a second) along x: 3 ticks on, it's where the velocity carried it.
        let v = (10.8 / VEL_MAX * max_code(VEL_BITS - 1) as f32).round() as i32;
        let base = unit(5_000.0, v);
        let mut cur = base;
        cur.pos = pos_code(Vec2::new(5_000.0 + 3.0 * 10.8, 1_920.0));
        let walking = round_trip(&cur, &base, 3);
        assert!(walking <= 36, "walking: {walking} bits");
        let still = unit(5_000.0, 0);
        let resting = round_trip(&still, &still, 5);
        assert!(resting <= 25, "at rest: {resting} bits");
        // A unit at rest a minute: its baseline is that old, at a dozen bits more.
        let long = round_trip(&still, &still, 1_800);
        assert!(long <= 37, "at rest a minute: {long} bits");
        assert!(Delta::plan(&still, &still, 5).unwrap().is_same());
    }

    #[test]
    fn adding_nothing_is_a_delta_that_says_nothing() {
        let v = (10.8 / VEL_MAX * max_code(VEL_BITS - 1) as f32).round() as i32;
        let base = unit(5_000.0, v);
        let mut cases = [base; 8];
        for (k, back) in [1u32, 3, 3, 20].into_iter().enumerate() {
            cases[k].pos = Delta::plan(&base, &base, back).map(|_| base.carried(back)).unwrap();
        }
        cases[2].pos[0] += 1;
        cases[4].hp = 3;
        cases[5].corner = None;
        cases[6].generation = 9;
        cases[7].flags = flags::SLOWED;
        for back in [1, 3, 20, 4_000] {
            for c in &cases {
                let same = Delta::plan(c, &base, back).is_some_and(|d| d.is_same());
                assert_eq!(c.adds_nothing_to(&base, back), same, "{c:?} back {back}");
            }
        }
    }

    #[test]
    fn every_change_survives_a_delta() {
        let base = unit(5_000.0, 40);
        let mut cur = unit(5_400.0, -300);
        cur.hp = 300;
        cur.flags = flags::STUNNED | flags::SLOWED;
        cur.cast = 3;
        cur.target = 4;
        cur.corner = None;
        cur.facing = 3;
        round_trip(&cur, &base, 9);
        cur.corner = Some([1, 2]);
        round_trip(&cur, &base, 200);
        for back in [1, BACK_SHORT, BACK_SHORT + 1, BACK_MAX] {
            round_trip(&cur, &base, back);
        }
        assert!(Delta::plan(&cur, &base, 0).is_none());
        assert!(Delta::plan(&cur, &base, BACK_MAX + 1).is_none());
        let mut other = cur;
        other.generation = 8;
        assert!(Delta::plan(&other, &base, 3).is_none(), "another life is never a delta");
    }
}
