//! A champion's tick: its orders turned into what it means to do, then crowd control, dashes,
//! casts winding up, attacks, and walking. The owner's prediction runs this same code
//! (`Match::step_seat`), so a click moves the champion on the next frame and the server agrees.

use el_proto::{Aim, Order, UnitRef};
use glam::Vec2;

use crate::content::Effect;
use crate::content::Target;
use crate::content::champions::{BLINK_SLOT, MARK_DASH_SPEED, MARK_SLOT, ability};
use crate::content::rules::{self, COMBO_IMMOBILE, COMBO_IMMUNE, COMBO_WINDOW};
use crate::events::Event;
use crate::game::{Casting, Dash, Intent, Match, NO_SLOT, SeatInput};
use crate::map;
use crate::math;
use crate::missile::Source;
use crate::path::PathScratch;
use crate::ticks;
use crate::units::{BuffKind, Kind, SEATS, team_of_seat};

/// How far an attack-moving champion looks for a fight.
const ATTACK_MOVE_ACQUIRE: f32 = 600.0;
/// Ticks between a chaser's new paths.
const CHASE_REPATH: u32 = 10;

/// `from` toward `to`, at most `range` away.
fn within(from: Vec2, to: Vec2, range: f32) -> Vec2 {
    from + math::clamp_len(to - from, range)
}

/// The world direction of a WASD steer (1 along +x, then counter-clockwise by 45°).
pub fn steer_dir(steer: u8) -> Vec2 {
    math::dir(f32::from(steer.saturating_sub(1)) * core::f32::consts::FRAC_PI_4)
}

impl Match {
    pub(crate) fn apply_input(&mut self, seat: usize, input: &SeatInput, scratch: &mut PathScratch) {
        self.champs[seat].steer = input.steer.min(8);
        for o in input.orders() {
            self.apply_order(seat, *o, scratch);
        }
    }

    /// The unit an order names, if it's alive and is the life the client meant.
    fn resolve(&self, r: UnitRef) -> Option<usize> {
        let i = usize::from(r.slot);
        let u = self.units.get(i)?;
        (u.alive && u.generation as u8 == r.generation).then_some(i)
    }

    pub(crate) fn apply_order(&mut self, seat: usize, o: Order, scratch: &mut PathScratch) {
        // What a champion may do dead: spend points, pick augments, shop.
        match o {
            Order::LevelSkill { slot } => return self.level_skill(seat, slot),
            Order::PickAugment { card } => return self.pick_augment(seat, card),
            Order::Reroll { card } => return self.reroll_augment(seat, card),
            Order::Buy { item } => return self.buy(seat, item),
            Order::Sell { inv } => return self.sell(seat, inv),
            // And speak to the team.
            Order::Ping { kind, at, unit } => return self.ping(seat, kind, at, unit),
            Order::Surrender { yes } => return self.vote(seat, yes),
            _ => {}
        }
        if !self.units[seat].alive {
            return;
        }
        let team = team_of_seat(seat);
        match o {
            Order::Move { to } => {
                self.units[seat].windup = 0;
                self.champs[seat].intent = Intent::Move(to);
                self.route(seat, to, scratch);
            }
            Order::AttackMove { to } => {
                self.champs[seat].intent = Intent::AttackMove(to);
                self.route(seat, to, scratch);
            }
            Order::Attack { unit } => {
                if let Some(t) = self.resolve(unit).filter(|&t| self.attackable(team, t)) {
                    let h = self.units[t].handle(t);
                    if self.units[seat].target != h {
                        self.units[seat].windup = 0;
                    }
                    self.champs[seat].intent = Intent::Attack(h);
                    self.units[seat].path.clear();
                }
            }
            Order::Cast { slot, aim } => self.order_cast(seat, slot, aim, scratch),
            Order::Stop => {
                self.champs[seat].intent = Intent::Idle;
                self.units[seat].path.clear();
                self.units[seat].windup = 0;
            }
            Order::Hold => {
                self.champs[seat].intent = Intent::Hold;
                self.units[seat].path.clear();
            }
            _ => {}
        }
    }

