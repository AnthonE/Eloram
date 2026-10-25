//! Everyone else, drawn a few ticks behind the server (`docs/NETCODE.md`, "Everyone else is
//! interpolated"). A unit's record is sent only when it says something its last one didn't: one at
//! rest, or walking straight, isn't sent again. So between two records a unit is carried on its
//! velocity, as the server took it to be, and blended into the next record over its last few
//! ticks; past its newest it is carried on, to the corner of its path at most, never through it.

use el_proto::quant::{dequantize_centered, facing_from_code, pos_from_code};
use el_proto::unit::{QUnit, VEL_BITS, VEL_MAX};
use glam::Vec2;

/// Records a track keeps.
const HELD: usize = 4;
/// The furthest a unit is carried past its newest record, ticks.
pub const CARRY_MAX: f64 = 45.0;
/// Ticks over which a unit is blended into its next record.
const BLEND: f64 = 3.0;
/// A unit this far (units) from where it was carried to has jumped (a blink, a respawn): it is drawn
/// at one place and then the other, never sliding between.
const JUMP: f32 = 300.0;

/// Where a record's unit is `dt` ticks after it, on its velocity, stopping at its path's corner.
pub fn carry(q: &QUnit, dt: f64) -> Vec2 {
    let pos = pos_from_code(q.pos);
    let vel = Vec2::new(
        dequantize_centered(q.vel[0], VEL_MAX, VEL_BITS),
        dequantize_centered(q.vel[1], VEL_MAX, VEL_BITS),
    );
    let step = vel * dt.max(0.0) as f32;
    if let Some(c) = q.corner {
        let to = pos_from_code(c) - pos;
        if step.dot(to) > 0.0 && step.length_squared() > to.length_squared() {
            return pos + to;
        }
    }
    pos + step
}

fn lerp_angle(a: f32, b: f32, w: f32) -> f32 {
    let tau = core::f32::consts::TAU;
    let d = (b - a + core::f32::consts::PI).rem_euclid(tau) - core::f32::consts::PI;
    a + d * w
}

/// A unit as it's drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub pos: Vec2,
    pub facing: f32,
    /// The newest record at or before the time drawn (its health, flags, level: what isn't blended).
    pub q: QUnit,
}

/// One slot's newest records, oldest first.
#[derive(Clone, Copy, Debug, Default)]
pub struct Track {
    recs: [(u32, QUnit); HELD],
    n: u8,
    /// The team sees it now (it hasn't left sight or died since its newest record).
    pub known: bool,
}

impl Track {
    /// Takes a record of the tick `tick`.
    pub fn push(&mut self, tick: u32, q: QUnit) {
        self.known = true;
        let n = usize::from(self.n);
        if n > 0 {
            let (last_t, last) = self.recs[n - 1];
            if !q.same_unit(&last) {
                // Another life in the slot: nothing of the last one is blended into it.
                self.recs[0] = (tick, q);
                self.n = 1;
                return;
            }
            if tick <= last_t {
                self.recs[n - 1] = (tick, q);
                return;
            }
        }
        if n == HELD {
            self.recs.copy_within(1.., 0);
            self.recs[HELD - 1] = (tick, q);
        } else {
            self.recs[n] = (tick, q);
            self.n += 1;
        }
    }

    pub fn newest(&self) -> Option<(u32, QUnit)> {
        (self.n > 0).then(|| self.recs[usize::from(self.n) - 1])
    }

    /// Forgets it (left sight, or died).
    pub fn forget(&mut self) {
        self.known = false;
        self.n = 0;
    }

