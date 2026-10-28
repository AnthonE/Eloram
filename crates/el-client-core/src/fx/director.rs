//! What a frame should show and play, from what the client was told (`docs/FX.md`, "The effects
//! runtime"): the events, each when it should be seen; the units' flags, where one rises; the own
//! champion's own doings, from the prediction. Each becomes a [`Cue`]: what it is, where, whose,
//! how much it matters to this viewer, and its sound and rung on the mixer's ladder.
//!
//! When: the own champion's casts, blinks, missiles and swings start from the prediction (it steps
//! the server's own code, so they're there a round trip early), and the server's word of them
//! isn't played again. Anything else about a unit waits for the clock everyone else is drawn at,
//! so a hit lands where the unit is seen to be; what has a place and no unit (a blast, a missile's
//! end) is drawn on the predicted clock already, and goes at once.
//!
//! Every event has its own line, with no catch-all, as replication's team rules do: a new event
//! can't be silently ignored here either.

use el_proto::event::WireEvent;
use el_proto::quant::pos_from_code;
use el_proto::unit::{QUnit, flags};
use el_sim::content::champions::CHAMPIONS;
use el_sim::content::rules;
use el_sim::game::Match;
use el_sim::missile::Source;
use el_sim::units::{Kind, MAX_UNITS, SEATS, team_of_seat};
use glam::Vec2;

use super::look::{self, Look, Reach, Side, world};
use super::mix;
use super::sound::{Part, Sfx};
use super::voice::Announcer;
use crate::client::{OWN_FEED, OwnEvent};
use crate::interp::Sample;
use crate::missiles::Shot;
use crate::spectator::Spectator;
use crate::{Client, client::FEED};

/// "Not seen" on the wire.
const HIDDEN: u8 = u8::MAX;
/// Longest an event about a unit waits (s) for the drawn clock to reach it.
const HOLD_MAX: f64 = 0.5;
/// Ticks either side within which the server's word of the own champion's cast, missile or swing
/// is the one the prediction already played.
const OWN_MATCH: u32 = 45;

/// A status a unit's flags show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Stun,
    Root,
    Silence,
    Slow,
    Airborne,
    /// Knocked back or pulled.
    Shove,
    Reveal,
    Immune,
    Empowered,
    Hasted,
}

impl Status {
    /// The flag it shows as, and the order flags are read in.
    pub const FLAGS: [(u32, Status); 9] = [
        (flags::STUNNED, Status::Stun),
        (flags::ROOTED, Status::Root),
        (flags::SILENCED, Status::Silence),
        (flags::SLOWED, Status::Slow),
        (flags::AIRBORNE, Status::Airborne),
        (flags::REVEALED, Status::Reveal),
        (flags::IMMUNE, Status::Immune),
        (flags::EMPOWERED, Status::Empowered),
        (flags::HASTED, Status::Hasted),
    ];

    pub fn sound(self) -> Sfx {
        match self {
            Status::Stun => Sfx::StatusStun,
            Status::Root => Sfx::StatusRoot,
            Status::Silence => Sfx::StatusSilence,
            Status::Slow => Sfx::StatusSlow,
            Status::Airborne => Sfx::StatusAirborne,
            Status::Shove => Sfx::StatusShove,
            Status::Reveal => Sfx::StatusReveal,
            Status::Immune => Sfx::StatusImmune,
            Status::Empowered => Sfx::Empowered,
            Status::Hasted => Sfx::Hasted,
        }
    }

    pub fn palette(self) -> look::Palette {
        match self {
            Status::Stun | Status::Airborne | Status::Shove => world::STUN,
            Status::Root => world::ROOT,
            Status::Silence | Status::Reveal => world::SILENCE,
            Status::Slow => world::SLOW,
            Status::Immune => world::IMMUNE,
            Status::Empowered => world::EMPOWER,
            Status::Hasted => world::HASTE,
        }
    }

    /// How much it matters, to the unit's own side and to the other (crowd control matters most
    /// to whoever it holds, a buff to whoever must face it less).
    fn ratings(self) -> look::Ratings {
        match self {
            Status::Stun | Status::Airborne => look::Ratings::new(9, 7, 6),
            Status::Root | Status::Shove => look::Ratings::new(8, 6, 5),
            Status::Silence | Status::Slow => look::Ratings::new(6, 4, 4),
            Status::Immune => look::Ratings::new(6, 4, 6),
            Status::Reveal => look::Ratings::new(5, 3, 3),
            Status::Empowered | Status::Hasted => look::Ratings::new(3, 2, 4),
        }
    }
}

/// What a cue is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum What {
    /// An ability's, an auto's, a turret's part.
    Part(Part),
    /// A status rose on `Cue::unit`.
    Status(Status),
    Shield,
    Heal,
    Death(Kind),
    Respawn,
    LevelUp,
    Relic {
        beam: bool,
    },
    /// A teammate's ping, of its kind.
    Ping(u8),
    /// A skillshot that ran out with no one in its way.
    Fizzle,
    /// A turret turned on the viewer.
    TurretWarn,
    /// The interface: its sound alone.
    Ui,
}

/// One thing to show or play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub what: What,
    pub source: Source,
    pub look: Look,
    /// Where it happens.
    pub at: Vec2,
    /// Where it came from, for what moves (a blink's start, a missile's origin).
    pub from: Option<Vec2>,
    /// The unit it's on, if it's on one.
    pub unit: Option<u8>,
    /// The viewer's side of whoever caused it.
    pub side: Side,
    /// How much it matters to the viewer (1 to 10).
    pub rating: u8,
    /// Its size, where it has one (an area's radius, a lane's width; units).
    pub size: f32,
    pub sound: Option<Sfx>,
    /// The sound's rung on the mixer's ladder.
    pub rung: u8,
    /// It's about the viewer's own champion (cast by it, or landing on it).
    pub own: bool,
}

