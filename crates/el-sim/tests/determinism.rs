//! The same match must produce bit-identical state on every machine and on wasm32 (the browser
//! re-steps it to watch, and predicts with it). Native runs need `--features deterministic` (glam
//! scalar math, which is what wasm32 uses too); `cargo test --workspace` gets it through feature
//! unification.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]
#![cfg(any(feature = "deterministic", target_arch = "wasm32"))]

mod common;

use el_sim::ticks;

/// Hash after two minutes of a bot match: the first waves, the first fights (update deliberately
/// when the sim changes).
const GOLDEN: u64 = 0xff28_1499_efbf_2616;
/// Hash of a whole bot match, at the tick its nexus fell, and that tick.
const MATCH_GOLDEN: (u64, u32) = (0xa557_6f5a_a726_8b72, 25_225);

fn two_minutes() -> u64 {
    let (mut m, mut s) = common::bot_match(7);
    common::run(&mut m, &mut s, ticks(120.0));
    assert!(m.champs.iter().map(|c| c.kills).sum::<u16>() > 0, "two minutes in, someone has died");
    m.hash()
}

fn whole_match() -> (u64, u32) {
    let (mut m, mut s) = common::bot_match(11);
    common::run(&mut m, &mut s, ticks(40.0 * 60.0));
    assert!(m.outcome.is_some(), "the match ends");
    (m.hash(), m.tick)
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn golden_hash_native() {
    let h = two_minutes();
    assert_eq!(h, two_minutes(), "must be reproducible within a process");
    assert_eq!(h, GOLDEN, "state hash changed: {h:#018x}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_whole_match_is_golden_native() {
    let got = whole_match();
    assert_eq!(got, MATCH_GOLDEN, "the match changed: ({:#018x}, {})", got.0, got.1);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn golden_hash_wasm() {
    assert_eq!(two_minutes(), GOLDEN);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn a_whole_match_is_golden_wasm() {
    assert_eq!(whole_match(), MATCH_GOLDEN);
}
