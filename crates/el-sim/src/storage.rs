//! The only place the simulation allocates: fixed-size storage created at construction.
//! Nothing here grows afterwards.

// This module is the sanctioned allocation site; the hot-path bans apply everywhere else.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

use alloc::boxed::Box;
use alloc::vec::Vec;

/// A boxed slice of `n` copies of `v`, allocated once.
pub fn boxed<T: Clone>(n: usize, v: T) -> Box<[T]> {
    let mut out: Vec<T> = Vec::with_capacity(n);
    out.resize(n, v);
    out.into_boxed_slice()
}

/// A fixed-capacity bitset. (`Default` is an empty set that owns no memory; it only exists so a
/// scratch set can be `mem::take`n out of a struct and put back.)
#[derive(Clone, Default)]
pub struct BitSet {
    words: Box<[u64]>,
}

impl BitSet {
    pub fn new(bits: usize) -> Self {
        Self { words: boxed(bits.div_ceil(64), 0u64) }
    }
    #[inline]
    pub fn get(&self, i: usize) -> bool {
        self.words[i >> 6] & (1u64 << (i & 63)) != 0
    }
    #[inline]
    pub fn set(&mut self, i: usize, v: bool) {
        let w = &mut self.words[i >> 6];
        if v {
            *w |= 1u64 << (i & 63);
        } else {
            *w &= !(1u64 << (i & 63));
        }
    }
    pub fn clear(&mut self) {
        self.words.fill(0);
    }
    pub fn copy_from(&mut self, other: &BitSet) {
        self.words.copy_from_slice(&other.words);
    }
    /// Iterates set bits in ascending order.
    pub fn iter(&self) -> BitIter<'_> {
        BitIter { words: &self.words, wi: 0, cur: self.words.first().copied().unwrap_or(0) }
    }
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// Iterator over the set bits of a [`BitSet`].
#[derive(Clone)]
pub struct BitIter<'a> {
    words: &'a [u64],
    wi: usize,
    cur: u64,
}

impl Iterator for BitIter<'_> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<usize> {
        loop {
            if self.cur != 0 {
                let tz = self.cur.trailing_zeros() as usize;
                self.cur &= self.cur - 1;
                return Some(self.wi * 64 + tz);
            }
            self.wi += 1;
            if self.wi >= self.words.len() {
                return None;
            }
            self.cur = self.words[self.wi];
        }
    }
}

/// A stack of free slot indices.
pub struct FreeList {
    stack: Box<[u16]>,
    len: usize,
}

impl FreeList {
    /// All `n` slots free; `pop` hands out low indices first.
    pub fn full(n: usize) -> Self {
        let mut stack = boxed(n, 0u16);
        for (i, s) in stack.iter_mut().enumerate() {
            *s = (n - 1 - i) as u16;
        }
        Self { stack, len: n }
    }
    /// Every slot free again, in place.
    pub fn refill(&mut self) {
        let n = self.stack.len();
        for (i, s) in self.stack.iter_mut().enumerate() {
            *s = (n - 1 - i) as u16;
        }
        self.len = n;
    }

    /// Becomes a copy of `other` (the same capacity), in place.
    pub fn copy_from(&mut self, other: &FreeList) {
        self.stack.copy_from_slice(&other.stack);
        self.len = other.len;
    }

    pub fn pop(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.stack[self.len])
    }
    pub fn push(&mut self, idx: u16) {
        if self.len < self.stack.len() {
            self.stack[self.len] = idx;
            self.len += 1;
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A fixed-capacity vector (no growth after construction).
pub struct FixedVec<T: Copy> {
    items: Box<[T]>,
    len: usize,
}

impl<T: Copy> FixedVec<T> {
    pub fn new(cap: usize, fill: T) -> Self {
        Self { items: boxed(cap, fill), len: 0 }
    }
    /// Appends; returns `false` (dropping the item) when full.
    #[inline]
    pub fn try_push(&mut self, v: T) -> bool {
        if self.len == self.items.len() {
            return false;
        }
        self.items[self.len] = v;
        self.len += 1;
        true
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Moves `v` to the heap for the life of the process: what every match shares (the map's grids)
/// is built once and lent out.
pub fn leak<T>(v: T) -> &'static T {
    Box::leak(Box::new(v))
}