/// What the director reads of a match: a player's client (the match as their team is told it,
/// and their own champion predicted), or a match watched from its record, seen whole.
pub trait Scene {
    /// The match as known.
    fn world(&self) -> &Match;
    /// The events told, a ring of [`FEED`], and how many have been told in all.
    fn feed(&self) -> (&[(u32, WireEvent)], u64);
    /// The tick units are drawn at.
    fn view_tick(&self, now: f64) -> f64;
    fn started(&self) -> bool;
    /// Unit `i` as it's drawn (not a player's own champion), if it's seen.
    fn sample(&self, i: usize, now: f64) -> Option<Sample>;
    /// Unit `i`'s newest record, seen or not, if there is one.
    fn newest(&self, i: usize) -> Option<QUnit>;
    /// Whether unit `i` is seen.
    fn known(&self, i: usize) -> bool;
    /// Missile `id` as told.
    fn shot(&self, id: usize) -> Option<&Shot>;
    /// The player whose champion this is, with its prediction; none for a match watched.
    fn player(&self) -> Option<&Client>;
}

impl Scene for Client {
    fn world(&self) -> &Match {
        &self.world
    }

    fn feed(&self) -> (&[(u32, WireEvent)], u64) {
        (&self.feed, self.feed_n)
    }

    fn view_tick(&self, now: f64) -> f64 {
        self.clock.view_tick(now)
    }

    fn started(&self) -> bool {
        Client::started(self)
    }

    fn sample(&self, i: usize, now: f64) -> Option<Sample> {
        self.unit(i, now)
    }

    fn newest(&self, i: usize) -> Option<QUnit> {
        self.tracks.get(i).and_then(|t| t.newest()).map(|(_, q)| q)
    }

    fn known(&self, i: usize) -> bool {
        self.tracks.get(i).is_some_and(|t| t.known)
    }

    fn shot(&self, id: usize) -> Option<&Shot> {
        self.missiles.shots.get(id)
    }

    fn player(&self) -> Option<&Client> {
        Some(self)
    }
}

/// A match watched: stepped from its record to `tick` (drawn `blend` of the way on from the tick
/// before, where its units were at `prev`), and what it told.
pub struct Watched<'a> {
    pub m: &'a Match,
    pub told: &'a Spectator,
    pub prev: &'a [Vec2],
    pub blend: f32,
    /// The tick drawn (fractional).
    pub at: f64,
}

impl Scene for Watched<'_> {
    fn world(&self) -> &Match {
        self.m
    }

    fn feed(&self) -> (&[(u32, WireEvent)], u64) {
        (&self.told.feed, self.told.feed_n)
    }

    fn view_tick(&self, _now: f64) -> f64 {
        self.at
    }

    fn started(&self) -> bool {
        true
    }

    fn sample(&self, i: usize, _now: f64) -> Option<Sample> {
        let u = self.m.units.get(i).filter(|u| u.alive)?;
        let pos = self.prev.get(i).map_or(u.pos, |p| p.lerp(u.pos, self.blend));
        Some(Sample { pos, facing: u.facing, q: self.m.quantize(i, 0) })
    }

    fn newest(&self, i: usize) -> Option<QUnit> {
        (i < MAX_UNITS).then(|| self.m.quantize(i, 0))
    }

    fn known(&self, i: usize) -> bool {
        self.m.units.get(i).is_some_and(|u| u.alive)
    }

    fn shot(&self, id: usize) -> Option<&Shot> {
        self.told.missiles.shots.get(id)
    }

    fn player(&self) -> Option<&Client> {
        None
    }
}

/// No seat: the director of a match watched.
pub const NO_SEAT: usize = usize::MAX;

/// What the director remembers of a unit slot from the last frame.
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    known: bool,
    generation: u8,
    flags: u32,
    target: u8,
    kind: u8,
    team: u8,
    pos: Vec2,
}

/// The own champion's interface, as it last stood.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ui {
    points_spent: u16,
    items: [u8; el_sim::content::items::SLOTS],
    n_aug: u8,
    screen_open: bool,
    offers: [u8; 3],
}

pub struct Director {
    seat: usize,
    team: u8,
    read: u64,
    own_read: u64,
    held: Vec<(u32, f64, WireEvent)>,
    seen: Vec<Seen>,
    /// The tick of the own champion's latest predicted cast, by slot (u32::MAX: none), its latest
    /// predicted missiles (their source codes) and swing.
    own_cast: [u32; 6],
    own_missiles: [(u32, u32); 8],
    own_swing: u32,
    own_blink: u32,
    /// Each champion's latest cast seen: its tick and slot.
    cast: [(u32, u8); SEATS],
    ui: Option<Ui>,
    pub announcer: Announcer,
    welcomed: bool,
    vote_open: bool,
    surrendered: bool,
    over: bool,
    /// The server tick last frame (for what the clock says: the waves).
    tick: u32,
    inhibitors: [bool; 2],
}

impl Director {
    pub fn new(seat: usize) -> Self {
        Self {
            seat,
            team: team_of_seat(seat),
            read: 0,
            own_read: 0,
            held: Vec::with_capacity(64),
            seen: vec![Seen::default(); MAX_UNITS],
            own_cast: [u32::MAX; 6],
            own_missiles: [(u32::MAX, 0); 8],
            own_swing: u32::MAX,
            own_blink: u32::MAX,
            cast: [(u32::MAX, 0); SEATS],
            ui: None,
            announcer: Announcer::new(seat),
            welcomed: false,
            vote_open: false,
            surrendered: false,
            over: false,
            tick: 0,
            inhibitors: [true; 2],
        }
    }

    /// The director of a match watched: no champion its own, Dawn's side the near one (as the
    /// match is drawn, each side in its own colour).
    pub fn watching() -> Self {
        Self { seat: NO_SEAT, team: 0, announcer: Announcer::watching(), ..Self::new(0) }
    }

    fn side(&self, team: u8, unit: Option<usize>) -> Side {
        if unit == Some(self.seat) {
            Side::Own
        } else if team == self.team {
            Side::Ally
        } else {
            Side::Enemy
        }
    }

