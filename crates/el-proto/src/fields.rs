//! A fixed list of fields coded against a baseline, losslessly: how the owner's own champion
//! travels (`el_sim::own`). The fields go in groups of [`GROUP`]: a bit for a group that says what
//! the baseline did, else a bit a field, and each changed field's value. A narrow field goes whole;
//! a 32-bit one (a float's bits, a tick) as its difference from the baseline's (XOR), its
//! significant bits after their count, since what changes in a tick is mostly its low bits.
//! Against a baseline of zeros it is the whole list, a zero field at a bit or less.

use crate::{BitReader, BitWriter, DecodeError};

/// Fields a group.
pub const GROUP: usize = 8;
/// Bits of a 32-bit field's difference's count of significant bits, less one.
const SIG_BITS: u32 = 5;

fn changed_bits(cur: u32, base: u32, width: u8) -> usize {
    if width == 32 { (SIG_BITS + (32 - (cur ^ base).leading_zeros())) as usize } else { usize::from(width) }
}

/// Bits [`write`] takes.
pub fn bits(cur: &[u32], base: &[u32], widths: &[u8]) -> usize {
    let mut n = 0;
    for g in (0..cur.len()).step_by(GROUP) {
        let end = (g + GROUP).min(cur.len());
        n += 1;
        if cur[g..end] != base[g..end] {
            for k in g..end {
                n += 1 + if cur[k] != base[k] { changed_bits(cur[k], base[k], widths[k]) } else { 0 };
            }
        }
    }
    n
}

/// Writes `cur` against `base` (all three of one length; a field's value fits its width).
pub fn write(w: &mut BitWriter<'_>, cur: &[u32], base: &[u32], widths: &[u8]) {
    for g in (0..cur.len()).step_by(GROUP) {
        let end = (g + GROUP).min(cur.len());
        let same = cur[g..end] == base[g..end];
        w.write_bool(same);
        if same {
            continue;
        }
        for k in g..end {
            let changed = cur[k] != base[k];
            w.write_bool(changed);
            if !changed {
                continue;
            }
            if widths[k] == 32 {
                let x = cur[k] ^ base[k];
                let sig = 32 - x.leading_zeros();
                w.write_bits(sig - 1, SIG_BITS);
                w.write_bits(x, sig);
            } else {
                w.write_bits(cur[k], u32::from(widths[k]));
            }
        }
    }
}

/// Reads what [`write`] wrote against the same `base` into `out`.
pub fn read(r: &mut BitReader<'_>, base: &[u32], widths: &[u8], out: &mut [u32]) -> Result<(), DecodeError> {
    out.copy_from_slice(base);
    for g in (0..out.len()).step_by(GROUP) {
        let end = (g + GROUP).min(out.len());
        if r.read_bool() {
            continue;
        }
        for k in g..end {
            if !r.read_bool() {
                continue;
            }
            out[k] = if widths[k] == 32 {
                let sig = r.read_bits(SIG_BITS) + 1;
                base[k] ^ r.read_bits(sig)
            } else {
                r.read_bits(u32::from(widths[k]))
            };
        }
    }
    if r.overflowed() { Err(DecodeError::Truncated) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: [u8; 19] = [1, 16, 32, 32, 32, 3, 3, 8, 32, 32, 32, 32, 32, 5, 5, 32, 32, 8, 32];

    fn round_trip(cur: &[u32], base: &[u32]) -> usize {
        let mut buf = [0u8; 256];
        let mut w = BitWriter::new(&mut buf);
        write(&mut w, cur, base, &WIDTHS);
        let written = w.bits_written();
        assert_eq!(written, bits(cur, base, &WIDTHS), "bits counts what's written");
        let mut out = [0u32; 19];
        read(&mut BitReader::new(&buf), base, &WIDTHS, &mut out).unwrap();
        assert_eq!(out, cur, "rebuilt to the bit");
        written
    }

    #[test]
    fn fields_round_trip_against_any_baseline() {
        let zero = [0u32; 19];
        let mut cur = [0u32; 19];
        for (k, v) in cur.iter_mut().enumerate() {
            let w = WIDTHS[k];
            *v = if w == 32 { 5_000.25f32.to_bits().rotate_left(k as u32) } else { (1 << w) - 1 };
        }
        let whole = round_trip(&cur, &zero);
        assert_eq!(round_trip(&zero, &zero), 3, "nothing: a bit a group");
        assert_eq!(round_trip(&cur, &cur), 3);
        // A tick of walking: a position's two floats move a little.
        let mut next = cur;
        next[2] = f32::from_bits(cur[2]).to_bits() ^ 0x3F;
        next[3] = (f32::from_bits(cur[3]) + 10.0).to_bits();
        let walk = round_trip(&next, &cur);
        assert!(walk < 64, "{walk} bits");
        assert!(walk * 6 < whole, "{walk} against {whole} whole");
        // Every field changed, every way.
        let mut all = cur;
        for (k, v) in all.iter_mut().enumerate() {
            *v = if WIDTHS[k] == 32 { !*v } else { 0 };
        }
        round_trip(&all, &cur);
        round_trip(&cur, &all);
    }

    #[test]
    fn a_cut_list_is_refused() {
        let cur = [7u32; 19];
        let mut buf = [0u8; 256];
        let mut w = BitWriter::new(&mut buf);
        write(&mut w, &cur, &[0; 19], &WIDTHS);
        let n = w.bytes_written();
        let mut out = [0u32; 19];
        for cut in 0..n - 1 {
            assert!(
                read(&mut BitReader::new(&buf[..cut]), &[0; 19], &WIDTHS, &mut out).is_err(),
                "cut at {cut}"
            );
        }
    }
}
