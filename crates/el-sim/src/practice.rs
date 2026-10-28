//! Practice (`docs/FX.md`, step 4): a match for casting abilities, to make and judge their looks.
//! Seat 0 is the player's: every rank open, nothing on cooldown, its mana full, gold to spend. The
//! other nine are target dummies at the Causeway's middle, a champion's stats each, on both
//! sides: some stand, some walk a loop, one is shielded every few seconds. No one dies (a champion
//! brought to nothing is made whole), no minions march, and the dummies never fight back.
//!
//! It's a match like any other otherwise, made practice by [`Match::set_practice`] after its
//! reset, and its record says so (`record::Frame::Practice`), so it replays as it was played;
//! nothing here runs in a match that isn't practice. The dummies' doings are inputs, from
//! [`think`], recorded as a bot's are.

use el_proto::Order;
use glam::Vec2;

use crate::content::champions::ability;
use crate::content::rules::MAX_LEVEL;
use crate::game::{Match, SeatInput};
use crate::map::{MAP_W_HALF, MID_Y};
use crate::missile::Source;
use crate::ticks;
use crate::units::SEATS;

/// The player's seat.
pub const PLAYER: usize = 0;
/// The gold the player starts with: enough for any build.
pub const GOLD: f32 = 50_000.0;
/// The dummy shielded, how much, and how often (s).
pub const SHIELDED: usize = 7;
const SHIELD: f32 = 400.0;
const SHIELD_EVERY: f32 = 4.0;
/// How often a walking dummy turns back (s), and how far it walks.
const WALK_EVERY: f32 = 3.0;
const WALK: f32 = 360.0;

/// What a dummy does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dummy {
    Stands,
    Walks,
    Shielded,
}

/// Each seat's place at the start (the player's a little back from the dummies, facing them) and,
/// for a dummy, what it does: three of Dawn's beside the player, five of Dusk's across from it.
pub const PLACES: [(Vec2, Option<Dummy>); SEATS] = [
    (Vec2::new(MAP_W_HALF - 900.0, MID_Y), None),
    (Vec2::new(MAP_W_HALF - 650.0, MID_Y + 260.0), Some(Dummy::Stands)),
    (Vec2::new(MAP_W_HALF - 650.0, MID_Y - 260.0), Some(Dummy::Walks)),
    (Vec2::new(MAP_W_HALF - 1_150.0, MID_Y + 200.0), Some(Dummy::Stands)),
    (Vec2::new(MAP_W_HALF - 1_150.0, MID_Y - 200.0), Some(Dummy::Stands)),
    (Vec2::new(MAP_W_HALF - 200.0, MID_Y), Some(Dummy::Stands)),
    (Vec2::new(MAP_W_HALF - 50.0, MID_Y + 330.0), Some(Dummy::Walks)),
    (Vec2::new(MAP_W_HALF - 50.0, MID_Y - 330.0), Some(Dummy::Shielded)),
    (Vec2::new(MAP_W_HALF + 300.0, MID_Y + 150.0), Some(Dummy::Stands)),
    (Vec2::new(MAP_W_HALF + 300.0, MID_Y - 150.0), Some(Dummy::Walks)),
];

impl Match {
    /// Makes this (just reset) match a practice one: every champion at the top level with every
    /// rank, the player's gold, everyone at their place, no waves.
    pub fn set_practice(&mut self) {
        self.practice = true;
        self.next_wave = u32::MAX;
        for (seat, &(at, _)) in PLACES.iter().enumerate() {
            let c = &mut self.champs[seat];
            c.level = MAX_LEVEL;
            c.xp = 0.0;
            c.points = 0;
            c.ranks = [0, 1, 2, 3].map(|slot| Match::max_rank(slot, MAX_LEVEL));
            if seat == PLAYER {
                c.gold = GOLD;
            }
            self.refresh_stats(seat);
            let u = &mut self.units[seat];
            u.pos = at;
            u.facing = if at.x < MAP_W_HALF - 100.0 { 0.0 } else { core::f32::consts::PI };
            u.hp = u.stats.max_hp;
            u.mana = u.stats.max_mana;
        }
        // (What each team sees follows at the end of the first tick: a client's copy is told it.)
    }

    /// Whether seat `seat`'s champion is a dummy: one that never fights.
    pub fn is_dummy(&self, seat: usize) -> bool {
        self.practice && seat < SEATS && seat != PLAYER
    }

    /// The player's own rules, at the end of each tick it's stepped (the server's step and the
    /// player's own prediction alike): nothing on cooldown, mana full.
    pub(crate) fn practice_seat(&mut self, seat: usize) {
        if !self.practice || seat != PLAYER {
            return;
        }
        let now = self.tick;
        let c = &mut self.champs[seat];
        for r in c.ready.iter_mut() {
            *r = (*r).min(now);
        }
        let u = &mut self.units[seat];
        u.mana = u.stats.max_mana;
    }

    /// Practice's tick: the player's rules, and the shielded dummy's shield.
    pub(crate) fn practice_tick(&mut self) {
        if !self.practice {
            return;
        }
        self.practice_seat(PLAYER);
        let every = ticks(SHIELD_EVERY);
        if self.units[SHIELDED].alive && self.tick.is_multiple_of(every) {
            self.shield(SHIELDED, SHIELD, SHIELD_EVERY, Source::Relic);
        }
    }

    /// A champion brought to nothing in practice is made whole instead of dying.
    pub(crate) fn spared(&mut self, victim: usize) -> bool {
        if !self.practice || victim >= SEATS {
            return false;
        }
        let u = &mut self.units[victim];
        u.hp = u.stats.max_hp;
        true
    }
}

/// How far a standing dummy may be moved (pulled, knocked) before it walks back to its place.
const STRAYED: f32 = 60.0;

/// This tick's orders for dummy `seat` (practice only): a walker turns back now and then; the
/// rest stand at their places, and walk back to them when they're moved off.
pub fn think(m: &Match, seat: usize, out: &mut SeatInput) {
    let Some((at, Some(dummy))) = PLACES.get(seat).copied() else { return };
    let every = ticks(WALK_EVERY);
    if dummy != Dummy::Walks {
        let u = &m.units[seat];
        if (m.tick + seat as u32).is_multiple_of(ticks(0.5)) && u.alive && u.pos.distance(at) > STRAYED {
            out.push(Order::Move { to: at }.quantized());
        }
        return;
    }
    if !m.tick.is_multiple_of(every) {
        return;
    }
    let out_and_back = if (m.tick / every).is_multiple_of(2) { WALK } else { 0.0 };
    // On the wire's grid, as every order is (the record holds it so).
    let to = at + Vec2::new(0.0, if at.y > MID_Y { out_and_back } else { -out_and_back });
    out.push(Order::Move { to }.quantized());
}

/// Each of the player's abilities, as it's cast in practice: its slot and whether it's aimed at a
/// point, at a unit, or at nothing (for the `fx` suite's loop over a kit).
pub fn kit(def: u8) -> [(u8, crate::content::Target); 4] {
    [0u8, 1, 2, 3].map(|slot| (slot, ability(def, slot).target))
}