    /// Where unit `i` is drawn now (or was last).
    fn pos(&self, c: &dyn Scene, i: usize, now: f64) -> Vec2 {
        if i == self.seat
            && let Some(p) = c.player()
        {
            return p.own_pos(now);
        }
        match c.sample(i, now) {
            Some(s) => s.pos,
            None if self.seen.get(i).is_some_and(|s| s.known) => self.seen[i].pos,
            None => c.world().units.get(i).map_or(Vec2::ZERO, |u| u.pos),
        }
    }

    fn kind(&self, c: &dyn Scene, i: usize) -> Kind {
        if i < SEATS {
            return Kind::Champion;
        }
        match c.newest(i) {
            Some(q) => Kind::from_u8(q.kind),
            None if self.seen.get(i).is_some_and(|s| s.known) => Kind::from_u8(self.seen[i].kind),
            None => c.world().units.get(i).map_or(Kind::Melee, |u| u.kind),
        }
    }

    fn team_of(&self, c: &dyn Scene, i: usize) -> u8 {
        if i < SEATS {
            return team_of_seat(i);
        }
        match c.newest(i) {
            Some(q) => q.team,
            None if self.seen.get(i).is_some_and(|s| s.known) => self.seen[i].team,
            None => c.world().units.get(i).map_or(0, |u| u.team),
        }
    }

    /// A champion's number, if slot `i` holds one.
    fn champ(&self, c: &dyn Scene, i: usize) -> Option<u8> {
        (i < SEATS).then(|| c.world().champs[i].def)
    }

    #[allow(clippy::too_many_arguments)]
    fn cue(
        &self,
        what: What,
        source: Source,
        look: Look,
        at: Vec2,
        unit: Option<u8>,
        side: Side,
        own: bool,
        sound: Option<Sfx>,
    ) -> Cue {
        let rating = look.ratings.of(side);
        let rung = if own || matches!(what, What::Ui | What::Ping(_) | What::TurretWarn) {
            0
        } else {
            mix::rung(side, rating)
        };
        Cue { what, source, look, at, from: None, unit, side, rating, size: 0.0, sound, rung, own }
    }

    /// This frame's cues, into `out`.
    pub fn frame(&mut self, c: &dyn Scene, now: f64, out: &mut Vec<Cue>) {
        if !self.welcomed && c.started() {
            self.welcomed = true;
            self.announcer.welcome(now);
        }
        self.own(c, now, out);
        // The server's word, each when it should be seen.
        let (feed, feed_n) = c.feed();
        let from = self.read.max(feed_n.saturating_sub(FEED as u64));
        for n in from..feed_n {
            let (tick, e) = feed[(n % FEED as u64) as usize];
            if self.waits(&e) {
                if self.held.len() < 256 {
                    self.held.push((tick, now, e));
                }
            } else {
                self.event(c, tick, &e, now, out);
            }
        }
        self.read = feed_n;
        let view = c.view_tick(now);
        let mut k = 0;
        while k < self.held.len() {
            let (tick, got, e) = self.held[k];
            if f64::from(tick) <= view || now - got >= HOLD_MAX {
                self.held.remove(k);
                self.event(c, tick, &e, now, out);
            } else {
                k += 1;
            }
        }
        self.flags(c, now, out);
        self.interface(c, now, out);
        self.clock(c, now);
    }

    /// Whether an event waits for the drawn clock: it's about a unit other than the viewer's own.
    fn waits(&self, e: &WireEvent) -> bool {
        let other = |u: u8| usize::from(u) != self.seat && u != HIDDEN;
        match *e {
            WireEvent::Cast { unit, .. }
            | WireEvent::Attack { unit, .. }
            | WireEvent::Blink { unit, .. }
            | WireEvent::LevelUp { unit, .. } => other(unit),
            WireEvent::Damage { target, .. }
            | WireEvent::Heal { target, .. }
            | WireEvent::Shield { target, .. } => other(target),
            WireEvent::Death { unit, .. } => other(unit) && usize::from(unit) < SEATS,
            WireEvent::Leave { .. }
            | WireEvent::MissileSpawn { .. }
            | WireEvent::MissileEnd { .. }
            | WireEvent::ZoneSpawn { .. }
            | WireEvent::ZoneEnd { .. }
            | WireEvent::Respawn { .. }
            | WireEvent::Augment { .. }
            | WireEvent::Relic { .. }
            | WireEvent::Outcome { .. }
            | WireEvent::Ping { .. }
            | WireEvent::Vote { .. }
            | WireEvent::Area { .. }
            | WireEvent::Nothing => false,
        }
    }

    /// The own champion's doings, from the prediction.
    fn own(&mut self, c: &dyn Scene, now: f64, out: &mut Vec<Cue>) {
        let Some(p) = c.player() else { return };
        let me = self.seat;
        let def = p.pred.champs[me].def;
        let from = self.own_read.max(p.own_feed_n.saturating_sub(OWN_FEED as u64));
        for n in from..p.own_feed_n {
            let (tick, e) = p.own_feed[(n % OWN_FEED as u64) as usize];
            match e {
                OwnEvent::Cast { slot, at } => {
                    self.own_cast[usize::from(slot.min(5))] = tick;
                    self.cast(c, me, def, slot, at, now, true, out);
                }
                OwnEvent::Blink { from, to } => {
                    self.own_blink = tick;
                    self.blink(c, me, from, to, tick, out);
                }
                OwnEvent::Missile { source, from } => {
                    let k = (tick as usize) % self.own_missiles.len();
                    self.own_missiles[k] = (tick, source);
                    self.launch(c, Source::from_code(source), Some(me), from, now, true, out);
                }
                OwnEvent::Swing => {
                    self.own_swing = tick;
                    self.swing(c, me, now, out);
                }
                OwnEvent::Nothing => {}
            }
        }
        self.own_read = p.own_feed_n;
    }

