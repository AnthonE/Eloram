//! LSB-first bit packing over caller buffers.
//!
//! Overflowing writes and reads do not panic. The writer stops and records the overflow; the reader
//! returns zeros and records it. Callers check [`BitWriter::overflowed`] or [`BitReader::overflowed`]
//! once at the end, which keeps the hot encode path free of per-field error plumbing.

/// Writes bit fields into a borrowed byte buffer.
pub struct BitWriter<'a> {
    buf: &'a mut [u8],
    bit_pos: usize,
    limit_bits: usize,
    overflow: bool,
}

impl<'a> BitWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        let limit_bits = buf.len() * 8;
        Self { buf, bit_pos: 0, limit_bits, overflow: false }
    }

    /// Like [`new`](Self::new) but never writes past `max_bytes`.
    pub fn with_limit(buf: &'a mut [u8], max_bytes: usize) -> Self {
        let limit_bits = buf.len().min(max_bytes) * 8;
        Self { buf, bit_pos: 0, limit_bits, overflow: false }
    }

    /// Writes the low `bits` bits of `value` (`bits` ≤ 32).
    #[inline]
    pub fn write_bits(&mut self, value: u32, bits: u32) {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return;
        }
        if self.overflow || self.bit_pos + bits as usize > self.limit_bits {
            self.overflow = true;
            return;
        }
        // The field lands in at most five bytes (32 bits after up to 7 already used in the first):
        // merge it with the first byte's used bits as one word and store the word whole. What it
        // stores past the field is unwritten yet, so zeros there harm nothing.
        let byte = self.bit_pos >> 3;
        let offset = (self.bit_pos & 7) as u32;
        let used = if offset == 0 { 0 } else { u64::from(self.buf[byte]) & ((1u64 << offset) - 1) };
        let word = used | (u64::from(value) & ((1u64 << bits) - 1)) << offset;
        match self.buf.get_mut(byte..byte + 8) {
            Some(dst) => dst.copy_from_slice(&word.to_le_bytes()),
            None => {
                let n = (offset + bits).div_ceil(8) as usize;
                for (i, b) in self.buf[byte..byte + n].iter_mut().enumerate() {
                    *b = (word >> (8 * i)) as u8;
                }
            }
        }
        self.bit_pos += bits as usize;
    }

    #[inline]
    pub fn write_bool(&mut self, b: bool) {
        self.write_bits(u32::from(b), 1);
    }

    #[inline]
    pub fn write_u8(&mut self, v: u8) {
        self.write_bits(u32::from(v), 8);
    }

    #[inline]
    pub fn write_u16(&mut self, v: u16) {
        self.write_bits(u32::from(v), 16);
    }

    #[inline]
    pub fn write_u32(&mut self, v: u32) {
        self.write_bits(v, 32);
    }

    /// Raw IEEE-754 bits (lossless).
    #[inline]
    pub fn write_f32(&mut self, v: f32) {
        self.write_bits(v.to_bits(), 32);
    }

    /// Signed value in two's complement over `bits` bits.
    #[inline]
    pub fn write_i32(&mut self, v: i32, bits: u32) {
        self.write_bits(v as u32, bits);
    }

    pub fn bits_written(&self) -> usize {
        self.bit_pos
    }

    pub fn bits_remaining(&self) -> usize {
        self.limit_bits.saturating_sub(self.bit_pos)
    }

    /// Bytes touched so far (rounded up).
    pub fn bytes_written(&self) -> usize {
        self.bit_pos.div_ceil(8)
    }

    pub fn overflowed(&self) -> bool {
        self.overflow
    }

    /// Rewinds to an earlier position, e.g. to drop a partially written item.
    pub fn rewind(&mut self, bit_pos: usize) {
        debug_assert!(bit_pos <= self.bit_pos);
        self.bit_pos = bit_pos;
        // The byte at the new position may carry stale high bits; clear them so later writes
        // (which OR into partially filled bytes) stay correct.
        let offset = (bit_pos & 7) as u32;
        if offset != 0 {
            let byte = bit_pos >> 3;
            self.buf[byte] &= ((1u16 << offset) - 1) as u8;
        }
    }
}

