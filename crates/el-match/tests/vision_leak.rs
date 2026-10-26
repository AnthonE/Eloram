//! Fog is the server's: an oracle over every datagram a seat is sent (`docs/NETCODE.md`, "Fog is
//! the server's, a team at a time"). A spy client, sent every datagram with nothing lost, decodes
//! each as it leaves the server, and everything in it is checked against what the seat's team saw
//! on the server at the time: a unit's record only while the team sees it, a missile only from
//! where the team saw it, a name on an event only of a unit seen. The count of leaks stays at 0.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use common::{BAD, Rig, bot_brain};
use el_client_core::Client;
use el_proto::event::WireEvent;
use el_proto::quant::pos_from_code;
use el_proto::unit::NO_TARGET;
use el_sim::map::{Map, VIS_H, VIS_WORDS};
use el_sim::units::{MAX_UNITS, SEATS, team_of_seat};

/// Ticks back an event's subject may have been seen: a reliable event goes out in the first
/// datagram with room, which is nearly always its own tick's.
const WINDOW: u32 = 30;
const HIDDEN: u8 = u8::MAX;

/// What a team saw at a tick.
#[derive(Clone)]
struct Seen {
    tick: u32,
    units: [[bool; MAX_UNITS]; 2],
    missiles: [Vec<bool>; 2],
    grid: [[u64; VIS_WORDS * VIS_H]; 2],
}

#[derive(Default)]
struct Oracle {
    spies: BTreeMap<usize, (Client, u32, u64)>,
    history: VecDeque<Seen>,
    leaks: Vec<String>,
    records: u64,
    events: u64,
    /// Zones told, of an enemy's among them, and of one following its owner.
    zones: [u64; 3],
}

impl Oracle {
    fn seen_at(&self, tick: u32) -> Option<&Seen> {
        self.history.iter().rev().find(|s| s.tick == tick)
    }

    /// Whether `team` saw unit `i` at some tick in the window before `tick`.
    fn saw(&self, team: u8, i: u8, tick: u32) -> bool {
        let i = usize::from(i);
        i < MAX_UNITS
            && self
                .history
                .iter()
                .any(|s| s.tick <= tick && s.tick + WINDOW >= tick && s.units[usize::from(team)][i])
    }

    /// Whether `team` saw the ground at `at` at some tick in the window before `tick`.
    fn lit(&self, team: u8, at: [u32; 2], tick: u32) -> bool {
        self.history.iter().any(|s| {
            s.tick <= tick
                && s.tick + WINDOW >= tick
                && Map::lit(&s.grid[usize::from(team)], pos_from_code(at))
        })
    }

    fn leak(&mut self, what: String) {
        if self.leaks.len() < 50 {
            self.leaks.push(what);
        } else {
            self.leaks[49] = format!("... and more: {what}");
        }
    }