    /// Champion `unit` (number `def`) began casting `slot` at `at`.
    #[allow(clippy::too_many_arguments)]
    fn cast(
        &mut self,
        c: &dyn Scene,
        unit: usize,
        def: u8,
        slot: u8,
        at: Vec2,
        now: f64,
        own: bool,
        out: &mut Vec<Cue>,
    ) {
        let l = look::ability_look(def, slot);
        let src = Source::Ability { champ: def, slot };
        let side = self.side(team_of_seat(unit), Some(unit));
        let pos = self.pos(c, unit, now);
        let u = Some(unit as u8);
        let mut cue = self.cue(What::Part(Part::Cast), src, l, pos, u, side, own, l.sound(Part::Cast));
        cue.from = Some(at);
        out.push(cue);
        if l.ult {
            out.push(self.cue(What::Part(Part::Ult), src, l, pos, u, side, own, l.sound(Part::Ult)));
        }
        if l.reach == Reach::Dash {
            let mut d = self.cue(What::Part(Part::Dash), src, l, at, u, side, own, l.sound(Part::Dash));
            d.from = Some(pos);
            out.push(d);
        }
        if l.friendly || l.reach == Reach::OnSelf {
            out.push(self.cue(What::Part(Part::Buff), src, l, pos, u, side, own, l.sound(Part::Buff)));
        }
    }

    /// A missile of `source` left `from`, `owner`'s (a slot, if seen).
    #[allow(clippy::too_many_arguments)]
    fn launch(
        &self,
        c: &dyn Scene,
        source: Source,
        owner: Option<usize>,
        from: Vec2,
        _now: f64,
        own: bool,
        out: &mut Vec<Cue>,
    ) {
        let team = owner.map_or(1 - self.team, |o| self.team_of(c, o));
        let champ = owner.and_then(|o| self.champ(c, o));
        let l = look::look(source, champ, team);
        let sound = match (source, owner.map(|o| self.kind(c, o))) {
            (Source::Auto, Some(Kind::Caster)) => Some(Sfx::MinionShot),
            (Source::Auto, Some(Kind::Siege)) => Some(Sfx::MinionSiege),
            (Source::Auto, Some(k)) if k.is_minion() => None,
            _ => l.sound(Part::Launch),
        };
        let side = self.side(team, owner);
        let mut cue =
            self.cue(What::Part(Part::Launch), source, l, from, owner.map(|o| o as u8), side, own, sound);
        if matches!(owner.map(|o| self.kind(c, o)), Some(k) if k.is_minion()) {
            cue.rung = 4;
        }
        out.push(cue);
    }

    /// Unit `unit` swung in melee.
    fn swing(&self, c: &dyn Scene, unit: usize, now: f64, out: &mut Vec<Cue>) {
        let kind = self.kind(c, unit);
        let team = self.team_of(c, unit);
        let (sound, l) = match kind {
            Kind::Champion => {
                let def = c.world().champs[unit].def;
                if CHAMPIONS[usize::from(def)].missile_speed > 0.0 {
                    return;
                }
                let l = look::look(Source::Auto, Some(def), team);
                (l.sound(Part::Launch), l)
            }
            Kind::Melee | Kind::Super => (Some(Sfx::MinionSwing), look::look(Source::Auto, None, team)),
            _ => return,
        };
        let side = self.side(team, Some(unit));
        let mut cue = self.cue(
            What::Part(Part::Launch),
            Source::Auto,
            l,
            self.pos(c, unit, now),
            Some(unit as u8),
            side,
            unit == self.seat,
            sound,
        );
        if kind != Kind::Champion {
            cue.rung = 4;
        }
        out.push(cue);
    }

    /// Unit `unit` went from `from` to `to` at once: a blink of its own, or a shove.
    #[allow(clippy::too_many_arguments)]
    fn blink(&self, c: &dyn Scene, unit: usize, from: Vec2, to: Vec2, tick: u32, out: &mut Vec<Cue>) {
        let team = self.team_of(c, unit);
        let side = self.side(team, Some(unit));
        let (t, slot) = if unit == self.seat {
            let slot = (0..6).max_by_key(|&s| self.own_cast[s].wrapping_add(1)).unwrap_or(0) as u8;
            (self.own_cast[usize::from(slot)], slot)
        } else if unit < SEATS {
            self.cast[unit]
        } else {
            (u32::MAX, 0)
        };
        let def = self.champ(c, unit);
        let own = unit == self.seat;
        let cast_blink = t != u32::MAX
            && tick.abs_diff(t) <= 15
            && def.is_some_and(|d| look::ability_look(d, slot).reach == Reach::Blink);
        if let (true, Some(d)) = (cast_blink, def) {
            let l = look::ability_look(d, slot);
            let src = Source::Ability { champ: d, slot };
            let mut cue = self.cue(
                What::Part(Part::Dash),
                src,
                l,
                to,
                Some(unit as u8),
                side,
                own,
                l.sound(Part::Dash),
            );
            cue.from = Some(from);
            // Where it lands, if it hurts, is the Damage it does: heard there.
            out.push(cue);
        } else {
            let s = Status::Shove;
            let l = status_look(s);
            let mut cue = self.cue(
                What::Status(s),
                Source::Auto,
                l,
                to,
                Some(unit as u8),
                self.side(team, None),
                own,
                Some(s.sound()),
            );
            cue.from = Some(from);
            cue.rating = s.ratings().of(if own {
                Side::Own
            } else if team == self.team {
                Side::Ally
            } else {
                Side::Enemy
            });
            out.push(cue);
        }
    }