/// Where bit fields go: a [`BitWriter`], or a [`Packer`] in front of one.
pub trait BitSink {
    /// Writes the low `bits` bits of `value` (`bits` ≤ 32).
    fn write_bits(&mut self, value: u32, bits: u32);

    fn write_bool(&mut self, b: bool) {
        self.write_bits(u32::from(b), 1);
    }

    /// Signed value in two's complement over `bits` bits.
    fn write_i32(&mut self, v: i32, bits: u32) {
        self.write_bits(v as u32, bits);
    }
}

impl BitSink for BitWriter<'_> {
    #[inline]
    fn write_bits(&mut self, value: u32, bits: u32) {
        BitWriter::write_bits(self, value, bits);
    }
}

/// Packs fields into a word before they go to a [`BitWriter`], so a record of many small fields
/// (a suit's delta: twenty-odd) is written a 32-bit word at a time. [`finish`](Self::finish) writes
/// what's left; until then the writer is behind by up to 31 bits.
pub struct Packer<'w, 'a> {
    w: &'w mut BitWriter<'a>,
    acc: u64,
    n: u32,
}

impl<'w, 'a> Packer<'w, 'a> {
    pub fn new(w: &'w mut BitWriter<'a>) -> Self {
        Self { w, acc: 0, n: 0 }
    }

    /// Writes what's packed and not yet written.
    pub fn finish(self) {
        self.w.write_bits(self.acc as u32, self.n);
    }
}

impl BitSink for Packer<'_, '_> {
    #[inline]
    fn write_bits(&mut self, value: u32, bits: u32) {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return;
        }
        // Fewer than 32 held, so at most 63 after this.
        self.acc |= (u64::from(value) & ((1u64 << bits) - 1)) << self.n;
        self.n += bits;
        if self.n >= 32 {
            self.w.write_bits(self.acc as u32, 32);
            self.acc >>= 32;
            self.n -= 32;
        }
    }
}

/// Reads bit fields from a borrowed byte slice.
pub struct BitReader<'a> {
    buf: &'a [u8],
    bit_pos: usize,
    overflow: bool,
}

