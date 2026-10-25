//! A match: ten champions, their structures and minions, missiles and zones, stepped at 30 Hz.
//!
//! [`Match::step`] is a closed function of the match's seed and every seat's input: the server
//! runs it, the record replays it, the browser re-steps it to watch, and the owner's prediction
//! runs a seat's share of it ([`Match::step_seat`]). Nothing in it allocates; every table is sized
//! in [`Match::new`].

use alloc::boxed::Box;
use el_proto::Order;
use glam::Vec2;

use crate::content::StatMods;
use crate::content::rules::{self, START_GOLD, START_LEVEL};
use crate::events::{Event, MAX_EVENTS};
use crate::handle::Handle;
use crate::map::{self, Map, VIS_H, VIS_WORDS};
use crate::math::{self, Rng};
use crate::missile::{MAX_MISSILES, Missile, Source};
use crate::path::PathScratch;
use crate::storage::{BitSet, FixedVec, FreeList, boxed};
use crate::ticks;
use crate::units::{
    self, FIRST_MINION, Kind, MAX_UNITS, SEATS, Unit, structure, structure_slot, team_of_seat,
};
use crate::zone::{MAX_ZONES, Zone};

/// Orders a seat may give in one tick.
pub const MAX_ORDERS_TICK: usize = 4;
/// "No ability being cast".
pub const NO_SLOT: u8 = u8::MAX;

/// The parts of a tick, in order, as [`Match::step_marked`] reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Orders,
    Champions,
    Minions,
    Separate,
    Missiles,
    World,
    Vision,
}

/// What one seat does in one tick: what it holds, and the orders that land this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatInput {
    /// WASD: 0 none, 1 to 8 the eight directions (`el_proto::InputCmd::steer`).
    pub steer: u8,
    pub orders: [Order; MAX_ORDERS_TICK],
    pub n: u8,
}

impl SeatInput {
    pub fn push(&mut self, o: Order) -> bool {
        if usize::from(self.n) >= MAX_ORDERS_TICK {
            return false;
        }
        self.orders[usize::from(self.n)] = o;
        self.n += 1;
        true
    }

    pub fn orders(&self) -> &[Order] {
        &self.orders[..usize::from(self.n)]
    }
}

/// What a champion means to do when nothing else is going on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Intent {
    /// Stand, and fight what comes into range.
    #[default]
    Idle,
    Move(Vec2),
    AttackMove(Vec2),
    Attack(Handle),
    /// Stand and fight what comes into range, never moving.
    Hold,
    /// Walk into range of a unit, then cast at it.
    CastAt {
        slot: u8,
        target: Handle,
    },
}

/// A cast being wound up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Casting {
    pub slot: u8,
    pub left: u16,
    pub at: Vec2,
    pub target: Handle,
    pub rank: u8,
}

impl Default for Casting {
    fn default() -> Self {
        Self { slot: NO_SLOT, left: 0, at: Vec2::ZERO, target: Handle::NONE, rank: 0 }
    }
}

/// A dash under way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dash {
    pub active: bool,
    pub to: Vec2,
    /// Dashing to a unit (else to `to`).
    pub target: Handle,
    pub speed: f32,
    pub radius: f32,
    pub source: Source,
    pub rank: u8,
    /// It ends by this tick whatever happens.
    pub until: u32,
}

impl Default for Dash {
    fn default() -> Self {
        Self {
            active: false,
            to: Vec2::ZERO,
            target: Handle::NONE,
            speed: 0.0,
            radius: 0.0,
            source: Source::Auto,
            rank: 0,
            until: 0,
        }
    }
}

/// An augment screen: open, its tier, its three offers, and which were rerolled.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Screen {
    pub open: bool,
    pub tier: u8,
    pub offers: [u8; 3],
    pub rerolled: u8,
}