    fn order_cast(&mut self, seat: usize, slot: u8, aim: Aim, scratch: &mut PathScratch) {
        let now = self.tick;
        if usize::from(slot) >= el_proto::order::slot::COUNT {
            return;
        }
        let c = self.champs[seat];
        let u = self.units[seat];
        if u.cc.disabled(now) || c.dash.active || c.cast.slot != NO_SLOT {
            return;
        }
        if slot == MARK_SLOT
            && let Some(t) = self.marked_target(seat)
        {
            // The mark's dash: free while the mark lasts, and it uses the mark up.
            let mark = BuffKind::Marked as u8;
            for b in self.units[t].buffs.iter_mut() {
                if b.kind == mark {
                    b.until = now;
                }
            }
            self.champs[seat].dash = Dash {
                active: true,
                to: self.units[t].pos,
                target: self.units[t].handle(t),
                speed: MARK_DASH_SPEED,
                radius: 0.0,
                source: Source::Auto,
                rank: 0,
                until: now + ticks(2.0),
            };
            self.units[seat].windup = 0;
            return;
        }
        let a = ability(c.def, slot);
        let rank = if slot < 4 { c.ranks[usize::from(slot)] } else { 1 };
        if rank == 0 || c.ready[usize::from(slot)] > now {
            return;
        }
        if slot < BLINK_SLOT && u.cc.silenced(now) {
            return;
        }
        if u.mana < a.cost[usize::from(rank - 1)] {
            return;
        }
        let team = team_of_seat(seat);
        match a.target {
            Target::Dir | Target::Point => {
                if let Aim::Point(p) = aim {
                    self.begin_cast(seat, slot, p, crate::handle::Handle::NONE);
                }
            }
            Target::Caster => self.begin_cast(seat, slot, u.pos, crate::handle::Handle::NONE),
            Target::Enemy | Target::Ally => {
                let t = match aim {
                    Aim::Unit(r) => self.resolve(r),
                    _ if a.target == Target::Ally => Some(seat),
                    _ => None,
                };
                let Some(t) = t else { return };
                // Abilities are for champions and minions: structures take only attacks.
                let ok = if a.target == Target::Enemy {
                    self.attackable(team, t) && !self.units[t].kind.is_structure()
                } else {
                    t < SEATS && self.units[t].team == team && self.units[t].alive
                };
                if !ok {
                    return;
                }
                let h = self.units[t].handle(t);
                if t == seat || self.in_range(seat, t, a.range) {
                    self.begin_cast(seat, slot, self.units[t].pos, h);
                } else {
                    self.champs[seat].intent = Intent::CastAt { slot, target: h };
                    let to = self.units[t].pos;
                    self.route(seat, to, scratch);
                }
            }
        }
    }

    fn begin_cast(&mut self, seat: usize, slot: u8, at: Vec2, target: crate::handle::Handle) {
        let now = self.tick;
        let c = self.champs[seat];
        let a = ability(c.def, slot);
        let rank = if slot < 4 { c.ranks[usize::from(slot)] } else { 1 };
        let r = usize::from(rank - 1);
        let haste = if slot < 4 { self.units[seat].stats.haste } else { 0.0 };
        let u = &mut self.units[seat];
        u.mana -= a.cost[r];
        u.windup = 0;
        let to = at - u.pos;
        if to.length_squared() > 1.0 {
            u.facing = math::angle(to);
        }
        self.champs[seat].ready[usize::from(slot)] = now + ticks(a.cooldown[r] / (1.0 + haste / 100.0));
        self.champs[seat].cast = Casting { slot, left: ticks(a.windup) as u16, at, target, rank };
        self.event(Event::Cast { unit: seat as u16, slot, at });
        if slot < BLINK_SLOT {
            self.cast_triggers(seat);
        }
        if self.champs[seat].cast.left == 0 {
            self.fire_cast(seat);
        }
    }

