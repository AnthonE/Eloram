//! What a watched match tells its watcher (`?watch=`, `?replay=`): the events a seat is sent, read
//! from the match itself with nothing in fog, so a record watched shows and sounds as the match
//! did to whoever played it (`docs/FX.md`, "What landed"). The record steps the match in the page
//! (`el_sim::record::Referee`); after each tick, [`Spectator::tell`] reads what it did as
//! replication's `gather` reads it for a team, under one rule instead of a team's: every unit is
//! seen. A team's own word (its pings, its vote) stays the team's.
//!
//! What it's told goes into a feed, missiles and zones shaped as a [`Client`](crate::Client)'s,
//! so the effects' director, the champions' poses and the missiles drawn read either the same way.

use el_proto::event::WireEvent;
use el_proto::quant::{facing_code, pos_code};
use el_sim::events::Event;
use el_sim::game::Match;
use el_sim::missile::MAX_MISSILES;
use el_sim::units::MAX_UNITS;
use el_sim::zone::MAX_ZONES;

use crate::client::FEED;
use crate::missiles::{HIDDEN, Missiles};
use crate::zones::Zones;

/// A slot's number on the wire, or [`HIDDEN`] for one that names no unit.
fn slot(i: u16) -> u8 {
    if usize::from(i) < MAX_UNITS { i as u8 } else { HIDDEN }
}

pub struct Spectator {
    /// The newest [`FEED`] events, with their ticks: event `n` sits at `n % FEED`.
    pub feed: Box<[(u32, WireEvent)]>,
    pub feed_n: u64,
    pub missiles: Missiles,
    pub zones: Zones,
    /// The missiles and zones told of, by the generation they were told at.
    missile_told: Box<[Option<u8>]>,
    zone_told: [Option<u8>; MAX_ZONES],
}

impl Default for Spectator {
    fn default() -> Self {
        Self {
            feed: vec![(0, WireEvent::Nothing); FEED].into_boxed_slice(),
            feed_n: 0,
            missiles: Missiles::default(),
            zones: Zones::default(),
            missile_told: vec![None; MAX_MISSILES].into_boxed_slice(),
            zone_told: [None; MAX_ZONES],
        }
    }
}

impl Spectator {
    /// Forgets the missiles and zones told of (the match started again from a checkpoint); the feed
    /// goes on counting, so whoever reads it reads on.
    pub fn restart(&mut self) {
        self.missiles.clear();
        self.zones.clear();
        self.missile_told.fill(None);
        self.zone_told = [None; MAX_ZONES];
    }

    fn told(&mut self, tick: u32, e: WireEvent, feed: bool) {
        match e {
            WireEvent::MissileSpawn { .. } | WireEvent::MissileEnd { .. } => self.missiles.take(&e),
            WireEvent::ZoneSpawn { .. } | WireEvent::ZoneEnd { .. } => self.zones.take(tick, &e),
            _ => {}
        }
        if feed {
            self.feed[(self.feed_n % FEED as u64) as usize] = (tick, e);
            self.feed_n += 1;
        }
    }