    fn event(&mut self, c: &dyn Scene, tick: u32, e: &WireEvent, now: f64, out: &mut Vec<Cue>) {
        let me = self.seat;
        match *e {
            WireEvent::Cast { unit, slot, at } => {
                let u = usize::from(unit);
                if u >= SEATS {
                    return;
                }
                self.cast[u] = (tick, slot);
                // The own champion's, unless the prediction never had it.
                if u == me && self.own_cast[usize::from(slot.min(5))].abs_diff(tick) <= OWN_MATCH {
                    return;
                }
                let def = c.world().champs[u].def;
                self.cast(c, u, def, slot, pos_from_code(at), now, u == me, out);
            }
            WireEvent::Attack { unit, .. } => {
                let u = usize::from(unit);
                if u == me && self.own_swing.abs_diff(tick) <= OWN_MATCH {
                    return;
                }
                self.swing(c, u, now, out);
            }
            WireEvent::MissileSpawn { source, owner, origin, width, .. } => {
                let owner = (owner != HIDDEN).then_some(usize::from(owner));
                let code = u32::from(source);
                if owner == Some(me)
                    && self.own_missiles.iter().any(|&(t, s)| s == code && t.abs_diff(tick) <= OWN_MATCH)
                {
                    return;
                }
                let mut k = Vec::new();
                self.launch(
                    c,
                    Source::from_code(code),
                    owner,
                    pos_from_code(origin),
                    now,
                    owner == Some(me),
                    &mut k,
                );
                for mut cue in k {
                    cue.size = f32::from(width);
                    out.push(cue);
                }
            }
            WireEvent::MissileEnd { id, at, .. } => {
                // A skillshot's end, drawn where it stopped (a hit is the Damage it does).
                let Some(s) = c.shot(usize::from(id)) else { return };
                if s.homing {
                    return;
                }
                let owner = (s.owner != HIDDEN).then_some(usize::from(s.owner));
                let l = look::look(s.source, owner.and_then(|o| self.champ(c, o)), s.team);
                let mut cue = self.cue(
                    What::Fizzle,
                    s.source,
                    l,
                    pos_from_code(at),
                    None,
                    self.side(s.team, owner),
                    false,
                    None,
                );
                cue.size = s.width;
                out.push(cue);
            }
            WireEvent::Damage { target, source, amount, kind: _, from } => {
                let t = usize::from(target);
                let src = Source::from_code(u32::from(from));
                let by = (source != HIDDEN).then_some(usize::from(source));
                let team = by.map_or(1 - self.team_of(c, t), |s| self.team_of(c, s));
                let champ = by.and_then(|s| self.champ(c, s));
                let l = look::look(src, champ, team);
                let attacker = by.map(|s| self.kind(c, s));
                let sound = match (src, attacker) {
                    (Source::Fountain, _) => None,
                    (Source::Auto, Some(k)) if k.is_minion() => Some(Sfx::MinionHit),
                    _ => l.sound(Part::Impact),
                };
                let own = t == me || by == Some(me);
                let mut cue = self.cue(
                    What::Part(Part::Impact),
                    src,
                    l,
                    self.pos(c, t, now),
                    Some(target),
                    self.side(team, by),
                    own,
                    sound,
                );
                cue.size = f32::from(amount);
                if attacker.is_some_and(|k| k.is_minion()) && t != me {
                    cue.rung = 4;
                }
                out.push(cue);
            }
            WireEvent::Heal { target, from, amount } => {
                let src = Source::from_code(u32::from(from));
                // Lifesteal is every auto's: too often to hear. A relic's heal is its own.
                if matches!(src, Source::Auto | Source::Relic) {
                    return;
                }
                let t = usize::from(target);
                let team = self.team_of(c, t);
                let mut cue = self.cue(
                    What::Heal,
                    src,
                    status_look_heal(),
                    self.pos(c, t, now),
                    Some(target),
                    self.side(team, Some(t)),
                    t == me,
                    Some(Sfx::Heal),
                );
                cue.size = f32::from(amount);
                out.push(cue);
            }
            WireEvent::Shield { target, from, amount } => {
                let t = usize::from(target);
                let team = self.team_of(c, t);
                let src = Source::from_code(u32::from(from));
                let mut cue = self.cue(
                    What::Shield,
                    src,
                    status_look_shield(),
                    self.pos(c, t, now),
                    Some(target),
                    self.side(team, Some(t)),
                    t == me,
                    Some(Sfx::ShieldUp),
                );
                cue.size = f32::from(amount);
                out.push(cue);
            }
            WireEvent::Death { unit, killer } => {
                let u = usize::from(unit);
                let kind = self.kind(c, u);
                let team = self.team_of(c, u);
                let sound = match kind {
                    Kind::Champion if u == me => Some(Sfx::DeathOwn),
                    Kind::Champion => Some(Sfx::DeathChampion),
                    Kind::Turret => Some(Sfx::TurretFall),
                    Kind::Inhibitor => Some(Sfx::InhibitorFall),
                    Kind::Nexus => Some(Sfx::NexusFall),
                    Kind::Fountain => None,
                    Kind::Melee | Kind::Caster | Kind::Siege | Kind::Super => Some(Sfx::MinionDie),
                };
                let l = look::look(Source::Auto, self.champ(c, u), team);
                let mut cue = self.cue(
                    What::Death(kind),
                    Source::Auto,
                    l,
                    self.pos(c, u, now),
                    Some(unit),
                    self.side(team, Some(u)),
                    u == me,
                    sound,
                );
                cue.rating = match kind {
                    Kind::Champion => 8,
                    k if k.is_structure() => 9,
                    _ => 2,
                };
                cue.rung = match kind {
                    Kind::Champion | Kind::Turret | Kind::Inhibitor | Kind::Nexus => cue.rung.min(1),
                    _ => 4,
                };
                out.push(cue);
                if kind == Kind::Inhibitor {
                    self.inhibitors[usize::from(team.min(1))] = false;
                }
                self.announcer.death(u, kind, team, usize::from(killer), now);
            }
            WireEvent::Respawn { unit } => {
                let u = usize::from(unit);
                self.announcer.respawn(u);
                if u == me {
                    let l = look::look(Source::Auto, self.champ(c, u), self.team);
                    out.push(self.cue(
                        What::Respawn,
                        Source::Auto,
                        l,
                        self.pos(c, u, now),
                        Some(unit),
                        Side::Own,
                        true,
                        Some(Sfx::Respawn),
                    ));
                }
            }
            WireEvent::LevelUp { unit, .. } => {
                let u = usize::from(unit);
                // Seen, or the viewer's own (anyone's level is on the scoreboard, not on the map).
                if u != me && !c.known(u) {
                    return;
                }
                let l = look::look(Source::Auto, self.champ(c, u), self.team_of(c, u));
                let sound = (u == me).then_some(Sfx::LevelUp);
                out.push(self.cue(
                    What::LevelUp,
                    Source::Auto,
                    l,
                    self.pos(c, u, now),
                    Some(unit),
                    self.side(self.team_of(c, u), Some(u)),
                    u == me,
                    sound,
                ));
            }
            WireEvent::Blink { unit, to } => {
                let u = usize::from(unit);
                if u == me && self.own_blink.abs_diff(tick) <= OWN_MATCH {
                    return;
                }
                let from = self.seen.get(u).filter(|s| s.known).map_or(pos_from_code(to), |s| s.pos);
                self.blink(c, u, from, pos_from_code(to), tick, out);
            }
            WireEvent::Area { at, radius, team, source, owner, .. } => {
                let src = Source::from_code(u32::from(source));
                let owner = (owner != HIDDEN).then_some(usize::from(owner));
                let l = look::look(src, owner.and_then(|o| self.champ(c, o)), team);
                let mut cue = self.cue(
                    What::Part(Part::Blast),
                    src,
                    l,
                    pos_from_code(at),
                    None,
                    self.side(team, owner),
                    owner == Some(me),
                    l.sound(Part::Blast),
                );
                cue.size = f32::from(radius);
                out.push(cue);
            }
            WireEvent::ZoneSpawn { at, radius, team, source, owner, next, .. } => {
                // A blast that lands late: its warning, where it will land (whoever the team sees cast it).
                if next <= tick + 3 {
                    return;
                }
                let src = Source::from_code(u32::from(source));
                let owner = (owner != HIDDEN).then_some(usize::from(owner));
                let l = look::look(src, owner.and_then(|o| self.champ(c, o)), team);
                let mut cue = self.cue(
                    What::Part(Part::Windup),
                    src,
                    l,
                    pos_from_code(at),
                    None,
                    self.side(team, owner),
                    owner == Some(me),
                    l.sound(Part::Windup),
                );
                cue.size = f32::from(radius);
                out.push(cue);
            }
            WireEvent::Relic { pad, beam, .. } => {
                let at = c.world().relics.get(usize::from(pad)).map_or(Vec2::ZERO, |r| r.pos);
                let l = look::look(Source::Relic, None, self.team);
                let sound = Some(if beam { Sfx::RelicBeam } else { Sfx::RelicTake });
                let mut cue =
                    self.cue(What::Relic { beam }, Source::Relic, l, at, None, Side::Ally, false, sound);
                cue.rating = 5;
                out.push(cue);
            }
            WireEvent::Ping { kind, at, .. } => {
                let sound = match kind {
                    el_proto::order::ping::DANGER => Sfx::PingDanger,
                    el_proto::order::ping::COMING => Sfx::PingComing,
                    el_proto::order::ping::GONE => Sfx::PingGone,
                    el_proto::order::ping::ATTACK => Sfx::PingAttack,
                    el_proto::order::ping::BACK => Sfx::PingBack,
                    _ => Sfx::PingLook,
                };
                let l = look::look(Source::Auto, None, self.team);
                out.push(self.cue(
                    What::Ping(kind),
                    Source::Auto,
                    l,
                    pos_from_code(at),
                    None,
                    Side::Ally,
                    false,
                    Some(sound),
                ));
            }
            WireEvent::Vote { open, passed, .. } => {
                if open && !self.vote_open {
                    out.push(self.ui(c, Sfx::UiVote, now));
                }
                self.vote_open = open;
                self.surrendered |= passed;
            }
            WireEvent::Outcome { winner } => {
                if !self.over {
                    self.over = true;
                    self.announcer.outcome(winner, self.surrendered, now);
                }
            }
            // Its sound is the interface's (an augment taken: see `interface`).
            WireEvent::Augment { .. } => {}
            WireEvent::Leave { .. } | WireEvent::ZoneEnd { .. } | WireEvent::Nothing => {}
        }
    }

