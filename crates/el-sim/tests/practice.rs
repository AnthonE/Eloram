//! Practice (`el_sim::practice`): the player casts anything, as often as it likes, at dummies that
//! never die and never fight back; no minions march; and its record replays it as it was played.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use el_proto::{Aim, Order, UnitRef};
use el_sim::content::Target;
use el_sim::content::champions::{CHAMPIONS, ability};
use el_sim::events::Event;
use el_sim::game::SeatInput;
use el_sim::path::PathScratch;
use el_sim::practice::{self, PLACES, PLAYER, SHIELDED};
use el_sim::record::{self, Frame, Frames, Referee};
use el_sim::units::{FIRST_MINION, MAX_UNITS, SEATS};
use el_sim::{Match, bot, ticks};

/// Seconds the player casts each slot for, in turn.
const EACH: f32 = 15.0;

/// The player's input this tick: a slot at a time for [`EACH`] seconds (Q, W, E, R), cast a few
/// times a second at the dummy in front of it (or an ally, for what's for allies), its order on
/// the wire's grid as a client's is.
fn player_input(m: &Match) -> SeatInput {
    let mut input = SeatInput::default();
    let every = ticks(0.25);
    // Each slot in turn once, then nothing (for what's moved to settle).
    if !m.tick.is_multiple_of(every) || m.tick >= 4 * ticks(EACH) {
        return input;
    }
    let slot = ((m.tick / ticks(EACH)) % 4) as u8;
    // A cast under way (winding up, or walking into range) is let finish, as a person would.
    let c = &m.champs[PLAYER];
    if c.cast.slot != el_sim::game::NO_SLOT || matches!(c.intent, el_sim::game::Intent::CastAt { .. }) {
        return input;
    }
    let def = m.champs[PLAYER].def;
    let unit = |i: usize| UnitRef { slot: i as u8, generation: m.units[i].generation as u8 };
    let aim = match ability(def, slot).target {
        Target::Dir | Target::Point => Aim::Point(m.units[5].pos),
        Target::Enemy => Aim::Unit(unit(5)),
        Target::Ally => Aim::Unit(unit(1)),
        Target::Caster => Aim::None,
    };
    input.push(Order::Cast { slot, aim }.quantized());
    input
}

/// A practice match with `champ` in the player's seat, stepped `secs`: the match, what it did, its
/// record, and the farthest each seat went from its place.
fn practise(champ: u8, secs: f32) -> (Match, Vec<Event>, Vec<u8>, [f32; SEATS]) {
    let picks = std::array::from_fn(|s| ((usize::from(champ) + s) % CHAMPIONS.len()) as u8);
    let mut m = Match::new(common::map(), 77, picks);
    m.set_practice();
    let mut scratch = PathScratch::new();
    let mut events = Vec::new();
    let mut rec = Vec::new();
    let mut buf = [0u8; record::MAX_FRAME];
    let mut far = [0.0f32; SEATS];
    let n = record::write_header(&mut buf, &record::Header::new(77, picks));
    rec.extend_from_slice(&buf[..n]);
    let n = record::write_practice(&mut buf);
    rec.extend_from_slice(&buf[..n]);
    for _ in 0..ticks(secs) {
        let mut inputs = [SeatInput::default(); SEATS];
        inputs[PLAYER] = player_input(&m);
        for (seat, input) in inputs.iter_mut().enumerate().skip(1) {
            bot::think(&m, seat, input);
        }
        let n = record::write_tick(&mut buf, m.tick, &inputs);
        rec.extend_from_slice(&buf[..n]);
        m.step(&inputs, &mut scratch);
        events.extend_from_slice(m.events.as_slice());
        for (s, f) in far.iter_mut().enumerate() {
            *f = f.max(m.units[s].pos.distance(PLACES[s].0));
        }
        if m.tick.is_multiple_of(record::HASH_EVERY) {
            let n = record::write_hash(&mut buf, m.tick, m.hash());
            rec.extend_from_slice(&buf[..n]);
        }
    }
    (m, events, rec, far)
}