/// A champion's state beyond its unit's: its level, purse, kit and what it's doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Champ {
    pub def: u8,
    pub level: u8,
    pub xp: f32,
    pub gold: f32,
    /// Q, W, E, R.
    pub ranks: [u8; 4],
    pub points: u8,
    /// The tick each of Q, W, E, R, D, F is ready.
    pub ready: [u32; 6],
    pub items: [u8; crate::content::items::SLOTS],
    pub augments: [u8; 5],
    pub n_aug: u8,
    pub screen: Screen,
    /// Screens earned, and resolved.
    pub screens_earned: u8,
    pub screens_done: u8,
    /// When each augment's trigger may fire again.
    pub aug_ready: [u32; 5],
    pub auto_hits: u16,
    pub intent: Intent,
    pub cast: Casting,
    pub dash: Dash,
    pub steer: u8,
    /// Respawns at this tick (while dead).
    pub dead_until: u32,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub last_hits: u16,
    /// Combo Breaker: which of the last window's ticks it couldn't move, and how many.
    pub immobile: [u64; 4],
    pub immobile_n: u16,
    /// Items' and augments' stat changes, summed.
    pub mods: StatMods,
}

impl Champ {
    fn new(def: u8) -> Self {
        Self {
            def,
            level: START_LEVEL,
            xp: 0.0,
            gold: START_GOLD,
            ranks: [0; 4],
            points: START_LEVEL,
            ready: [0; 6],
            items: [crate::content::items::NONE; crate::content::items::SLOTS],
            augments: [crate::content::augments::NONE; 5],
            n_aug: 0,
            screen: Screen::default(),
            screens_earned: 1,
            screens_done: 0,
            aug_ready: [0; 5],
            auto_hits: 0,
            intent: Intent::Idle,
            cast: Casting::default(),
            dash: Dash::default(),
            steer: 0,
            dead_until: 0,
            kills: 0,
            deaths: 0,
            assists: 0,
            last_hits: 0,
            immobile: [0; 4],
            immobile_n: 0,
            mods: StatMods::ZERO,
        }
    }

    pub fn def(&self) -> &'static crate::content::ChampDef {
        &crate::content::champions::CHAMPIONS[usize::from(self.def)]
    }

    pub fn augments(&self) -> &[u8] {
        &self.augments[..usize::from(self.n_aug)]
    }
}

/// A health relic's pad.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Relic {
    pub pos: Vec2,
    pub up: bool,
    /// When it next appears (while it's down).
    pub next: u32,
    /// When its beam strikes (0: none coming).
    pub beam_at: u32,
}

/// What each team sees this tick.
#[derive(Clone)]
pub struct Vision {
    pub grid: [[u64; VIS_WORDS * VIS_H]; 2],
    /// Units each team sees (its own included).
    pub units: [BitSet; 2],
    pub missiles: [BitSet; 2],
}

pub struct Match {
    pub map: &'static Map,
    pub seed: u64,
    pub tick: u32,
    pub rng: Rng,
    pub units: Box<[Unit]>,
    pub champs: [Champ; SEATS],
    pub missiles: Box<[Missile]>,
    pub zones: Box<[Zone]>,
    pub(crate) minion_slots: FreeList,
    pub next_wave: u32,
    pub waves: u16,
    pub relics: [Relic; 4],
    /// When each team's inhibitor comes back (while it's down).
    pub inhib_back: [u32; 2],
    pub vision: Vision,
    /// Each augment screen's tier, the same for everyone.
    pub aug_tiers: [u8; 4],
    pub events: FixedVec<Event>,
    pub events_dropped: u32,
    /// The team that won, once a nexus falls.
    pub outcome: Option<u8>,
    /// A* searches left this tick.
    pub(crate) path_budget: u8,
}

/// A* searches a match may run in one tick (the rest of who wants one walks straight a tick more).
pub const PATH_BUDGET: u8 = 16;

impl Match {
    /// A new match: `picks` is each seat's champion (an index into `CHAMPIONS`).
    pub fn new(map: &'static Map, seed: u64, picks: [u8; SEATS]) -> Match {
        let mut m = Match {
            map,
            seed,
            tick: 0,
            rng: Rng::new(seed),
            units: boxed(MAX_UNITS, Unit::default()),
            champs: picks.map(Champ::new),
            missiles: boxed(MAX_MISSILES, Missile::default()),
            zones: boxed(MAX_ZONES, Zone::default()),
            minion_slots: FreeList::full(MAX_UNITS - FIRST_MINION),
            next_wave: ticks(rules::FIRST_WAVE),
            waves: 0,
            relics: [Relic::default(); 4],
            inhib_back: [0; 2],
            vision: Vision {
                grid: [[0; VIS_WORDS * VIS_H]; 2],
                units: [BitSet::new(MAX_UNITS), BitSet::new(MAX_UNITS)],
                missiles: [BitSet::new(MAX_MISSILES), BitSet::new(MAX_MISSILES)],
            },
            aug_tiers: [0; 4],
            events: FixedVec::new(MAX_EVENTS, Event::default()),
            events_dropped: 0,
            outcome: None,
            path_budget: PATH_BUDGET,
        };
        m.reset(seed, picks);
        m
    }

