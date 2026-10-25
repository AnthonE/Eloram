//! The owner's own champion, at full precision: everything [`Match::step_seat`] reads or writes of
//! it, so the client's prediction starts from exactly the server's state and steps it exactly as
//! the server will (`docs/NETCODE.md`, "Your champion is predicted"). Floats travel as their bits.

use el_proto::{BitReader, BitWriter, DecodeError};
use glam::Vec2;

use crate::game::{Casting, Dash, Intent, Match, Screen};
use crate::handle::Handle;
use crate::missile::Source;
use crate::units::{Buff, Kind, MAX_BUFFS};

fn w_vec(w: &mut BitWriter<'_>, v: Vec2) {
    w.write_f32(v.x);
    w.write_f32(v.y);
}

fn r_vec(r: &mut BitReader<'_>) -> Vec2 {
    Vec2::new(r.read_f32(), r.read_f32())
}

fn w_handle(w: &mut BitWriter<'_>, h: Handle) {
    w.write_u16(h.idx);
    w.write_u16(h.generation);
}

fn r_handle(r: &mut BitReader<'_>) -> Handle {
    Handle { idx: r.read_u16(), generation: r.read_u16() }
}

impl Match {
    /// Becomes a copy of `from`, in place and without allocating: how the client's prediction
    /// starts over from its model of the server's match.
    pub fn assume(&mut self, from: &Match) {
        self.seed = from.seed;
        self.tick = from.tick;
        self.rng = from.rng;
        self.units.copy_from_slice(&from.units);
        self.champs = from.champs;
        self.missiles.copy_from_slice(&from.missiles);
        self.zones.copy_from_slice(&from.zones);
        self.minion_slots.copy_from(&from.minion_slots);
        self.next_wave = from.next_wave;
        self.waves = from.waves;
        self.relics = from.relics;
        self.inhib_back = from.inhib_back;
        self.vision.grid = from.vision.grid;
        for t in 0..2 {
            self.vision.units[t].copy_from(&from.vision.units[t]);
            self.vision.missiles[t].copy_from(&from.vision.missiles[t]);
        }
        self.aug_tiers = from.aug_tiers;
        self.headliner = from.headliner;
        self.events.clear();
        self.events_dropped = 0;
        self.outcome = from.outcome;
    }

    /// Writes the champion of `seat` whole.
    pub fn write_own(&self, seat: usize, w: &mut BitWriter<'_>) {
        let u = &self.units[seat];
        let c = &self.champs[seat];
        w.write_bool(u.alive);
        w.write_u16(u.generation);
        w_vec(w, u.pos);
        w_vec(w, u.moved);
        w.write_f32(u.facing);
        w.write_f32(u.hp);
        w.write_f32(u.mana);
        w.write_f32(u.shield);
        w.write_u32(u.shield_until);
        for t in [u.cc.stun, u.cc.root, u.cc.silence, u.cc.airborne, u.cc.slow_until, u.cc.immune] {
            w.write_u32(t);
        }
        w.write_f32(u.cc.slow);
        let live = u.buffs.iter().filter(|b| b.kind != 0 && b.until > self.tick).count();
        w.write_bits(live as u32, 4);
        for b in u.buffs.iter().filter(|b| b.kind != 0 && b.until > self.tick) {
            w.write_u8(b.kind);
            w.write_u32(b.until);
            w.write_f32(b.value);
        }
        w_handle(w, u.target);
        w.write_u32(u.attack_ready);
        w.write_u16(u.windup);
        w.write_u32(u.repath_at);
        w.write_bits(u32::from(u.path.len), 5);
        w.write_bits(u32::from(u.path.next), 5);
        for p in &u.path.points[..usize::from(u.path.len)] {
            w_vec(w, *p);
        }
        // The champion.
        w.write_u8(c.def);
        w.write_u8(c.level);
        w.write_f32(c.xp);
        w.write_f32(c.gold);
        for r in c.ranks {
            w.write_bits(u32::from(r), 3);
        }
        w.write_u8(c.points);
        for t in c.ready {
            w.write_u32(t);
        }
        for it in c.items {
            w.write_u8(it);
        }
        w.write_bits(u32::from(c.n_aug), 3);
        for a in c.augments() {
            w.write_u8(*a);
        }
        w.write_bool(c.screen.open);
        w.write_bits(u32::from(c.screen.tier), 2);
        for o in c.screen.offers {
            w.write_u8(o);
        }
        w.write_bits(u32::from(c.screen.rerolled), 3);
        w.write_bits(u32::from(c.screens_earned), 3);
        w.write_bits(u32::from(c.screens_done), 3);
        for t in c.aug_ready {
            w.write_u32(t);
        }
        w.write_u16(c.auto_hits);
        match c.intent {
            Intent::Idle => w.write_bits(0, 3),
            Intent::Move(p) => {
                w.write_bits(1, 3);
                w_vec(w, p);
            }
            Intent::AttackMove(p) => {
                w.write_bits(2, 3);
                w_vec(w, p);
            }
            Intent::Attack(h) => {
                w.write_bits(3, 3);
                w_handle(w, h);
            }
            Intent::Hold => w.write_bits(4, 3),
            Intent::CastAt { slot, target } => {
                w.write_bits(5, 3);
                w.write_u8(slot);
                w_handle(w, target);
            }
        }
        w.write_u8(c.cast.slot);
        w.write_u16(c.cast.left);
        w_vec(w, c.cast.at);
        w_handle(w, c.cast.target);
        w.write_u8(c.cast.rank);
        w.write_bool(c.dash.active);
        if c.dash.active {
            w_vec(w, c.dash.to);
            w_handle(w, c.dash.target);
            w.write_f32(c.dash.speed);
            w.write_f32(c.dash.radius);
            w.write_u32(c.dash.source.code());
            w.write_u8(c.dash.rank);
            w.write_u32(c.dash.until);
        }
        w.write_u32(c.dead_until);
        for n in [c.kills, c.deaths, c.assists, c.last_hits] {
            w.write_u16(n);
        }
        w.write_u16(c.immobile_n);
        if c.immobile_n > 0 {
            for word in c.immobile {
                w.write_u32(word as u32);
                w.write_u32((word >> 32) as u32);
            }
        }
    }

