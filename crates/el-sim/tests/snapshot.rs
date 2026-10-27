//! A checkpoint read back is the match it was taken from: stepped on the same inputs, the two stay
//! the same match, tick for tick and byte for byte (`el_sim::snapshot`).
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]
#![cfg(not(target_arch = "wasm32"))]

mod common;

use el_sim::path::PathScratch;
use el_sim::snapshot::{self, SnapError};
use el_sim::units::SEATS;
use el_sim::{Match, ticks};

fn take(m: &Match) -> Vec<u8> {
    let mut b = vec![0u8; snapshot::len(m)];
    assert_eq!(snapshot::write(m, &mut b), b.len());
    b
}

fn blank() -> Match {
    Match::new(common::map(), 0, [0; SEATS])
}

/// Steps `a` and a match read from its checkpoint on `a`'s bots' inputs: the hash every tick and
/// the checkpoint at the end must agree (the hash leaves fields out; the checkpoint doesn't).
fn steps_alike(a: &mut Match, s: &mut PathScratch, secs: f32) {
    let mut b = blank();
    snapshot::read(&take(a), &mut b).unwrap();
    assert_eq!(snapshot::diff(a, &b), None);
    let mut sb = PathScratch::new();
    for _ in 0..ticks(secs) {
        let inputs = common::bot_inputs(a);
        a.step(&inputs, s);
        b.step(&inputs, &mut sb);
        assert_eq!(a.hash(), b.hash(), "tick {}", a.tick);
    }
    assert_eq!(snapshot::diff(a, &b), None);
    assert_eq!(take(a), take(&b), "the checkpoints differ at tick {}", a.tick);
}

#[test]
fn a_match_read_back_steps_as_the_original() {
    let (mut a, mut s) = common::bot_match(23);
    common::run(&mut a, &mut s, ticks(150.0));
    assert!(a.units.iter().filter(|u| u.alive).count() > 30, "a busy match");
    steps_alike(&mut a, &mut s, 150.0);
}

#[test]
fn every_augment_reads_back_and_steps_alike() {
    use el_sim::content::augments::AUGMENTS;
    use el_sim::content::champions::CHAMPIONS;
    for half in 0..2usize {
        let picks = std::array::from_fn(|s| ((half * SEATS + s) % CHAMPIONS.len()) as u8);
        let mut m = Match::new(common::map(), 31 + half as u64, picks);
        let mut s = PathScratch::new();
        for seat in 0..SEATS {
            for k in 0..5 {
                m.grant_augment(seat, ((half * 50 + seat * 5 + k) % AUGMENTS.len()) as u8);
            }
        }
        common::run(&mut m, &mut s, ticks(75.0));
        steps_alike(&mut m, &mut s, 60.0);
    }
}

#[test]
fn the_same_length_for_every_match_and_read_write_round_trips() {
    let (mut a, mut s) = common::bot_match(5);
    let fresh = snapshot::len(&a);
    common::run(&mut a, &mut s, ticks(100.0));
    assert_eq!(snapshot::len(&a), fresh);
    let once = take(&a);
    assert_eq!(snapshot::tick_of(&once), Some(a.tick));
    let mut b = blank();
    snapshot::read(&once, &mut b).unwrap();
    assert_eq!(take(&b), once);
    assert_eq!(snapshot::write(&a, &mut vec![0u8; once.len() - 1]), 0, "too short a buffer writes nothing");
    let mut buf = vec![0u8; once.len()];
    let started = std::time::Instant::now();
    for _ in 0..100 {
        snapshot::write(&a, &mut buf);
    }
    let write_us = started.elapsed().as_secs_f64() * 1e4;
    let started = std::time::Instant::now();
    for _ in 0..100 {
        snapshot::read(&once, &mut b).unwrap();
    }
    let read_us = started.elapsed().as_secs_f64() * 1e4;
    println!("a checkpoint is {} bytes, written in {write_us:.0} µs, read in {read_us:.0}", once.len());
}

#[test]
fn the_offer_secret_stays_out() {
    let (mut a, mut s) = common::bot_match(9);
    a.set_offer_secret(0x1234_5678_9abc_def0);
    common::run(&mut a, &mut s, ticks(10.0));
    let mut b = blank();
    snapshot::read(&take(&a), &mut b).unwrap();
    assert_eq!(b.offer_secret, None);
    let bytes = take(&a);
    let secret = 0x1234_5678_9abc_def0u64.to_le_bytes();
    assert!(!bytes.windows(8).any(|w| w == secret), "the secret's bytes are in the checkpoint");
}

#[test]
fn a_damaged_or_foreign_checkpoint_is_refused_and_changes_nothing() {
    let (mut a, mut s) = common::bot_match(3);
    common::run(&mut a, &mut s, ticks(40.0));
    let good = take(&a);
    let mut b = blank();
    let before = take(&b);
    let mut bad = good.clone();
    bad[good.len() / 2] ^= 0x10;
    assert_eq!(snapshot::read(&bad, &mut b), Err(SnapError::Checksum));
    assert_eq!(snapshot::read(&good[..good.len() - 1], &mut b), Err(SnapError::Shape));
    let mut magic = good.clone();
    magic[0] = b'X';
    assert_eq!(snapshot::read(&magic, &mut b), Err(SnapError::Magic));
    assert_eq!(take(&b), before, "a refused checkpoint left its mark");
}

#[test]
fn diff_names_the_unit_and_the_field() {
    let (mut a, mut s) = common::bot_match(13);
    common::run(&mut a, &mut s, ticks(80.0));
    let mut b = blank();
    snapshot::read(&take(&a), &mut b).unwrap();
    let slot = a.units.iter().enumerate().rposition(|(_, u)| u.alive && u.kind.is_minion()).unwrap();
    b.units[slot].stats.armor += 1.0;
    let w = snapshot::diff(&a, &b).unwrap();
    assert_eq!(w.to_string(), format!("unit {slot}: stats.armor"));
    b.units[slot].stats.armor = a.units[slot].stats.armor;
    b.champs[3].mods.ad += 1.0;
    assert_eq!(snapshot::diff(&a, &b).unwrap().to_string(), "champ 3: mods.ad");
    b.champs[3].mods.ad = a.champs[3].mods.ad;
    b.rng = el_sim::math::Rng::from_state(1);
    assert_eq!(snapshot::diff(&a, &b).unwrap().to_string(), "match: rng");
}