    /// Starts the match over in place, as a new one with `seed` and `picks` (a shard reuses its
    /// matches; nothing is allocated).
    pub fn reset(&mut self, seed: u64, picks: [u8; SEATS]) {
        self.seed = seed;
        self.tick = 0;
        self.rng = Rng::new(seed);
        // Every slot's generation from 0 again, as a new match's: a match reset in place is the
        // same match as one built from its seed, so its record replays on a fresh one.
        self.units.fill(Unit::default());
        self.champs = picks.map(Champ::new);
        self.missiles.fill(Missile::default());
        self.zones.fill(Zone::default());
        self.minion_slots.refill();
        self.next_wave = ticks(rules::FIRST_WAVE);
        self.waves = 0;
        self.inhib_back = [0; 2];
        self.events.clear();
        self.events_dropped = 0;
        self.outcome = None;
        for team in [map::DAWN, map::DUSK] {
            self.spawn_structures(team);
        }
        for (k, (team, dawn)) in [(map::DAWN, map::RELICS[0]), (map::DAWN, map::RELICS[1])]
            .into_iter()
            .chain([(map::DUSK, map::RELICS[0]), (map::DUSK, map::RELICS[1])])
            .enumerate()
        {
            let first = if k % 2 == 0 { rules::RELIC_OUTER_FIRST } else { rules::RELIC_INNER_FIRST };
            self.relics[k] =
                Relic { pos: map::for_team(team, dawn), up: false, next: ticks(first), beam_at: 0 };
        }
        // The augment screens' tiers: drawn from the seed, the first two never both Silver.
        for (s, t) in self.aug_tiers.iter_mut().enumerate() {
            *t = (math::hash32(seed as u32 ^ (seed >> 32) as u32, 0xA0 + s as u32) % 3) as u8;
        }
        if self.aug_tiers[0] == 0 && self.aug_tiers[1] == 0 {
            self.aug_tiers[1] = 1;
        }
        for seat in 0..SEATS {
            self.spawn_champion(seat);
            self.refresh_stats(seat);
            let u = &mut self.units[seat];
            u.hp = u.stats.max_hp;
            u.mana = u.stats.max_mana;
        }
        self.update_vision();
    }

    fn spawn_structures(&mut self, team: u8) {
        let at = |d: map::Disc| map::for_team(team, d.0);
        let list = [
            (
                structure::OUTER,
                Kind::Turret,
                at(map::OUTER_TURRET),
                rules::OUTER_TURRET_HP,
                map::OUTER_TURRET.1,
            ),
            (
                structure::INHIB_TURRET,
                Kind::Turret,
                at(map::INHIB_TURRET),
                rules::INHIB_TURRET_HP,
                map::INHIB_TURRET.1,
            ),
            (
                structure::INHIBITOR,
                Kind::Inhibitor,
                at(map::INHIBITOR),
                rules::INHIBITOR_HP,
                map::INHIBITOR.1,
            ),
            (
                structure::NEXUS_TURRET_A,
                Kind::Turret,
                at(map::NEXUS_TURRETS[0]),
                rules::NEXUS_TURRET_HP,
                map::NEXUS_TURRETS[0].1,
            ),
            (
                structure::NEXUS_TURRET_B,
                Kind::Turret,
                at(map::NEXUS_TURRETS[1]),
                rules::NEXUS_TURRET_HP,
                map::NEXUS_TURRETS[1].1,
            ),
            (structure::NEXUS, Kind::Nexus, at(map::NEXUS), rules::NEXUS_HP, map::NEXUS.1),
            (structure::FOUNTAIN, Kind::Fountain, map::for_team(team, map::FOUNTAIN), 1.0, 80.0),
        ];
        for (k, kind, pos, hp, radius) in list {
            let i = structure_slot(team, k);
            let generation = self.units[i].generation.wrapping_add(1);
            let shoots = kind.shoots();
            self.units[i] = Unit {
                alive: true,
                kind,
                team,
                generation,
                pos,
                radius,
                hp,
                stats: units::Stats {
                    max_hp: hp,
                    ad: if shoots { rules::TURRET_DAMAGE } else { 0.0 },
                    armor: rules::STRUCTURE_ARMOR,
                    mr: rules::STRUCTURE_ARMOR,
                    attack_speed: rules::TURRET_ATTACK_SPEED,
                    range: if kind == Kind::Fountain { rules::FOUNTAIN_RANGE } else { rules::TURRET_RANGE },
                    ..units::Stats::default()
                },
                spawned: self.tick,
                ..Unit::default()
            };
        }
    }

