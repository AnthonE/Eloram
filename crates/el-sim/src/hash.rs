//! FNV-1a over the match's state: the digest the golden tests, the record and the referee compare.

/// An FNV-1a digest being built.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    #[inline]
    pub fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    #[inline]
    pub fn u64(&mut self, v: u64) {
        self.u32(v as u32);
        self.u32((v >> 32) as u32);
    }
    #[inline]
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    #[inline]
    pub fn vec2(&mut self, v: glam::Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
}
