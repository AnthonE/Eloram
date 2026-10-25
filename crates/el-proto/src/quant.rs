//! Quantization: floats ↔ fixed-width integers.
//!
//! Every quantizer rounds to the nearest step, so a round trip is within half a step (½ LSB). The
//! proptest suite checks this. The map is flat: a position is two numbers over the map's extent.

use glam::Vec2;

use crate::{BitReader, BitWriter, MAP_H, MAP_W};

#[inline]
fn max_q(bits: u32) -> u32 {
    if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 }
}

/// Maps `v ∈ [-max_abs, max_abs]` onto `[0, 2^bits - 1]` (values outside are clamped).
///
/// Computed in f64, so the rounding is the same whatever the target's f32 intermediates.
#[inline]
pub fn quantize_signed(v: f32, max_abs: f32, bits: u32) -> u32 {
    let m = f64::from(max_q(bits));
    let max_abs = f64::from(max_abs);
    let t = ((f64::from(v) + max_abs) / (2.0 * max_abs)).clamp(0.0, 1.0);
    libm::round(t * m) as u32
}

#[inline]
pub fn dequantize_signed(q: u32, max_abs: f32, bits: u32) -> f32 {
    let m = f64::from(max_q(bits));
    let max_abs = f64::from(max_abs);
    ((f64::from(q.min(max_q(bits))) / m) * (2.0 * max_abs) - max_abs) as f32
}

/// Step size (LSB) of [`quantize_signed`].
#[inline]
pub fn signed_step(max_abs: f32, bits: u32) -> f32 {
    2.0 * max_abs / max_q(bits) as f32
}

/// Maps `v ∈ [-max_abs, max_abs]` onto the signed integers `±(2^(bits-1) - 1)` (values outside are
/// clamped), written in two's complement. Unlike [`quantize_signed`], whose grid has no middle
/// value, zero comes back exactly zero: what is at rest stays at rest.
#[inline]
pub fn quantize_centered(v: f32, max_abs: f32, bits: u32) -> i32 {
    let m = f64::from(max_q(bits - 1));
    let t = (f64::from(v) / f64::from(max_abs)).clamp(-1.0, 1.0);
    libm::round(t * m) as i32
}

#[inline]
pub fn dequantize_centered(q: i32, max_abs: f32, bits: u32) -> f32 {
    let m = f64::from(max_q(bits - 1));
    // (A hostile -2^(bits-1) reads as the end of the range.)
    ((f64::from(q).clamp(-m, m) / m) * f64::from(max_abs)) as f32
}

/// Step size (LSB) of [`quantize_centered`].
#[inline]
pub fn centered_step(max_abs: f32, bits: u32) -> f32 {
    max_abs / max_q(bits - 1) as f32
}

/// Maps `v ∈ [0, 1]` onto `[0, 2^bits - 1]`.
#[inline]
pub fn quantize_unit(v: f32, bits: u32) -> u32 {
    libm::round(f64::from(v.clamp(0.0, 1.0)) * f64::from(max_q(bits))) as u32
}

#[inline]
pub fn dequantize_unit(q: u32, bits: u32) -> f32 {
    (f64::from(q.min(max_q(bits))) / f64::from(max_q(bits))) as f32
}

// ---------------------------------------------------------------------------------------------
// Positions on the map
// ---------------------------------------------------------------------------------------------

/// Bits for a position along the map (x): 15 360 units / 2¹⁷ ≈ 0.12 units.
pub const POS_X_BITS: u32 = 17;
/// Bits for a position across it (y): 3 840 units / 2¹⁵ ≈ 0.12 units.
pub const POS_Y_BITS: u32 = 15;

/// A position's codes on the map's grid: what both ends agree on to the bit.
#[inline]
pub fn pos_code(p: Vec2) -> [u32; 2] {
    [quantize_unit(p.x / MAP_W, POS_X_BITS), quantize_unit(p.y / MAP_H, POS_Y_BITS)]
}

/// The position a [`pos_code`] stands for.
#[inline]
pub fn pos_from_code(c: [u32; 2]) -> Vec2 {
    Vec2::new(dequantize_unit(c[0], POS_X_BITS) * MAP_W, dequantize_unit(c[1], POS_Y_BITS) * MAP_H)
}

/// A position as it will arrive: its codes decoded. Quantizing it again changes nothing.
#[inline]
pub fn quantize_pos(p: Vec2) -> Vec2 {
    pos_from_code(pos_code(p))
}

