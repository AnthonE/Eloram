//! The hot-path contract for the runtime: a shard's tick (the input drain, the step, replication,
//! the datagrams queued) makes not one heap operation, with players seated over lossy links and
//! the match in full swing.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use common::{BAD, Rig, bot_brain, walker};
use el_alloc::CountingAlloc;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn a_shard_ticks_without_allocating() {
    let mut rig = Rig::new(21, vec![(0, BAD, bot_brain()), (3, BAD, walker(3)), (5, BAD, bot_brain())]);
    // Warm up: waves out, fights begun.
    rig.run(60.0);
    rig.heap_ops = 0;
    let from = rig.ticks;
    rig.run(180.0);
    let ticks = rig.ticks - from;
    let m = rig.shared.metrics.snapshots.load(std::sync::atomic::Ordering::Relaxed);
    assert!(ticks > 5_000, "{ticks} ticks");
    assert!(m > 15_000, "{m} snapshots sent");
    assert_eq!(rig.heap_ops, 0, "heap operations inside {ticks} shard ticks");
}