    /// Puts a seat's champion on its platform, whole.
    pub(crate) fn spawn_champion(&mut self, seat: usize) {
        let team = team_of_seat(seat);
        let def = self.champs[seat].def();
        let generation = self.units[seat].generation.wrapping_add(1);
        let pos = map::spawn_point(team, seat % units::TEAM_SIZE);
        let facing = if team == map::DAWN { 0.0 } else { core::f32::consts::PI };
        let stats = self.units[seat].stats;
        self.units[seat] = Unit {
            alive: true,
            kind: Kind::Champion,
            team,
            generation,
            pos,
            facing,
            radius: def.radius,
            hp: stats.max_hp,
            mana: stats.max_mana,
            stats,
            spawned: self.tick,
            ..Unit::default()
        };
        let c = &mut self.champs[seat];
        c.intent = Intent::Idle;
        c.cast = Casting::default();
        c.dash = Dash::default();
        c.immobile = [0; 4];
        c.immobile_n = 0;
    }

    /// A minion of `team` at `pos`; `None` if the table is full.
    pub(crate) fn spawn_minion(&mut self, team: u8, kind: Kind, pos: Vec2) -> Option<usize> {
        let i = usize::from(self.minion_slots.pop()?) + FIRST_MINION;
        let def = rules::minion_def(kind);
        let growth = 1.0
            + rules::MINION_GROWTH
                * crate::math::floor(self.tick as f32 / ticks(rules::MINION_GROWTH_SECS) as f32);
        let mut stats = def.stats;
        stats.max_hp *= growth;
        stats.ad *= growth;
        let generation = self.units[i].generation.wrapping_add(1);
        let facing = if team == map::DAWN { 0.0 } else { core::f32::consts::PI };
        self.units[i] = Unit {
            alive: true,
            kind,
            team,
            generation,
            pos,
            facing,
            radius: def.radius,
            hp: stats.max_hp,
            stats,
            spawned: self.tick,
            ..Unit::default()
        };
        Some(i)
    }

    /// A minion's slot goes back to be used again.
    pub(crate) fn free_minion(&mut self, i: usize) {
        if i >= FIRST_MINION {
            self.minion_slots.push((i - FIRST_MINION) as u16);
        }
    }

    pub(crate) fn event(&mut self, e: Event) {
        if !self.events.try_push(e) {
            self.events_dropped += 1;
        }
    }

    /// One tick of the whole match.
    pub fn step(&mut self, inputs: &[SeatInput; SEATS], scratch: &mut PathScratch) {
        self.step_marked(inputs, scratch, &mut |_| {});
    }

    /// [`step`](Self::step), calling `mark` as each phase ends (the bench times them; the sim never
    /// reads a clock).
    pub fn step_marked(
        &mut self,
        inputs: &[SeatInput; SEATS],
        scratch: &mut PathScratch,
        mark: &mut impl FnMut(Phase),
    ) {
        self.events.clear();
        self.path_budget = PATH_BUDGET;
        if self.outcome.is_some() {
            self.tick += 1;
            return;
        }
        for (seat, input) in inputs.iter().enumerate() {
            self.apply_input(seat, input, scratch);
        }
        for seat in 0..SEATS {
            self.refresh_stats(seat);
        }
        mark(Phase::Orders);
        for seat in 0..SEATS {
            self.champion_tick(seat, scratch);
        }
        mark(Phase::Champions);
        self.minions_and_structures(scratch);
        mark(Phase::Minions);
        self.separate();
        mark(Phase::Separate);
        self.step_missiles();
        self.step_zones();
        mark(Phase::Missiles);
        self.augment_ticks();
        self.economy_tick();
        self.world_tick();
        mark(Phase::World);
        self.update_vision();
        mark(Phase::Vision);
        self.tick += 1;
    }