pub fn write_pos(w: &mut BitWriter<'_>, p: Vec2) {
    let [x, y] = pos_code(p);
    w.write_bits(x, POS_X_BITS);
    w.write_bits(y, POS_Y_BITS);
}

pub fn read_pos(r: &mut BitReader<'_>) -> Vec2 {
    let x = r.read_bits(POS_X_BITS);
    let y = r.read_bits(POS_Y_BITS);
    pos_from_code([x, y])
}

/// Bits for a facing: 1/1024 of a turn.
pub const FACING_BITS: u32 = 10;

/// A facing angle (radians, any) as its code.
#[inline]
pub fn facing_code(angle: f32) -> u32 {
    let turns = f64::from(angle) / core::f64::consts::TAU;
    let frac = turns - libm::floor(turns);
    (libm::round(frac * f64::from(1u32 << FACING_BITS)) as u32) & max_q(FACING_BITS)
}

/// The angle a [`facing_code`] stands for, in `[0, TAU)`.
#[inline]
pub fn facing_from_code(c: u32) -> f32 {
    (f64::from(c & max_q(FACING_BITS)) / f64::from(1u32 << FACING_BITS) * core::f64::consts::TAU) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_round_trip_within_half_step() {
        for bits in [8u32, 10, 14, 16, 21] {
            let step = signed_step(100.0, bits);
            let mut v = -100.0f32;
            while v <= 100.0 {
                let back = dequantize_signed(quantize_signed(v, 100.0, bits), 100.0, bits);
                assert!((back - v).abs() <= step * 0.5 + 1e-4, "bits {bits} v {v} back {back}");
                v += 0.37;
            }
        }
    }

    #[test]
    fn centered_round_trip_within_half_step_and_zero_is_exact() {
        for bits in [8u32, 10, 14] {
            let step = centered_step(32.0, bits);
            let mut v = -32.0f32;
            while v <= 32.0 {
                let back = dequantize_centered(quantize_centered(v, 32.0, bits), 32.0, bits);
                assert!((back - v).abs() <= step * 0.5 + 1e-5, "bits {bits} v {v} back {back}");
                v += 0.037;
            }
            assert_eq!(dequantize_centered(quantize_centered(0.0, 32.0, bits), 32.0, bits).to_bits(), 0);
            assert_eq!(dequantize_centered(quantize_centered(-1e-4, 32.0, bits), 32.0, bits), 0.0);
            // Out of range clamps, both ways, and the one code past the end reads as the end.
            assert_eq!(dequantize_centered(quantize_centered(99.0, 32.0, bits), 32.0, bits), 32.0);
            assert_eq!(dequantize_centered(quantize_centered(-99.0, 32.0, bits), 32.0, bits), -32.0);
            assert_eq!(dequantize_centered(-(1 << (bits - 1)), 32.0, bits), -32.0);
        }
    }

    #[test]
    fn a_position_survives_the_wire_within_half_a_step_and_is_its_own_quantization() {
        let step = [MAP_W / max_q(POS_X_BITS) as f32, MAP_H / max_q(POS_Y_BITS) as f32];
        let mut x = 0.0f32;
        while x <= MAP_W {
            let mut y = 0.0f32;
            while y <= MAP_H {
                let p = Vec2::new(x, y);
                let mut buf = [0u8; 8];
                let mut w = BitWriter::new(&mut buf);
                write_pos(&mut w, p);
                assert_eq!(w.bits_written() as u32, POS_X_BITS + POS_Y_BITS);
                let back = read_pos(&mut BitReader::new(&buf));
                assert!((back.x - x).abs() <= step[0] * 0.5 + 1e-3, "{p} -> {back}");
                assert!((back.y - y).abs() <= step[1] * 0.5 + 1e-3, "{p} -> {back}");
                assert_eq!(
                    quantize_pos(back).to_array().map(f32::to_bits),
                    back.to_array().map(f32::to_bits)
                );
                y += 97.3;
            }
            x += 211.7;
        }
    }

    #[test]
    fn facings_wrap_and_are_their_own_quantization() {
        assert_eq!(facing_code(0.0), 0);
        assert_eq!(facing_code(core::f32::consts::TAU), 0);
        assert_eq!(facing_code(-core::f32::consts::FRAC_PI_2), 768);
        for c in 0..(1u32 << FACING_BITS) {
            assert_eq!(facing_code(facing_from_code(c)), c);
        }
    }
}