    fn ui(&self, c: &dyn Scene, sfx: Sfx, now: f64) -> Cue {
        let l = look::look(Source::Auto, None, self.team);
        self.cue(What::Ui, Source::Auto, l, self.pos(c, self.seat, now), None, Side::Own, true, Some(sfx))
    }

    /// The units' flags, where one rose since the last frame; and a turret turning on the viewer.
    fn flags(&mut self, c: &dyn Scene, now: f64, out: &mut Vec<Cue>) {
        let me = self.seat;
        for i in 0..MAX_UNITS {
            let now_seen = match c.player() {
                Some(p) if i == me => {
                    let u = &p.pred.units[me];
                    u.alive.then(|| {
                        let q = p.pred.quantize_teams(me)[usize::from(self.team)];
                        (u.generation as u8, q.flags, q.target, q.kind, q.team, p.own_pos(now))
                    })
                }
                _ => c
                    .sample(i, now)
                    .map(|s| (s.q.generation, s.q.flags, s.q.target, s.q.kind, s.q.team, s.pos)),
            };
            let Some((generation, f, target, kind, team, pos)) = now_seen else {
                self.seen[i].known = false;
                continue;
            };
            let was = self.seen[i];
            let same = was.known && was.generation == generation;
            let rose = if same { f & !was.flags } else { 0 };
            for (bit, s) in Status::FLAGS {
                if rose & bit == 0 {
                    continue;
                }
                // Minions' statuses are drawn, not heard.
                let minion = Kind::from_u8(kind).is_minion();
                let own = i == me;
                let side = if own {
                    Side::Own
                } else if team == self.team {
                    Side::Ally
                } else {
                    Side::Enemy
                };
                let l = status_look(s);
                let sound = (!minion).then_some(s.sound());
                let mut cue =
                    self.cue(What::Status(s), Source::Auto, l, pos, Some(i as u8), side, own, sound);
                cue.rating = s.ratings().of(side);
                cue.rung = if own {
                    0
                } else if minion {
                    4
                } else {
                    mix::rung(side, cue.rating)
                };
                out.push(cue);
            }
            if Kind::from_u8(kind) == Kind::Turret
                && team != self.team
                && usize::from(target) == me
                && (!same || usize::from(was.target) != me)
            {
                let l = look::look(Source::Turret, None, team);
                out.push(self.cue(
                    What::TurretWarn,
                    Source::Turret,
                    l,
                    pos,
                    Some(i as u8),
                    Side::Enemy,
                    true,
                    Some(Sfx::TurretWarn),
                ));
            }
            self.seen[i] = Seen { known: true, generation, flags: f, target, kind, team, pos };
        }
    }