    fn take(&mut self, shard: &el_match::Shard, seat: usize, bytes: &[u8]) {
        let m = &shard.matches[0];
        let team = team_of_seat(seat);
        let t = usize::from(team);
        if self.history.back().is_none_or(|s| s.tick != m.tick) {
            self.history.push_back(Seen {
                tick: m.tick,
                units: std::array::from_fn(|k| std::array::from_fn(|i| m.vision.units[k].get(i))),
                missiles: std::array::from_fn(|k| {
                    (0..el_sim::missile::MAX_MISSILES).map(|i| m.vision.missiles[k].get(i)).collect()
                }),
                grid: m.vision.grid,
            });
            if self.history.len() > 64 {
                self.history.pop_front();
            }
        }
        let (mut spy, last_tick, feed_from) = self.spies.remove(&seat).expect("a spy for the seat");
        // A new tick: every enemy the spy still holds, the team saw at the tick before (what it
        // stopped seeing, it was told to forget in that tick's first datagram).
        if m.tick > last_tick && last_tick > 0 {
            let before = self.seen_at(last_tick).map(|s| s.units[t]);
            for i in 0..MAX_UNITS {
                let u = spy.tracks[i];
                if u.known && i != seat {
                    let q = u.newest().unwrap().1;
                    if q.team != team && !before.is_some_and(|b| b[i]) {
                        self.leak(format!("tick {last_tick}: team {team} still holds unseen unit {i}"));
                    }
                }
            }
        }
        spy.on_datagram(bytes, 0.0).expect("the spy decodes every datagram");
        let now = m.tick;
        let seen = self.seen_at(now).expect("this tick's sight").clone();
        // Records: allies, or what the team sees now, exactly.
        for &slot in &spy.last_records[..spy.last_n] {
            self.records += 1;
            let i = usize::from(slot);
            if m.units[i].team != team && !seen.units[t][i] {
                self.leak(format!("tick {now}: team {team} sent a record of unseen unit {i}"));
            }
            // (A record and the unit's leaving can come in one datagram: then nothing is held.)
            if let Some((_, q)) = spy.tracks[i].newest()
                && q.target != NO_TARGET
                && !seen.units[t][usize::from(q.target)]
            {
                self.leak(format!("tick {now}: unit {i}'s record names unseen target {}", q.target));
            }
        }
        // Events, each the first time it's taken.
        for n in feed_from..spy.feed_n {
            self.events += 1;
            let (tick, e) = spy.feed[(n % el_client_core::client::FEED as u64) as usize];
            let ok_unit = |o: &Oracle, i: u8| {
                i == HIDDEN
                    || (usize::from(i) < SEATS && team_of_seat(usize::from(i)) == team)
                    || o.saw(team, i, tick)
            };
            if let WireEvent::ZoneSpawn { team: side, follows, .. } = e {
                self.zones[0] += 1;
                self.zones[1] += u64::from(side != team);
                self.zones[2] += u64::from(follows);
            }
            let bad = match e {
                WireEvent::MissileSpawn { id, owner, t0, target, .. } => {
                    // Told where the team first saw it, naming only who the team saw then.
                    let at = self.seen_at(t0);
                    let missile_seen = at.is_some_and(|s| s.missiles[t][usize::from(id)]);
                    let owner_ok = owner == HIDDEN || at.is_some_and(|s| s.units[t][usize::from(owner)]);
                    let target_ok = target == HIDDEN || at.is_some_and(|s| s.units[t][usize::from(target)]);
                    !(missile_seen && owner_ok && target_ok)
                }
                WireEvent::Cast { unit, .. } | WireEvent::Blink { unit, .. } => !ok_unit(self, unit),
                WireEvent::Attack { unit, target } => !ok_unit(self, unit) || !ok_unit(self, target),
                WireEvent::Damage { target, source, .. } => !ok_unit(self, target) || !ok_unit(self, source),
                WireEvent::Heal { target, .. } | WireEvent::Shield { target, .. } => !ok_unit(self, target),
                WireEvent::Death { unit, killer } => {
                    (usize::from(unit) >= SEATS && !ok_unit(self, unit)) || !ok_unit(self, killer)
                }
                WireEvent::Respawn { unit } | WireEvent::LevelUp { unit, .. } => usize::from(unit) >= SEATS,
                WireEvent::Augment { unit, .. } => {
                    usize::from(unit) >= SEATS || team_of_seat(usize::from(unit)) != team
                }
                // An area, or a zone, where the team saw its ground; one that follows its owner (so
                // stands where they do) only while the team saw them, by name.
                WireEvent::Area { at, team: side, owner, follows, .. }
                | WireEvent::ZoneSpawn { at, team: side, owner, follows, .. } => {
                    let shown = if follows {
                        owner != HIDDEN && ok_unit(self, owner)
                    } else {
                        self.lit(team, at, tick)
                    };
                    !ok_unit(self, owner) || (side != team && !shown)
                }
                WireEvent::Relic { unit, .. } => !ok_unit(self, unit),
                // A teammate's word, naming what the team sees.
                WireEvent::Ping { seat, unit, .. } => {
                    usize::from(seat) >= SEATS
                        || team_of_seat(usize::from(seat)) != team
                        || !ok_unit(self, unit)
                }
                WireEvent::Vote { team: voters, .. } => voters != team,
                WireEvent::Leave { .. }
                | WireEvent::MissileEnd { .. }
                | WireEvent::ZoneEnd { .. }
                | WireEvent::Outcome { .. }
                | WireEvent::Nothing => false,
            };
            if bad {
                self.leak(format!("tick {tick}: team {team} was told {e:?}"));
            }
        }
        let feed_n = spy.feed_n;
        self.spies.insert(seat, (spy, now, feed_n));
    }
}

#[test]
fn a_team_is_sent_only_what_it_sees() {
    let seed = 13;
    let mut rig = Rig::new(seed, vec![(2, BAD, bot_brain()), (7, BAD, bot_brain())]);
    let oracle = Rc::new(RefCell::new(Oracle::default()));
    for seat in [2, 7] {
        let spy = common::client(seat, seed);
        oracle.borrow_mut().spies.insert(seat, (spy, 0, 0));
    }
    let o = oracle.clone();
    rig.tap = Some(Box::new(move |shard, seat, bytes| o.borrow_mut().take(shard, seat, bytes)));
    rig.run(300.0);
    let o = oracle.borrow();
    let kills: u16 = rig.m().champs.iter().map(|c| c.kills).sum();
    println!(
        "{} records and {} events checked over {} ticks, {kills} kills; zones told {} ({} an enemy's, {} following)",
        o.records,
        o.events,
        rig.m().tick,
        o.zones[0],
        o.zones[1],
        o.zones[2]
    );
    for l in &o.leaks {
        println!("LEAK {l}");
    }
    assert!(o.records > 100_000 && o.events > 5_000, "{} records, {} events", o.records, o.events);
    assert!(kills > 5, "a fight to see: {kills} kills");
    assert!(o.zones[1] > 0, "an enemy's zone to check: {:?}", o.zones);
    assert!(o.leaks.is_empty(), "{} leaks, the first: {}", o.leaks.len(), o.leaks[0]);
}