    /// The unit at tick `at` (fractional).
    pub fn sample(&self, at: f64) -> Option<Sample> {
        let recs = &self.recs[..usize::from(self.n)];
        let first = recs.first()?;
        let Some(k) = recs.iter().rposition(|(t, _)| f64::from(*t) <= at) else {
            return Some(Sample {
                pos: pos_from_code(first.1.pos),
                facing: facing_from_code(first.1.facing),
                q: first.1,
            });
        };
        let (ta, a) = recs[k];
        let since = at - f64::from(ta);
        let facing_a = facing_from_code(a.facing);
        let Some(&(tb, b)) = recs.get(k + 1) else {
            return Some(Sample { pos: carry(&a, since.min(CARRY_MAX)), facing: facing_a, q: a });
        };
        let gap = f64::from(tb - ta);
        let pb = pos_from_code(b.pos);
        if carry(&a, gap).distance(pb) > JUMP {
            return Some(Sample { pos: carry(&a, since), facing: facing_a, q: a });
        }
        let start = gap - BLEND.min(gap);
        let pa = carry(&a, since.min(start));
        let w = if since <= start { 0.0 } else { ((since - start) / (gap - start)) as f32 };
        let facing = lerp_angle(facing_a, facing_from_code(b.facing), w);
        Some(Sample { pos: pa.lerp(pb, w), facing, q: a })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::quant::{pos_code, quantize_centered};

    fn rec(pos: Vec2, vel: Vec2) -> QUnit {
        QUnit {
            slot: 30,
            kind: 3,
            pos: pos_code(pos),
            vel: [quantize_centered(vel.x, VEL_MAX, VEL_BITS), quantize_centered(vel.y, VEL_MAX, VEL_BITS)],
            ..QUnit::default()
        }
    }

    #[test]
    fn a_unit_walking_straight_is_carried_between_its_records() {
        let mut t = Track::default();
        let v = Vec2::new(10.0, 0.0);
        t.push(100, rec(Vec2::new(1_000.0, 1_000.0), v));
        // No record for 20 ticks (it was "the same"), then one where it was carried to.
        t.push(120, rec(Vec2::new(1_200.0, 1_000.0), v));
        for k in 0..=20 {
            let at = 100.0 + f64::from(k);
            let p = t.sample(at).unwrap().pos;
            assert!((p.x - (1_000.0 + 10.0 * k as f32)).abs() < 2.0, "at {at}: {p}");
        }
        // Past the newest: carried on, as far as the cap.
        let p = t.sample(130.0).unwrap().pos;
        assert!((p.x - 1_300.0).abs() < 2.0, "{p}");
        let far = t.sample(1_000.0).unwrap().pos;
        assert!((far.x - (1_200.0 + 10.0 * CARRY_MAX as f32)).abs() < 2.0, "{far}");
    }

    #[test]
    fn carried_no_further_than_the_corner() {
        let mut q = rec(Vec2::new(1_000.0, 1_000.0), Vec2::new(10.0, 0.0));
        q.corner = Some(pos_code(Vec2::new(1_050.0, 1_000.0)));
        let p = carry(&q, 20.0);
        assert!((p - Vec2::new(1_050.0, 1_000.0)).length() < 2.0, "{p}");
        assert!((carry(&q, 2.0).x - 1_020.0).abs() < 2.0);
    }

    #[test]
    fn a_jump_is_drawn_as_a_jump() {
        let mut t = Track::default();
        t.push(100, rec(Vec2::new(1_000.0, 1_000.0), Vec2::ZERO));
        t.push(102, rec(Vec2::new(1_800.0, 1_000.0), Vec2::ZERO));
        assert!((t.sample(101.5).unwrap().pos.x - 1_000.0).abs() < 2.0);
        assert!((t.sample(102.0).unwrap().pos.x - 1_800.0).abs() < 2.0);
    }

    #[test]
    fn a_new_life_starts_its_track_over() {
        let mut t = Track::default();
        t.push(100, rec(Vec2::new(1_000.0, 1_000.0), Vec2::ZERO));
        let mut q = rec(Vec2::new(5_000.0, 1_000.0), Vec2::ZERO);
        q.generation = 1;
        t.push(101, q);
        assert!((t.sample(100.5).unwrap().pos.x - 5_000.0).abs() < 2.0);
    }
}