    /// The own champion's interface: a point spent, an item bought or sold, the cards dealt, an
    /// augment taken, a reroll.
    fn interface(&mut self, c: &dyn Scene, now: f64, out: &mut Vec<Cue>) {
        let Some(p) = c.player().filter(|_| c.started()) else { return };
        let ch = &p.pred.champs[self.seat];
        let ui = Ui {
            points_spent: ch.ranks.iter().map(|&r| u16::from(r)).sum(),
            items: ch.items,
            n_aug: ch.n_aug,
            screen_open: ch.screen.open,
            offers: ch.screen.offers,
        };
        let Some(was) = self.ui.replace(ui) else { return };
        if ui == was {
            return;
        }
        let none = el_sim::content::items::NONE;
        let count = |items: &[u8]| items.iter().filter(|&&i| i != none).count();
        if ui.points_spent > was.points_spent {
            out.push(self.ui(c, Sfx::UiLevel, now));
        }
        if count(&ui.items) > count(&was.items)
            || (ui.items != was.items && count(&ui.items) == count(&was.items))
        {
            out.push(self.ui(c, Sfx::UiBuy, now));
        } else if count(&ui.items) < count(&was.items) {
            out.push(self.ui(c, Sfx::UiSell, now));
        }
        if ui.n_aug > was.n_aug {
            out.push(self.ui(c, Sfx::UiAugment, now));
        } else if ui.screen_open && !was.screen_open {
            out.push(self.ui(c, Sfx::UiCard, now));
        } else if ui.screen_open && was.screen_open && ui.offers != was.offers {
            out.push(self.ui(c, Sfx::UiReroll, now));
        }
    }

    /// What the match's clock calls for: the minions' march, an inhibitor standing again.
    fn clock(&mut self, c: &dyn Scene, now: f64) {
        let t = c.world().tick;
        let at = |s: f32| el_sim::ticks(s);
        let soon = at(rules::FIRST_WAVE - 30.0);
        let wave = at(rules::FIRST_WAVE);
        if self.tick > 0 && self.tick < soon && t >= soon && t < soon + at(5.0) {
            self.announcer.minions(true, now);
        }
        if self.tick > 0 && self.tick < wave && t >= wave && t < wave + at(5.0) {
            self.announcer.minions(false, now);
        }
        for team in 0..2u8 {
            let slot = el_sim::units::structure_slot(team, el_sim::units::structure::INHIBITOR);
            let up = c.world().units.get(slot).is_some_and(|u| u.alive);
            let k = usize::from(team);
            if up && !self.inhibitors[k] {
                self.announcer.inhibitor_back(now);
            }
            self.inhibitors[k] = up;
        }
        self.tick = t;
    }
}

/// A status's look: its own palette, whoever caused it.
fn status_look(s: Status) -> Look {
    let mut l = look::look(Source::Auto, None, 0);
    l.palette = s.palette();
    l.ratings = s.ratings();
    l
}

fn status_look_heal() -> Look {
    let mut l = look::look(Source::Relic, None, 0);
    l.palette = world::HEAL;
    l.ratings = look::Ratings::new(4, 3, 2);
    l
}

