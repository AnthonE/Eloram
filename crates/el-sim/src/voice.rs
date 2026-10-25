//! What a team says to itself: pings, and the vote to surrender (`docs/DESIGN.md`, "The mode").
//! Both are orders like any other, so both are in the record; both are told to the team alone.

use el_proto::UnitRef;
use el_proto::order::ping;
use glam::Vec2;

use crate::content::rules;
use crate::events::Event;
use crate::game::Match;
use crate::ticks;
use crate::units::{MAX_UNITS, TEAM_SIZE, team_of_seat};

/// "A ping names no unit".
pub const NO_UNIT: u16 = u16::MAX;

/// A team's vote to surrender.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vote {
    /// Open until this tick (0: no vote open).
    pub until: u32,
    /// The team's seats that said yes, and no: a bit a seat, in seat order.
    pub yes: u8,
    pub no: u8,
    /// No vote opens before this tick (a failed one's wait).
    pub next: u32,
}

impl Vote {
    pub fn open(&self) -> bool {
        self.until != 0
    }
}

impl Match {
    /// Whether a player holds `seat` (as the record says: what the vote counts).
    pub fn seated(&self, seat: usize) -> bool {
        self.seated & (1 << seat) != 0
    }

    /// A player takes `seat` or leaves it (the shard, and the record's seat frames).
    pub fn set_seated(&mut self, seat: usize, on: bool) {
        if seat < crate::units::SEATS {
            if on {
                self.seated |= 1 << seat;
            } else {
                self.seated &= !(1 << seat);
            }
        }
    }

    /// The team's seats held by players, a bit each, in seat order.
    fn players(&self, team: u8) -> u8 {
        (self.seated >> (usize::from(team) * TEAM_SIZE)) as u8 & ((1 << TEAM_SIZE) - 1)
    }

    pub(crate) fn ping(&mut self, seat: usize, kind: u8, at: Vec2, unit: Option<UnitRef>) {
        let now = self.tick;
        if kind >= ping::COUNT {
            return;
        }
        // A few at once, then one a second (a theoretical arrival time, one number).
        let every = ticks(rules::PING_EVERY);
        let c = &mut self.champs[seat];
        if c.ping_at > now + every * (rules::PING_BURST - 1) {
            return;
        }
        c.ping_at = c.ping_at.max(now) + every;
        // A unit only if the team sees it: a ping can't find what the team can't.
        let team = team_of_seat(seat);
        let unit = unit
            .and_then(|r| {
                let i = usize::from(r.slot);
                let u = self.units.get(i)?;
                (u.alive && u.generation as u8 == r.generation && self.vision.units[usize::from(team)].get(i))
                    .then_some(i as u16)
            })
            .unwrap_or(NO_UNIT);
        self.event(Event::Ping { seat: seat as u8, kind, at, unit });
    }

    pub(crate) fn vote(&mut self, seat: usize, yes: bool) {
        let now = self.tick;
        if !self.seated(seat) {
            return;
        }
        let team = team_of_seat(seat);
        let bit = 1u8 << (seat % TEAM_SIZE);
        let v = &mut self.surrender[usize::from(team)];
        if !v.open() {
            if !yes || now < ticks(rules::SURRENDER_FROM) || now < v.next {
                return;
            }
            *v = Vote { until: now + ticks(rules::SURRENDER_VOTE), yes: 0, no: 0, next: v.next };
        }
        if (v.yes | v.no) & bit != 0 {
            return;
        }
        if yes {
            v.yes |= bit;
        } else {
            v.no |= bit;
        }
        let v = *v;
        self.event(Event::Vote { team, yes: v.yes, no: v.no, open: true, passed: false });
    }

    /// Settles each team's open vote: it passes once enough of the team's players said yes, and
    /// fails once too many said no or its time is up. Only players vote; a team's bots follow.
    pub(crate) fn surrender_tick(&mut self) {
        let now = self.tick;
        for team in 0..2u8 {
            let players = self.players(team);
            let v = &mut self.surrender[usize::from(team)];
            if !v.open() {
                continue;
            }
            let (yes, no) = ((v.yes & players).count_ones(), (v.no & players).count_ones());
            let n = players.count_ones();
            // Four of five: so many of the players there are, rounded up.
            let needed = (n * rules::SURRENDER_SHARE.0).div_ceil(rules::SURRENDER_SHARE.1).max(1);
            let passed = yes >= needed;
            if !passed && n.saturating_sub(no) >= needed && now < v.until {
                continue;
            }
            let (y, n) = (v.yes, v.no);
            *v = Vote { until: 0, yes: 0, no: 0, next: now + ticks(rules::SURRENDER_AGAIN) };
            self.event(Event::Vote { team, yes: y, no: n, open: false, passed });
            if passed && self.outcome.is_none() {
                let winner = 1 - team;
                self.outcome = Some(winner);
                self.event(Event::Outcome { winner });
            }
        }
    }

    /// Takes a team's vote as its seat was told it (the client's model, for the page and agents).
    pub fn set_vote(&mut self, team: u8, yes: u8, no: u8, open: bool) {
        if let Some(v) = self.surrender.get_mut(usize::from(team)) {
            v.until = if open { u32::MAX } else { 0 };
            v.yes = yes;
            v.no = no;
        }
    }
}

const _: () = assert!(MAX_UNITS < NO_UNIT as usize);