    /// What tick `m.tick` did, just stepped. With `feed` false (a match being caught up on, too
    /// far behind to show) its missiles and zones are kept and its events let go.
    pub fn tell(&mut self, m: &Match, feed: bool) {
        let now = m.tick;
        for e in m.events.as_slice() {
            let w = match *e {
                Event::Cast { unit, slot, at } => {
                    WireEvent::Cast { unit: unit as u8, slot, at: pos_code(at) }
                }
                Event::Attack { unit, target } => {
                    WireEvent::Attack { unit: unit as u8, target: target as u8 }
                }
                Event::Blink { unit, to, .. } => WireEvent::Blink { unit: unit as u8, to: pos_code(to) },
                Event::Damage { target, source, amount, kind, from } => WireEvent::Damage {
                    target: target as u8,
                    source: slot(source),
                    amount: amount as u16,
                    kind: kind as u8,
                    from: from.code() as u16,
                },
                Event::Heal { target, amount, from } => {
                    WireEvent::Heal { target: target as u8, amount: amount as u16, from: from.code() as u16 }
                }
                Event::Shield { target, amount, from } => WireEvent::Shield {
                    target: target as u8,
                    amount: amount as u16,
                    from: from.code() as u16,
                },
                Event::Death { unit, killer } => WireEvent::Death { unit: unit as u8, killer: slot(killer) },
                Event::Respawn { unit } => WireEvent::Respawn { unit: unit as u8 },
                Event::LevelUp { unit, level } => WireEvent::LevelUp { unit: unit as u8, level },
                Event::Area { zone, at, radius, team, source, owner, follows } => WireEvent::Area {
                    zone,
                    generation: m.zones[usize::from(zone)].generation,
                    at: pos_code(at),
                    radius: radius as u16,
                    team,
                    source: source.code() as u16,
                    owner: slot(owner),
                    follows,
                },
                Event::RelicTaken { pad, unit } => WireEvent::Relic { pad, beam: false, unit: slot(unit) },
                Event::RelicBeam { pad } => WireEvent::Relic { pad, beam: true, unit: HIDDEN },
                Event::Augment { unit, augment } => WireEvent::Augment { unit: unit as u8, augment },
                Event::Outcome { winner } => WireEvent::Outcome { winner },
                // A team's word is the team's.
                Event::Ping { .. } | Event::Vote { .. } => continue,
                // Missiles are told below, from the match's table.
                Event::MissileSpawn { .. } | Event::MissileEnd { .. } => continue,
            };
            self.told(now, w, feed);
        }
        // Missiles: told as they leave (where they are then, as replication tells one first seen),
        // and as they end.
        for i in 0..MAX_MISSILES {
            let ms = &m.missiles[i];
            if let Some(generation) = self.missile_told[i]
                && (!ms.alive || ms.generation != generation)
            {
                self.missile_told[i] = None;
                self.told(
                    now,
                    WireEvent::MissileEnd { id: i as u16, generation, at: pos_code(ms.pos) },
                    feed,
                );
            }
            if ms.alive && self.missile_told[i].is_none() {
                let travelled = (ms.pos - ms.origin).length();
                let target = usize::from(ms.target.idx);
                let e = WireEvent::MissileSpawn {
                    id: i as u16,
                    generation: ms.generation,
                    team: ms.team,
                    source: ms.source.code() as u16,
                    owner: slot(ms.owner.idx),
                    origin: pos_code(ms.pos),
                    t0: now,
                    target: if ms.homing && target < MAX_UNITS { target as u8 } else { HIDDEN },
                    dir: facing_code(el_sim::math::angle(ms.dir)),
                    speed: ms.speed as u16,
                    range: if ms.homing { 0 } else { (ms.range - travelled).max(0.0) as u16 },
                    width: ms.width.min(255.0) as u8,
                };
                self.missile_told[i] = Some(ms.generation);
                self.told(now, e, feed);
            }
        }
        // Zones, the same.
        for i in 0..MAX_ZONES {
            let z = &m.zones[i];
            if let Some(generation) = self.zone_told[i]
                && (!z.alive || z.generation != generation)
            {
                self.zone_told[i] = None;
                self.told(now, WireEvent::ZoneEnd { id: i as u8, generation }, feed);
            }
            if z.alive && self.zone_told[i].is_none() {
                let e = WireEvent::ZoneSpawn {
                    id: i as u8,
                    generation: z.generation,
                    team: z.team,
                    source: z.source.code() as u16,
                    owner: slot(z.owner.idx),
                    follows: z.follows,
                    at: pos_code(z.pos),
                    radius: z.radius as u16,
                    next: z.next,
                    period: z.period.min(u32::from(u16::MAX)) as u16,
                    end: z.end,
                };
                self.zone_told[i] = Some(z.generation);
                self.told(now, e, feed);
            }
        }
        self.zones.expire(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::bot;
    use el_sim::game::SeatInput;
    use el_sim::map::Map;
    use el_sim::path::PathScratch;
    use el_sim::units::SEATS;

    /// A tick of ten bots.
    fn step(m: &mut Match, scratch: &mut PathScratch) {
        let mut inputs = [SeatInput::default(); SEATS];
        for (seat, input) in inputs.iter_mut().enumerate() {
            bot::think(m, seat, input);
        }
        m.step(&inputs, scratch);
    }

    /// Ten bots for `secs` of a match, told as they go.
    fn watched(secs: f32) -> (Match, Spectator, Vec<(u32, WireEvent)>) {
        let mut m = Match::new(Map::leaked(), 11, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut scratch = PathScratch::new();
        let mut s = Spectator::default();
        let mut all = Vec::new();
        for _ in 0..el_sim::ticks(secs) {
            step(&mut m, &mut scratch);
            s.tell(&m, true);
            let from = s.feed_n.saturating_sub(FEED as u64).max(all.len() as u64);
            for n in from..s.feed_n {
                all.push(s.feed[(n % FEED as u64) as usize]);
            }
        }
        (m, s, all)
    }

    #[test]
    fn a_watched_match_tells_its_casts_missiles_zones_and_deaths() {
        let (m, s, all) = watched(240.0);
        let count = |f: fn(&WireEvent) -> bool| all.iter().filter(|(_, e)| f(e)).count();
        assert!(count(|e| matches!(e, WireEvent::Cast { .. })) > 20, "casts");
        assert!(count(|e| matches!(e, WireEvent::Damage { .. })) > 100, "hits");
        assert!(count(|e| matches!(e, WireEvent::Death { .. })) > 10, "deaths");
        let spawned = count(|e| matches!(e, WireEvent::MissileSpawn { .. }));
        let ended = count(|e| matches!(e, WireEvent::MissileEnd { .. }));
        assert!(spawned > 50, "missiles: {spawned}");
        // Every missile told ends, but those still flying.
        assert_eq!(spawned - ended, m.missiles.iter().filter(|ms| ms.alive).count());
        assert!(count(|e| matches!(e, WireEvent::ZoneSpawn { .. })) > 0, "zones");
        assert!(count(|e| matches!(e, WireEvent::Ping { .. } | WireEvent::Vote { .. })) == 0);
        // What's flying is what the match has, where its closed form says.
        for (i, ms) in m.missiles.iter().enumerate() {
            let shot = &s.missiles.shots[i];
            assert_eq!(shot.alive, ms.alive, "missile {i}");
            if ms.alive && !ms.homing {
                let drawn = shot.pos(f64::from(m.tick), None);
                assert!(drawn.distance(ms.pos) < 1.0, "missile {i} drawn {drawn} at {}", ms.pos);
            }
        }
        for (i, z) in m.zones.iter().enumerate() {
            assert_eq!(s.zones.fields[i].alive, z.alive && z.end >= m.tick, "zone {i}");
        }
    }

    #[test]
    fn caught_up_on_quietly_it_tells_nothing_but_keeps_what_flies() {
        let mut m = Match::new(Map::leaked(), 11, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut scratch = PathScratch::new();
        let mut s = Spectator::default();
        for _ in 0..el_sim::ticks(150.0) {
            step(&mut m, &mut scratch);
            s.tell(&m, false);
        }
        assert_eq!(s.feed_n, 0);
        let flying = m.missiles.iter().filter(|ms| ms.alive).count();
        assert_eq!(s.missiles.live().count(), flying);
    }
}