fn status_look_shield() -> Look {
    let mut l = look::look(Source::Relic, None, 0);
    l.palette = world::SHIELD;
    l.ratings = look::Ratings::new(5, 3, 3);
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::quant::pos_code;
    use el_proto::unit::QUnit;

    fn client(seat: usize) -> Client {
        let picks = core::array::from_fn(|k| k as u8);
        Client::new(el_sim::map::Map::leaked(), seat, picks, [0; 4], 0)
    }

    fn told(c: &mut Client, tick: u32, e: WireEvent) {
        let n = c.feed_n;
        c.feed[(n % FEED as u64) as usize] = (tick, e);
        c.feed_n += 1;
    }

    fn sounds(cues: &[Cue]) -> Vec<Sfx> {
        cues.iter().filter_map(|c| c.sound).collect()
    }

    #[test]
    fn an_enemy_cast_waits_for_the_drawn_clock_and_sounds_its_family() {
        let mut c = client(0);
        let mut d = Director::new(0);
        let mut out = Vec::new();
        // Seat 6 is Seren (dusk); her Skyfall, her ultimate.
        told(&mut c, 300, WireEvent::Cast { unit: 6, slot: 3, at: pos_code(Vec2::new(5_000.0, 1_900.0)) });
        // The unsynced clock draws everyone at `now × 30 − 3` ticks: at 9 s, tick 267.
        d.frame(&c, 9.0, &mut out);
        assert!(out.is_empty(), "a tick 300 cast isn't seen at tick 267: {out:?}");
        d.frame(&c, 10.2, &mut out);
        let s = sounds(&out);
        assert!(s.contains(&Sfx::DuskCast) && s.contains(&Sfx::DuskUlt), "{s:?}");
        let cast = out.iter().find(|q| q.sound == Some(Sfx::DuskUlt)).unwrap();
        assert_eq!(cast.side, Side::Enemy);
        assert!(cast.rung <= 2, "an enemy's ultimate is near the top of the ladder: {}", cast.rung);
    }

    #[test]
    fn a_blast_that_lands_late_warns_whoever_sees_its_ground() {
        let mut c = client(1);
        let mut d = Director::new(1);
        let mut out = Vec::new();
        // Orrin's Ash Field (seat 1 holds Orrin), cast by an unseen enemy into our sight.
        let src = Source::Ability { champ: 1, slot: 1 };
        told(
            &mut c,
            400,
            WireEvent::ZoneSpawn {
                id: 3,
                generation: 1,
                team: 1,
                source: src.code() as u16,
                owner: HIDDEN,
                follows: false,
                at: pos_code(Vec2::new(4_000.0, 1_900.0)),
                radius: 250,
                next: 430,
                period: 0,
                end: 430,
            },
        );
        d.frame(&c, 1.0, &mut out);
        let w = out.iter().find(|q| q.what == What::Part(Part::Windup)).expect("a warning");
        assert_eq!((w.sound, w.side, w.size), (Some(Sfx::EmberWindup), Side::Enemy, 250.0));
        assert!(w.rung <= 2, "a warning to dodge by is near the top: {}", w.rung);
    }

    #[test]
    fn the_own_cast_plays_from_the_prediction_once() {
        let mut c = client(2);
        let mut d = Director::new(2);
        let mut out = Vec::new();
        c.own_feed[0] = (500, OwnEvent::Cast { slot: 0, at: Vec2::new(3_000.0, 1_900.0) });
        c.own_feed_n = 1;
        d.frame(&c, 1.0, &mut out);
        let n = out.iter().filter(|q| q.what == What::Part(Part::Cast)).count();
        assert_eq!(n, 1);
        assert!(out.iter().all(|q| q.own && q.rung == 0));
        // The server's word of it, a round trip on: already played.
        out.clear();
        told(&mut c, 506, WireEvent::Cast { unit: 2, slot: 0, at: pos_code(Vec2::new(3_000.0, 1_900.0)) });
        d.frame(&c, 1.1, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_stun_rising_is_heard_once_and_a_hit_by_its_ability() {
        let mut c = client(0);
        let mut d = Director::new(0);
        let mut out = Vec::new();
        let rec = |flags: u32| QUnit {
            slot: 7,
            generation: 1,
            kind: Kind::Champion as u8,
            team: 1,
            pos: pos_code(Vec2::new(3_000.0, 1_900.0)),
            hp: 1_000,
            max_hp: 1_000,
            flags,
            target: el_proto::unit::NO_TARGET,
            ..QUnit::default()
        };
        c.tracks[7].push(10, rec(0));
        d.frame(&c, 1.0, &mut out);
        c.tracks[7].push(11, rec(flags::STUNNED));
        d.frame(&c, 1.1, &mut out);
        d.frame(&c, 1.2, &mut out);
        let stuns = out.iter().filter(|q| q.what == What::Status(Status::Stun)).count();
        assert_eq!(stuns, 1, "{out:?}");
        out.clear();
        // Hollis's Toll (bronze) on seat 7, from seat 2.
        let src = Source::Ability { champ: 7, slot: 0 };
        told(
            &mut c,
            20,
            WireEvent::Damage { target: 7, source: 2, amount: 120, kind: 1, from: src.code() as u16 },
        );
        d.frame(&c, 2.0, &mut out);
        assert_eq!(sounds(&out), [Sfx::BronzeImpact]);
    }

    #[test]
    fn a_match_watched_shows_and_sounds_both_sides_and_owns_nothing() {
        use el_sim::game::SeatInput;
        use el_sim::path::PathScratch;
        let mut m = Match::new(el_sim::map::Map::leaked(), 11, core::array::from_fn(|k| k as u8));
        let mut scratch = PathScratch::new();
        let mut told = Spectator::default();
        let mut d = Director::watching();
        let mut prev = vec![Vec2::ZERO; MAX_UNITS];
        let mut out = Vec::new();
        let mut cues = Vec::new();
        let mut lines = Vec::new();
        for _ in 0..el_sim::ticks(240.0) {
            for (p, u) in prev.iter_mut().zip(m.units.iter()) {
                *p = u.pos;
            }
            let mut inputs = [SeatInput::default(); SEATS];
            for (seat, input) in inputs.iter_mut().enumerate() {
                el_sim::bot::think(&m, seat, input);
            }
            m.step(&inputs, &mut scratch);
            told.tell(&m, true);
            let at = f64::from(m.tick);
            let scene = Watched { m: &m, told: &told, prev: &prev, blend: 1.0, at };
            out.clear();
            let now = at / f64::from(el_sim::TICK_HZ);
            d.frame(&scene, now, &mut out);
            cues.extend_from_slice(&out);
            lines.extend(d.announcer.next(now, false));
        }
        let n = |f: &dyn Fn(&Cue) -> bool| cues.iter().filter(|q| f(q)).count();
        assert_eq!(n(&|q| q.own), 0, "a watcher has no champion of their own");
        assert_eq!(n(&|q| q.side == Side::Own), 0);
        for side in [Side::Ally, Side::Enemy] {
            let casts = n(&|q| q.side == side && q.what == What::Part(Part::Cast) && q.sound.is_some());
            assert!(casts > 10, "{side:?}'s casts heard: {casts}");
            let hits = n(&|q| q.side == side && q.what == What::Part(Part::Impact));
            assert!(hits > 100, "{side:?}'s hits: {hits}");
        }
        assert!(n(&|q| q.what == What::Part(Part::Launch)) > 50, "missiles and swings");
        assert!(n(&|q| q.what == What::Death(Kind::Champion)) > 0, "a champion's fall");
        assert!(n(&|q| matches!(q.what, What::Status(_))) > 0, "a status rising");
        // Every cue is where the match is: on the map, not at its corner.
        assert_eq!(n(&|q| q.at == Vec2::ZERO), 0);
        // The announcer welcomes the watcher and calls the first blood.
        assert!(lines.contains(&Sfx::VoiceWelcome), "{lines:?}");
        assert!(lines.contains(&Sfx::VoiceFirstBlood), "{lines:?}");
    }
}
