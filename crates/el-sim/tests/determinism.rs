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
const GOLDEN: u64 = 0xbfcb_c82f_a8e9_a080;
/// Hash of a whole bot match, at the tick its nexus fell, and that tick.
const MATCH_GOLDEN: (u64, u32) = (0x5401_50db_fb40_8d62, 20_869);

fn two_minutes() -> u64 {
    let (mut m, mut s) = common::bot_match(7);
    common::run(&mut m, &mut s, ticks(120.0));
    assert!(m.champs.iter().map(|c| c.kills).sum::<u16>() > 0, "two minutes in, someone has died");
    m.hash()
}

/// The FNV-1a of a checkpoint taken a minute into `two_minutes`'s match: the same bytes native and
/// wasm (update with the golden hash).
const SNAPSHOT_GOLDEN: u64 = 0xe811_51f6_6f5e_7897;

/// `two_minutes`, read back from a checkpoint at one minute and stepped on from there: the same
/// hash, and the checkpoint's bytes digested.
fn two_minutes_from_a_checkpoint() -> (u64, u64) {
    let (mut m, mut s) = common::bot_match(7);
    common::run(&mut m, &mut s, ticks(60.0));
    let mut bytes = vec![0u8; el_sim::snapshot::len(&m)];
    el_sim::snapshot::write(&m, &mut bytes);
    let mut digest = el_sim::hash::Fnv::default();
    for b in &bytes {
        digest.u32(u32::from(*b));
    }
    let mut r = el_sim::Match::new(common::map(), 0, [0; el_sim::units::SEATS]);
    el_sim::snapshot::read(&bytes, &mut r).unwrap();
    let mut s = el_sim::path::PathScratch::new();
    common::run(&mut r, &mut s, ticks(60.0));
    (r.hash(), digest.0)
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_checkpoint_is_golden_native() {
    let (h, d) = two_minutes_from_a_checkpoint();
    assert_eq!(h, GOLDEN, "read back at a minute, the match went elsewhere: {h:#018x}");
    assert_eq!(d, SNAPSHOT_GOLDEN, "the checkpoint's bytes changed: {d:#018x}");
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn a_checkpoint_is_golden_wasm() {
    assert_eq!(two_minutes_from_a_checkpoint(), (GOLDEN, SNAPSHOT_GOLDEN));
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
