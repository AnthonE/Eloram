//! The hot-path contract: after construction, a match (and the bots playing it) ticks without a
//! single heap operation.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]
#![cfg(not(target_arch = "wasm32"))]

mod common;

use el_alloc::CountingAlloc;
use el_sim::ticks;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn a_busy_match_ticks_without_allocating() {
    let (mut m, mut s) = common::bot_match(23);
    // Warm up outside the measured region: waves out, fights begun, shops visited.
    common::run(&mut m, &mut s, ticks(90.0));
    let mut total = 0;
    let mut ticks_run = 0;
    let mut deaths = 0;
    for _ in 0..ticks(240.0) {
        if m.outcome.is_some() {
            break;
        }
        let ((), n) = el_alloc::count(|| {
            let inputs = common::bot_inputs(&m);
            m.step(&inputs, &mut s);
        });
        deaths +=
            m.events.as_slice().iter().filter(|e| matches!(e, el_sim::events::Event::Death { .. })).count();
        total += n;
        ticks_run += 1;
    }
    assert!(ticks_run > 7_000, "{ticks_run} ticks");
    assert!(deaths > 100, "the match should be busy: {deaths} deaths");
    assert!(s.searches > 100, "paths were found: {}", s.searches);
    assert_eq!(total, 0, "heap operations inside the tick: {total}");
}

#[test]
fn resetting_a_match_allocates_nothing() {
    let (mut m, mut s) = common::bot_match(5);
    common::run(&mut m, &mut s, ticks(30.0));
    let ((), n) = el_alloc::count(|| m.reset(99, common::picks(99)));
    assert_eq!(n, 0, "a shard reuses its matches: {n} heap operations");
}