impl<'a> BitReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, bit_pos: 0, overflow: false }
    }

    /// Reads `bits` bits (≤ 32). Past the end it returns 0 and marks the reader overflowed.
    #[inline]
    pub fn read_bits(&mut self, bits: u32) -> u32 {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return 0;
        }
        if self.overflow || self.bit_pos + bits as usize > self.buf.len() * 8 {
            self.overflow = true;
            return 0;
        }
        // As the writer: the field is in the word of the eight bytes from its first.
        let byte = self.bit_pos >> 3;
        let offset = (self.bit_pos & 7) as u32;
        let mut word = [0u8; 8];
        let src = &self.buf[byte..(byte + 8).min(self.buf.len())];
        word[..src.len()].copy_from_slice(src);
        self.bit_pos += bits as usize;
        ((u64::from_le_bytes(word) >> offset) & ((1u64 << bits) - 1)) as u32
    }

    #[inline]
    pub fn read_bool(&mut self) -> bool {
        self.read_bits(1) != 0
    }

    #[inline]
    pub fn read_u8(&mut self) -> u8 {
        self.read_bits(8) as u8
    }

    #[inline]
    pub fn read_u16(&mut self) -> u16 {
        self.read_bits(16) as u16
    }

    #[inline]
    pub fn read_u32(&mut self) -> u32 {
        self.read_bits(32)
    }

    #[inline]
    pub fn read_f32(&mut self) -> f32 {
        f32::from_bits(self.read_bits(32))
    }

    /// Two's-complement signed value over `bits` bits.
    #[inline]
    pub fn read_i32(&mut self, bits: u32) -> i32 {
        let raw = self.read_bits(bits);
        if bits == 32 {
            return raw as i32;
        }
        let shift = 32 - bits;
        ((raw << shift) as i32) >> shift
    }

    pub fn bits_read(&self) -> usize {
        self.bit_pos
    }

    pub fn bits_remaining(&self) -> usize {
        (self.buf.len() * 8).saturating_sub(self.bit_pos)
    }

    pub fn overflowed(&self) -> bool {
        self.overflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_mixed_widths() {
        let mut buf = [0xAAu8; 64];
        let mut w = BitWriter::new(&mut buf);
        w.write_bits(5, 3);
        w.write_bits(0xdead_beef, 32);
        w.write_bool(true);
        w.write_i32(-5, 7);
        w.write_f32(-1.25);
        w.write_bits(0x3ff, 10);
        assert!(!w.overflowed());
        let n = w.bytes_written();
        let mut r = BitReader::new(&buf[..n]);
        assert_eq!(r.read_bits(3), 5);
        assert_eq!(r.read_bits(32), 0xdead_beef);
        assert!(r.read_bool());
        assert_eq!(r.read_i32(7), -5);
        assert_eq!(r.read_f32(), -1.25);
        assert_eq!(r.read_bits(10), 0x3ff);
        assert!(!r.overflowed());
    }

    #[test]
    fn every_width_at_every_offset_round_trips_to_the_last_byte() {
        for len in [5usize, 9, 13] {
            for lead in 0..8u32 {
                for bits in 1..=32u32 {
                    if (lead + bits) as usize > len * 8 {
                        continue;
                    }
                    let mut buf = [0x5Au8; 16];
                    let v = 0x9E37_79B9u32.rotate_left(bits + lead) & (u32::MAX >> (32 - bits));
                    let mut w = BitWriter::new(&mut buf[..len]);
                    w.write_bits(0x7F, lead);
                    w.write_bits(v, bits);
                    assert!(!w.overflowed());
                    assert_eq!(buf[len..], [0x5A; 16][len..], "nothing past the buffer's end");
                    let mut r = BitReader::new(&buf[..len]);
                    let lead_back = if lead == 0 { 0 } else { 0x7F & (u32::MAX >> (32 - lead)) };
                    assert_eq!(r.read_bits(lead), lead_back);
                    assert_eq!(r.read_bits(bits), v, "{bits} bits after {lead} in {len} bytes");
                }
            }
        }
    }

    #[test]
    fn a_packer_writes_what_the_writer_would() {
        let fields: [(u32, u32); 9] = [
            (5, 3),
            (0xdead_beef, 32),
            (1, 1),
            (0x7f, 7),
            (0, 0),
            (0x1234, 13),
            (u32::MAX, 32),
            (3, 2),
            (9, 31),
        ];
        let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
        let mut wa = BitWriter::new(&mut a);
        wa.write_bits(1, 5);
        for (v, n) in fields {
            wa.write_bits(v, n);
        }
        let na = wa.bytes_written();
        let mut wb = BitWriter::new(&mut b);
        wb.write_bits(1, 5);
        let mut p = Packer::new(&mut wb);
        for (v, n) in fields {
            p.write_bits(v, n);
        }
        p.finish();
        assert_eq!((wb.bytes_written(), &b[..na]), (na, &a[..na]));
    }

    #[test]
    fn overflow_is_sticky_and_safe() {
        let mut buf = [0u8; 2];
        let mut w = BitWriter::new(&mut buf);
        w.write_bits(0xffff, 16);
        w.write_bits(1, 1);
        assert!(w.overflowed());
        let mut r = BitReader::new(&buf[..1]);
        assert_eq!(r.read_bits(16), 0);
        assert!(r.overflowed());
    }

    #[test]
    fn rewind_clears_partial_byte() {
        let mut buf = [0u8; 4];
        let mut w = BitWriter::new(&mut buf);
        w.write_bits(0b101, 3);
        let mark = w.bits_written();
        w.write_bits(0b11111, 5);
        w.rewind(mark);
        w.write_bits(0b00000, 5);
        w.write_bits(0xff, 8);
        let mut r = BitReader::new(&buf);
        assert_eq!(r.read_bits(3), 0b101);
        assert_eq!(r.read_bits(5), 0);
        assert_eq!(r.read_bits(8), 0xff);
    }
}