    /// Reads the champion of `seat` whole, into this (the client's) model of the match.
    pub fn read_own(&mut self, seat: usize, r: &mut BitReader<'_>) -> Result<(), DecodeError> {
        let now = self.tick;
        let def = {
            let u = &mut self.units[seat];
            u.alive = r.read_bool();
            u.kind = Kind::Champion;
            u.team = crate::units::team_of_seat(seat);
            u.generation = r.read_u16();
            u.pos = r_vec(r);
            u.moved = r_vec(r);
            u.facing = r.read_f32();
            u.hp = r.read_f32();
            u.mana = r.read_f32();
            u.shield = r.read_f32();
            u.shield_until = r.read_u32();
            u.cc.stun = r.read_u32();
            u.cc.root = r.read_u32();
            u.cc.silence = r.read_u32();
            u.cc.airborne = r.read_u32();
            u.cc.slow_until = r.read_u32();
            u.cc.immune = r.read_u32();
            u.cc.slow = r.read_f32();
            let n = r.read_bits(4) as usize;
            if n > MAX_BUFFS {
                return Err(DecodeError::Invalid);
            }
            u.buffs = [Buff::default(); MAX_BUFFS];
            for b in u.buffs.iter_mut().take(n) {
                *b = Buff { kind: r.read_u8(), until: r.read_u32(), value: r.read_f32() };
            }
            u.target = r_handle(r);
            u.attack_ready = r.read_u32();
            u.windup = r.read_u16();
            u.repath_at = r.read_u32();
            let len = r.read_bits(5) as u8;
            let next = r.read_bits(5) as u8;
            if usize::from(len) > crate::path::MAX_WAYPOINTS {
                return Err(DecodeError::Invalid);
            }
            u.path.len = len;
            u.path.next = next;
            for p in u.path.points.iter_mut().take(usize::from(len)) {
                *p = r_vec(r);
            }
            r.read_u8()
        };
        if usize::from(def) >= crate::content::champions::CHAMPIONS.len() {
            return Err(DecodeError::Invalid);
        }
        let c = &mut self.champs[seat];
        c.def = def;
        c.level = r.read_u8();
        c.xp = r.read_f32();
        c.gold = r.read_f32();
        for k in &mut c.ranks {
            *k = r.read_bits(3) as u8;
        }
        c.points = r.read_u8();
        for t in &mut c.ready {
            *t = r.read_u32();
        }
        for it in &mut c.items {
            *it = r.read_u8();
        }
        c.n_aug = r.read_bits(3) as u8;
        if usize::from(c.n_aug) > c.augments.len() {
            return Err(DecodeError::Invalid);
        }
        for k in 0..usize::from(c.n_aug) {
            c.augments[k] = r.read_u8();
        }
        c.screen = Screen {
            open: r.read_bool(),
            tier: r.read_bits(2) as u8,
            offers: [r.read_u8(), r.read_u8(), r.read_u8()],
            rerolled: 0,
        };
        c.screen.rerolled = r.read_bits(3) as u8;
        c.screens_earned = r.read_bits(3) as u8;
        c.screens_done = r.read_bits(3) as u8;
        for t in &mut c.aug_ready {
            *t = r.read_u32();
        }
        c.auto_hits = r.read_u16();
        c.intent = match r.read_bits(3) {
            0 => Intent::Idle,
            1 => Intent::Move(r_vec(r)),
            2 => Intent::AttackMove(r_vec(r)),
            3 => Intent::Attack(r_handle(r)),
            4 => Intent::Hold,
            5 => Intent::CastAt { slot: r.read_u8(), target: r_handle(r) },
            _ => return Err(DecodeError::Invalid),
        };
        c.cast = Casting {
            slot: r.read_u8(),
            left: r.read_u16(),
            at: r_vec(r),
            target: r_handle(r),
            rank: r.read_u8(),
        };
        c.dash = if r.read_bool() {
            Dash {
                active: true,
                to: r_vec(r),
                target: r_handle(r),
                speed: r.read_f32(),
                radius: r.read_f32(),
                source: Source::from_code(r.read_u32()),
                rank: r.read_u8(),
                until: r.read_u32(),
            }
        } else {
            Dash::default()
        };
        c.dead_until = r.read_u32();
        c.kills = r.read_u16();
        c.deaths = r.read_u16();
        c.assists = r.read_u16();
        c.last_hits = r.read_u16();
        c.immobile_n = r.read_u16();
        c.immobile = [0; 4];
        if c.immobile_n > 0 {
            for word in &mut c.immobile {
                *word = u64::from(r.read_u32()) | u64::from(r.read_u32()) << 32;
            }
        }
        if r.overflowed() {
            return Err(DecodeError::Truncated);
        }
        let _ = now;
        self.units[seat].radius = self.champs[seat].def().radius;
        // Stats are a function of what was read: worked out, not carried, and nothing read moved.
        self.recompute_mods(seat);
        self.units[seat].stats = self.stats_of(seat);
        Ok(())
    }
}