#[test]
fn practice_casts_anything_at_dummies_that_never_die_or_fight() {
    for champ in 0..CHAMPIONS.len() as u8 {
        let (m, events, _, far) = practise(champ, 4.0 * EACH + 5.0);
        let name = CHAMPIONS[usize::from(champ)].name;
        // Every rank open, at the top level.
        assert_eq!(m.champs[PLAYER].level, el_sim::content::rules::MAX_LEVEL);
        assert_eq!(m.champs[PLAYER].ranks, [5, 5, 5, 3], "{name}");
        // Cast as often as it's asked: no cooldowns, no mana spent. Not every order four times a
        // second takes (a cast winds up, a channel lasts), but each slot is cast again and again.
        for slot in 0..4u8 {
            let casts = events
                .iter()
                .filter(|e| matches!(e, Event::Cast { unit: 0, slot: s, .. } if *s == slot))
                .count();
            assert!(casts >= 5, "{name}'s slot {slot} cast {casts} times in {EACH} s");
        }
        // Hurt, and never dead; and a dummy never swings back.
        let hits = events.iter().filter(|e| matches!(e, Event::Damage { source: 0, .. })).count();
        assert!(hits > 20, "{name} hit the dummies {hits} times");
        assert!(!events.iter().any(|e| matches!(e, Event::Death { .. })), "{name}: nobody dies");
        assert!(
            !events.iter().any(|e| matches!(e, Event::Attack { unit, .. } if *unit != 0)),
            "{name}: the dummies never attack"
        );
        assert!(m.units[..SEATS].iter().all(|u| u.alive));
        // No waves.
        assert!(m.units[FIRST_MINION..MAX_UNITS].iter().all(|u| !u.alive), "no minions");
        // The shielded dummy is shielded, now and then.
        let shields = events
            .iter()
            .filter(|e| matches!(e, Event::Shield { target, .. } if usize::from(*target) == SHIELDED))
            .count();
        assert!(shields >= 10, "the shielded dummy's shields: {shields}");
        // The walkers walk their loops.
        for (s, (_, d)) in PLACES.iter().enumerate().skip(1) {
            if *d == Some(practice::Dummy::Walks) {
                assert!(far[s] > 300.0, "{name}: dummy {s} walked {}", far[s]);
            }
        }
        // And a stander moved off its place (a pull, a knock) is back at it.
        for (s, (at, d)) in PLACES.iter().enumerate().skip(1) {
            if *d != Some(practice::Dummy::Walks) {
                assert!(m.units[s].pos.distance(*at) < 80.0, "{name}: dummy {s} is back at its place");
            }
        }
    }
}

#[test]
fn a_practice_record_replays_as_it_was_played() {
    let (m, _, rec, _) = practise(3, 30.0);
    assert!(Frames::new(&rec).any(|f| f == Frame::Practice));
    let mut r = Referee::new(common::map());
    for f in Frames::new(&rec) {
        r.take(&f).expect("the replay keeps to the record");
    }
    r.flush();
    assert!(r.m.practice);
    assert_eq!(r.m.tick, m.tick);
    assert_eq!(r.m.hash(), m.hash(), "the replay comes to the match played");
    assert!(r.checked >= 25);
    // Without its practice frame, the same inputs make another match.
    let plain: Vec<u8> = {
        let mut out = Vec::new();
        let mut at = 0;
        while let Ok(Some((f, used))) = record::read_frame(&rec[at..]) {
            if f != Frame::Practice {
                out.extend_from_slice(&rec[at..at + used]);
            }
            at += used;
        }
        out
    };
    let mut r = Referee::new(common::map());
    let astray = Frames::new(&plain).any(|f| r.take(&f).is_err());
    assert!(astray, "a practice match's record read as a plain match's goes astray");
}

#[test]
fn a_checkpoint_keeps_a_match_practice() {
    let (m, _, _, _) = practise(5, 5.0);
    let mut buf = vec![0u8; el_sim::snapshot::len(&m)];
    let n = el_sim::snapshot::write(&m, &mut buf);
    let mut back = Match::new(common::map(), 0, [0; SEATS]);
    el_sim::snapshot::read(&buf[..n], &mut back).unwrap();
    assert!(back.practice);
    assert_eq!(back.hash(), m.hash());
}
