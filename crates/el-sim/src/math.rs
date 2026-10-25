//! Deterministic math helpers.
//!
//! The server (native, `glam/scalar-math`) and the browser client (wasm32, scalar) must step the
//! match identically. IEEE `+ - * / sqrt` are exact everywhere; transcendental functions are not,
//! so every one we use goes through the pure-Rust `libm` crate here, never `std` or glam's internal
//! choice. Never `mul_add`: a fused multiply-add rounds once where the other target rounds twice.

use glam::Vec2;

#[inline]
pub fn sqrt(x: f32) -> f32 {
    libm::sqrtf(x)
}
#[inline]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}
#[inline]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}
#[inline]
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}
#[inline]
pub fn floor(x: f32) -> f32 {
    libm::floorf(x)
}
#[inline]
pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}

/// Length that never divides by zero.
#[inline]
pub fn length(v: Vec2) -> f32 {
    sqrt(v.dot(v))
}

/// `|a - b|`.
#[inline]
pub fn distance(a: Vec2, b: Vec2) -> f32 {
    length(a - b)
}

/// `v / |v|`, or `fallback` for (near-)zero vectors.
#[inline]
pub fn normalize_or(v: Vec2, fallback: Vec2) -> Vec2 {
    let l2 = v.dot(v);
    if l2 > 1e-12 { v / sqrt(l2) } else { fallback }
}

/// The unit vector at `angle` radians from +x.
#[inline]
pub fn dir(angle: f32) -> Vec2 {
    Vec2::new(cos(angle), sin(angle))
}

/// The angle of `v` from +x, radians in (-π, π].
#[inline]
pub fn angle(v: Vec2) -> f32 {
    atan2(v.y, v.x)
}

/// `v`, shortened to length `max` if it's longer.
#[inline]
pub fn clamp_len(v: Vec2, max: f32) -> Vec2 {
    let l2 = v.dot(v);
    if l2 > max * max { v * (max / sqrt(l2)) } else { v }
}

/// From `from` toward `to` by at most `step`: `to` itself if it is that close.
#[inline]
pub fn move_toward(from: Vec2, to: Vec2, step: f32) -> Vec2 {
    let d = to - from;
    let l2 = d.dot(d);
    if l2 <= step * step { to } else { from + d * (step / sqrt(l2)) }
}

/// Small deterministic PRNG (xorshift64*), for noise that must match on every machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    /// Uniform in `0..n` (`n` > 0).
    pub fn below(&mut self, n: u32) -> u32 {
        ((u64::from(self.next_u32()) * u64::from(n)) >> 32) as u32
    }
    /// The generator's whole state, for a save.
    pub fn state(self) -> u64 {
        self.0
    }
    /// A generator restored from [`state`](Self::state).
    pub fn from_state(state: u64) -> Self {
        Self(state)
    }
}

/// Stateless hash → u32, for draws that must not depend on the order anything else happened in
/// (an augment's offers: the seed, the seat, the screen, the offer).
pub fn hash32(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    x
}

/// Stateless hash → [0, 1).
pub fn hash01(a: u32, b: u32) -> f32 {
    (hash32(a, b) >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_toward_stops_on_the_target() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(3.0, 4.0);
        assert_eq!(move_toward(a, b, 10.0), b);
        let half = move_toward(a, b, 2.5);
        assert!((half - Vec2::new(1.5, 2.0)).length() < 1e-6);
    }

    #[test]
    fn rng_below_stays_below_and_is_even() {
        let mut r = Rng::new(7);
        let mut counts = [0u32; 5];
        for _ in 0..50_000 {
            let k = r.below(5);
            counts[k as usize] += 1;
        }
        for c in counts {
            assert!((9_000..11_000).contains(&c), "{counts:?}");
        }
        let s = r.state();
        assert_eq!(Rng::from_state(s).next_u32(), r.next_u32());
    }

    #[test]
    fn hashes_spread() {
        let mut seen = [0u32; 16];
        for a in 0..64 {
            for b in 0..64 {
                seen[(hash32(a, b) >> 28) as usize] += 1;
            }
        }
        for c in seen {
            assert!((150..370).contains(&c), "{seen:?}");
        }
    }
}
