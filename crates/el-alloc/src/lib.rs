//! Counting global allocator.
//!
//! Tests install [`CountingAlloc`] as the `#[global_allocator]` and wrap the code under test in
//! [`count`] to prove that a region performs zero heap operations. This works in release builds too,
//! unlike debug-only guards.
//!
//! The server installs it as well and marks each match tick as a hot region with [`set_hot`]. Any
//! heap operation inside a hot region increments [`violations`], which `/status` reports. It does
//! not panic, because panicking inside an allocator would abort the process.
//!
//! Tracking is per thread. A const-initialised thread-local `Cell` never allocates, so checking the
//! flag is free when tracking is off.

// GlobalAlloc is an unsafe trait; this is the only crate in the workspace that needs `unsafe`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Wraps the system allocator and counts allocations, reallocations and frees made while the
/// current thread is inside a hot region.
pub struct CountingAlloc;

thread_local! {
    static HOT: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

static VIOLATIONS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn note() {
    // `try_with`: the allocator may run while thread-locals are being torn down.
    let hot = HOT.try_with(Cell::get).unwrap_or(false);
    if hot {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the caller's arguments unchanged; the only extra
// work is bumping thread-local and atomic counters, which never allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded verbatim; the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded verbatim.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: forwarded verbatim; `ptr`/`layout` came from this allocator (i.e. from `System`).
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: forwarded verbatim; `ptr`/`layout` came from this allocator (i.e. from `System`).
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Runs `f` as a hot region on this thread and returns its result plus how many heap operations it
/// performed. It nests correctly inside an outer hot region.
pub fn count<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let was_hot = HOT.with(|h| h.replace(true));
    let before = COUNT.with(Cell::get);
    let out = f();
    let after = COUNT.with(Cell::get);
    HOT.with(|h| h.set(was_hot));
    (out, after - before)
}

/// Marks the current thread as inside (`true`) or outside (`false`) a hot region. The shard runtime
/// calls this around every tick.
#[inline]
pub fn set_hot(on: bool) {
    let _ = HOT.try_with(|h| h.set(on));
}

/// Total heap operations seen inside hot regions, on any thread, since process start.
#[inline]
pub fn violations() -> u64 {
    VIOLATIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[global_allocator]
    static A: CountingAlloc = CountingAlloc;

    #[test]
    fn counts_allocations_only_inside_region() {
        let v: Vec<u32> = Vec::with_capacity(4); // outside: not counted
        let ((), n) = count(|| {
            let mut x = 0u64;
            for i in 0..1000u64 {
                x = x.wrapping_add(i);
            }
            std::hint::black_box(x);
        });
        assert_eq!(n, 0);
        let (b, n) = count(|| Box::new(7u64));
        assert_eq!(*b, 7);
        assert_eq!(n, 1);
        drop(v);
    }
}