    /// One tick of one seat's champion alone, on this match as it stands: what the owner's
    /// prediction runs (`docs/NETCODE.md`, "Your champion is predicted"). Its orders, its stats,
    /// its movement, casts and dashes; nothing else moves, and the clock doesn't advance.
    pub fn step_seat(&mut self, seat: usize, input: &SeatInput, scratch: &mut PathScratch) {
        self.events.clear();
        self.path_budget = PATH_BUDGET;
        self.apply_input(seat, input, scratch);
        self.refresh_stats(seat);
        self.champion_tick(seat, scratch);
    }

    /// The seat whose champion holds unit `i` (champions are slots 0–9).
    pub fn seat_of(i: usize) -> Option<usize> {
        (i < SEATS).then_some(i)
    }

    /// The champion of `seat` is dead.
    pub fn dead(&self, seat: usize) -> bool {
        !self.units[seat].alive
    }

    /// May `seat` shop and pick augments now: dead, or on its own platform.
    pub fn in_shop(&self, seat: usize) -> bool {
        let team = team_of_seat(seat);
        self.dead(seat) || {
            let p = map::for_team(team, map::PLATFORM.0);
            math::distance(self.units[seat].pos, p) <= map::PLATFORM.1 + 60.0
        }
    }

    /// The FNV-1a digest of everything that decides what happens next: what the golden tests, the
    /// record and the referee compare.
    pub fn hash(&self) -> u64 {
        let mut h = crate::hash::Fnv::default();
        h.u32(self.tick);
        h.u64(self.rng.state());
        for (i, u) in self.units.iter().enumerate() {
            if !u.alive {
                continue;
            }
            h.u32(i as u32 | u32::from(u.generation) << 8 | (u.kind as u32) << 24 | u32::from(u.team) << 28);
            h.vec2(u.pos);
            h.f32(u.facing);
            h.f32(u.hp);
            h.f32(u.mana);
            h.f32(u.shield);
            h.u32(u.target.idx as u32 | u32::from(u.target.generation) << 16);
            h.u32(u.attack_ready);
            h.u32(u32::from(u.windup));
            h.u32(
                u.cc.stun
                    ^ u.cc.root.rotate_left(8)
                    ^ u.cc.silence.rotate_left(16)
                    ^ u.cc.airborne.rotate_left(24),
            );
            h.f32(u.cc.slow);
            for b in &u.buffs {
                if b.kind != 0 {
                    h.u32(u32::from(b.kind) | b.until << 8);
                    h.f32(b.value);
                }
            }
            h.u32(u32::from(u.path.len) | u32::from(u.path.next) << 8);
        }
        for c in &self.champs {
            h.u32(u32::from(c.def) | u32::from(c.level) << 8 | u32::from(c.points) << 16);
            h.f32(c.xp);
            h.f32(c.gold);
            h.u32(u32::from_le_bytes(c.ranks));
            for r in c.ready {
                h.u32(r);
            }
            for it in c.items {
                h.u32(u32::from(it));
            }
            for a in c.augments() {
                h.u32(u32::from(*a));
            }
            h.u32(
                u32::from(c.screen.open) | u32::from(c.screen.tier) << 8 | u32::from(c.screen.rerolled) << 16,
            );
            h.u32(u32::from_le_bytes([
                c.screen.offers[0],
                c.screen.offers[1],
                c.screen.offers[2],
                c.screens_done,
            ]));
            h.u32(c.dead_until);
            h.u32(u32::from(c.cast.slot) | u32::from(c.cast.left) << 8);
            h.u32(u32::from(c.dash.active));
            h.vec2(c.dash.to);
        }
        for (i, m) in self.missiles.iter().enumerate() {
            if m.alive {
                h.u32(i as u32 | u32::from(m.generation) << 16);
                h.vec2(m.pos);
                h.u32(m.source.code());
            }
        }
        for (i, z) in self.zones.iter().enumerate() {
            if z.alive {
                h.u32(i as u32);
                h.vec2(z.pos);
                h.u32(z.next);
                h.u32(z.end);
            }
        }
        h.u32(self.next_wave);
        h.u32(u32::from(self.waves));
        for r in &self.relics {
            h.u32(u32::from(r.up) | r.next << 1);
            h.u32(r.beam_at);
        }
        h.u32(self.inhib_back[0]);
        h.u32(self.inhib_back[1]);
        h.0
    }
}