    /// The wound-up cast lets go.
    fn fire_cast(&mut self, seat: usize) {
        let c = self.champs[seat].cast;
        self.champs[seat].cast = Casting::default();
        let def = self.champs[seat].def;
        let a = ability(def, c.slot);
        let source = Source::Ability { champ: def, slot: c.slot };
        let rank = c.rank.saturating_sub(1);
        let pos = self.units[seat].pos;
        let now = self.tick;
        match a.effect {
            Effect::Line { speed, width, pierce, .. } => {
                let dir = math::normalize_or(c.at - pos, math::dir(self.units[seat].facing));
                self.spawn_line(seat, source, rank, dir, a.range, speed, width, pierce);
            }
            Effect::Homing { speed, .. } => {
                if let Some(t) = self.live(c.target) {
                    self.spawn_homing(seat, t, source, rank, speed, 0.0);
                }
            }
            Effect::Strike { ops } => {
                if let Some(t) = self.live(c.target) {
                    self.apply_ops(seat, t, ops, rank, source);
                    if self.units[t].team != self.units[seat].team {
                        self.ability_hit(seat, t);
                    }
                }
            }
            Effect::Area { radius, delay, filter, .. } => {
                let at = if a.target == Target::Caster { pos } else { within(pos, c.at, a.range) };
                self.spawn_zone(seat, source, rank, at, radius, filter, false, ticks(delay), 0, 0);
            }
            Effect::Zone { radius, secs, period, filter, follows, .. } => {
                let at = if a.target == Target::Caster { pos } else { within(pos, c.at, a.range) };
                self.spawn_zone(
                    seat,
                    source,
                    rank,
                    at,
                    radius,
                    filter,
                    follows,
                    0,
                    ticks(secs),
                    ticks(period).max(1),
                );
            }
            Effect::Dash { speed, radius, .. } => {
                let target =
                    self.live(c.target).map_or(crate::handle::Handle::NONE, |t| self.units[t].handle(t));
                let to = self.map.snap(within(pos, c.at, a.range));
                self.champs[seat].dash =
                    Dash { active: true, to, target, speed, radius, source, rank, until: now + ticks(2.0) };
            }
            Effect::Blink { radius, ops } => {
                let to = self.map.snap(within(pos, c.at, a.range));
                let to = self.gate(seat, to);
                self.units[seat].pos = to;
                self.units[seat].path.clear();
                self.event(Event::Blink { unit: seat as u16, from: pos, to });
                if radius > 0.0 {
                    self.ops_around(seat, to, radius, ops, rank, source);
                }
            }
            Effect::OnSelf { ops } => self.apply_ops(seat, seat, ops, rank, source),
        }
    }

    /// `ops` on every enemy within `radius` of `at`.
    fn ops_around(
        &mut self,
        caster: usize,
        at: Vec2,
        radius: f32,
        ops: &'static [crate::content::Op],
        rank: u8,
        source: Source,
    ) {
        let team = self.units[caster].team;
        for j in 0..self.units.len() {
            let u = &self.units[j];
            let reach = radius + u.radius;
            if u.kind.is_structure()
                || (u.pos - at).length_squared() > reach * reach
                || !self.attackable(team, j)
            {
                continue;
            }
            self.apply_ops(caster, j, ops, rank, source);
            self.ability_hit(caster, j);
        }
    }

    fn dash_tick(&mut self, seat: usize) {
        let now = self.tick;
        let d = self.champs[seat].dash;
        let pos = self.units[seat].pos;
        let target = self.live(d.target);
        let (goal, contact) = match target {
            Some(t) => (self.units[t].pos, self.units[t].radius + self.units[seat].radius),
            None => (d.to, 0.0),
        };
        let step = d.speed * crate::DT;
        let gap = math::distance(pos, goal) - contact;
        if gap > step && now < d.until {
            let next = pos + math::normalize_or(goal - pos, math::dir(self.units[seat].facing)) * step;
            if self.map.walkable(next) {
                self.units[seat].pos = self.gate(seat, next);
                self.units[seat].facing = math::angle(goal - pos);
            } else {
                self.champs[seat].dash.until = now;
            }
            return;
        }
        let to = if contact > 0.0 {
            goal - math::normalize_or(goal - pos, math::dir(0.0)) * contact
        } else {
            goal
        };
        if self.map.walkable(to) {
            self.units[seat].pos = self.gate(seat, to);
        }
        self.champs[seat].dash.active = false;
        self.units[seat].path.clear();
        let ops = d.source.ops();
        if d.radius > 0.0 {
            let at = self.units[seat].pos;
            self.ops_around(seat, at, d.radius, ops, d.rank, d.source);
        } else if let Some(t) = target {
            if !ops.is_empty() {
                self.apply_ops(seat, t, ops, d.rank, d.source);
                self.ability_hit(seat, t);
            }
        } else if !ops.is_empty() {
            self.apply_ops(seat, seat, ops, d.rank, d.source);
        }
    }

    /// Before the gates open, a champion stays on its side of them.
    fn gate(&self, seat: usize, p: Vec2) -> Vec2 {
        if self.tick >= ticks(rules::GATES_OPEN) {
            return p;
        }
        let line = map::BASE.0.x + 900.0;
        if team_of_seat(seat) == map::DAWN {
            Vec2::new(p.x.min(line), p.y)
        } else {
            Vec2::new(p.x.max(crate::MAP_W - line), p.y)
        }
    }

    // ---- moving --------------------------------------------------------------------------

    /// Units a tick unit `i` covers, slows applied.
    pub(crate) fn step_len(&self, i: usize) -> f32 {
        let u = &self.units[i];
        u.stats.move_speed * (1.0 - u.cc.slow(self.tick)) * crate::DT
    }

    /// Moves `i` up to `step` straight toward `to`, sliding along walls it meets.
    pub(crate) fn slide_toward(&mut self, i: usize, to: Vec2, step: f32) -> bool {
        let from = self.units[i].pos;
        let d = to - from;
        let len = math::length(d);
        if len < 1e-3 || step <= 0.0 {
            return false;
        }
        let dir = d / len;
        let s = step.min(len);
        let mut p = from + dir * s;
        if !self.map.walkable(p) {
            let x = from + Vec2::new(dir.x * s, 0.0);
            let y = from + Vec2::new(0.0, dir.y * s);
            p = if dir.x != 0.0 && self.map.walkable(x) {
                x
            } else if dir.y != 0.0 && self.map.walkable(y) {
                y
            } else {
                return false;
            };
        }
        let p = if i < SEATS { self.gate(i, p) } else { p };
        let u = &mut self.units[i];
        u.pos = p;
        let moved = p - from;
        if moved.length_squared() > 1e-6 {
            u.facing = math::angle(moved);
        }
        true
    }

    /// Walks `i` along its path, `step` units, turning its corners.
    pub(crate) fn follow_path(&mut self, i: usize, step: f32) {
        let mut left = step;
        for _ in 0..3 {
            let Some(corner) = self.units[i].path.corner() else { return };
            let d = math::distance(self.units[i].pos, corner);
            if d <= left {
                let before = self.units[i].pos;
                if !self.slide_toward(i, corner, d) || math::distance(self.units[i].pos, corner) > 1.0 {
                    // Couldn't reach it (pushed off, or a wall in the way): find a new way.
                    self.units[i].repath_at = self.tick;
                    let _ = before;
                    return;
                }
                self.units[i].path.next += 1;
                left -= d;
            } else {
                self.slide_toward(i, corner, left);
                return;
            }
        }
    }

    /// A path for `i` to `to`. Champions always get one (at most a few a tick: their orders);
    /// minions only while the tick's budget lasts, and walk straight otherwise.
    pub(crate) fn route(&mut self, i: usize, to: Vec2, scratch: &mut PathScratch) {
        if i >= SEATS {
            if self.path_budget == 0 {
                let u = &mut self.units[i];
                u.path.clear();
                u.path.points[0] = to;
                u.path.len = 1;
                return;
            }
            self.path_budget -= 1;
        }
        let from = self.units[i].pos;
        let mut path = self.units[i].path;
        crate::path::find(self.map, scratch, from, to, &mut path);
        self.units[i].path = path;
    }

    /// `i` goes after unit `t`: straight when nothing's between them, else by a path renewed now
    /// and then as `t` moves.
    pub(crate) fn chase(&mut self, i: usize, t: usize, scratch: &mut PathScratch) {
        let goal = self.units[t].pos;
        let step = self.step_len(i);
        if self.map.walk_clear(self.units[i].pos, goal) {
            self.units[i].path.clear();
            self.slide_toward(i, goal, step);
            return;
        }
        if self.units[i].path.is_empty() || self.units[i].repath_at <= self.tick {
            self.route(i, goal, scratch);
            self.units[i].repath_at = self.tick + CHASE_REPATH;
        }
        self.follow_path(i, step);
    }

    // ---- the tick ------------------------------------------------------------------------

    pub(crate) fn champion_tick(&mut self, seat: usize, scratch: &mut PathScratch) {
        let now = self.tick;
        if !self.units[seat].alive {
            let back = self.champs[seat].dead_until;
            if back > 0 && now >= back {
                self.spawn_champion(seat);
                self.refresh_stats(seat);
                let u = &mut self.units[seat];
                u.hp = u.stats.max_hp;
                u.mana = u.stats.max_mana;
                self.champs[seat].dead_until = 0;
                self.event(Event::Respawn { unit: seat as u16 });
            }
            return;
        }
        {
            let u = &mut self.units[seat];
            u.hp = (u.hp + u.stats.hp_regen * crate::DT).min(u.stats.max_hp);
            u.mana = (u.mana + u.stats.mana_regen * crate::DT).min(u.stats.max_mana);
            if u.shield_until <= now {
                u.shield = 0.0;
            }
        }
        self.combo_breaker(seat);
        let start = self.units[seat].pos;
        if self.units[seat].cc.disabled(now) {
            self.champs[seat].cast = Casting::default();
            self.champs[seat].dash.active = false;
            self.units[seat].windup = 0;
        } else if self.champs[seat].dash.active {
            self.dash_tick(seat);
        } else if self.champs[seat].cast.slot != NO_SLOT {
            let c = &mut self.champs[seat].cast;
            c.left = c.left.saturating_sub(1);
            if c.left == 0 {
                self.fire_cast(seat);
            }
        } else if self.units[seat].windup > 0 {
            self.attack_tick(seat);
        } else {
            self.act(seat, scratch);
        }
        self.units[seat].moved = self.units[seat].pos - start;
    }

    /// What a champion does with a free tick: its intent.
    fn act(&mut self, seat: usize, scratch: &mut PathScratch) {
        let now = self.tick;
        let team = team_of_seat(seat);
        let rooted = self.units[seat].cc.rooted(now);
        let step = self.step_len(seat);
        let steer = self.champs[seat].steer;
        if steer != 0 {
            if !rooted {
                let to = self.units[seat].pos + steer_dir(steer) * 1_000.0;
                self.slide_toward(seat, to, step);
            }
            self.champs[seat].intent = Intent::Idle;
            self.units[seat].path.clear();
            return;
        }
        let range = self.units[seat].stats.range;
        match self.champs[seat].intent {
            Intent::Idle | Intent::Hold => {
                if let Some(t) = self
                    .nearest_enemy(seat, range, true, true)
                    .or_else(|| self.structure_in_range(seat, range))
                {
                    self.try_attack(seat, t);
                }
            }
            Intent::Move(_) => {
                if !rooted {
                    self.follow_path(seat, step);
                }
                if self.units[seat].path.is_empty() {
                    self.champs[seat].intent = Intent::Idle;
                }
            }
            Intent::Attack(h) => match self.live(h) {
                Some(t) if self.attackable(team, t) => {
                    if !self.try_attack(seat, t) && !rooted {
                        self.chase(seat, t, scratch);
                    }
                }
                _ => self.champs[seat].intent = Intent::Idle,
            },
            Intent::AttackMove(to) => {
                let found = self
                    .nearest_enemy(seat, ATTACK_MOVE_ACQUIRE, true, true)
                    .or_else(|| self.structure_in_range(seat, ATTACK_MOVE_ACQUIRE));
                match found {
                    Some(t) => {
                        if !self.try_attack(seat, t) && !rooted {
                            self.chase(seat, t, scratch);
                        }
                    }
                    None => {
                        if self.units[seat].path.is_empty() && math::distance(self.units[seat].pos, to) > 50.0
                        {
                            self.route(seat, to, scratch);
                        }
                        if !rooted {
                            self.follow_path(seat, step);
                        }
                        if self.units[seat].path.is_empty() {
                            self.champs[seat].intent = Intent::Idle;
                        }
                    }
                }
            }
            Intent::CastAt { slot, target } => {
                let a = ability(self.champs[seat].def, slot);
                let valid = self.live(target).filter(|&t| {
                    if a.target == Target::Enemy {
                        self.attackable(team, t)
                    } else {
                        self.units[t].team == team
                    }
                });
                match valid {
                    Some(t) if self.in_range(seat, t, a.range) => {
                        self.champs[seat].intent = Intent::Idle;
                        if self.champs[seat].ready[usize::from(slot)] <= now {
                            let at = self.units[t].pos;
                            self.begin_cast(seat, slot, at, target);
                        }
                    }
                    Some(t) if !rooted => self.chase(seat, t, scratch),
                    Some(_) => {}
                    None => self.champs[seat].intent = Intent::Idle,
                }
            }
        }
    }

    /// An enemy structure `seat` may hit, within `range`.
    fn structure_in_range(&self, seat: usize, range: f32) -> Option<usize> {
        let team = team_of_seat(seat);
        (crate::units::FIRST_STRUCTURE..crate::units::FIRST_MINION).find(|&j| {
            self.units[j].kind != Kind::Fountain && self.attackable(team, j) && self.in_range(seat, j, range)
        })
    }

    /// Combo Breaker: immobilised long enough, a champion shakes it off.
    fn combo_breaker(&mut self, seat: usize) {
        let now = self.tick;
        let window = ticks(COMBO_WINDOW);
        let need = ticks(COMBO_IMMOBILE) as u16;
        let held = self.units[seat].cc.rooted(now);
        let c = &mut self.champs[seat];
        let k = (now % window) as usize;
        let (w, b) = (k / 64, 1u64 << (k % 64));
        if c.immobile[w] & b != 0 {
            c.immobile_n -= 1;
        }
        if held {
            c.immobile[w] |= b;
            c.immobile_n += 1;
        } else {
            c.immobile[w] &= !b;
        }
        if held && c.immobile_n >= need {
            c.immobile = [0; 4];
            c.immobile_n = 0;
            let cc = &mut self.units[seat].cc;
            cc.stun = now;
            cc.root = now;
            cc.airborne = now;
            cc.silence = now;
            cc.slow_until = now;
            cc.immune = now + ticks(COMBO_IMMUNE);
        }
    }
}
